//! Inverted index over file content (2층 — 본문), BM25-ranked.
//! No position info: v0.1 excludes phrase search on purpose (05 "인덱스
//! 크기 목표의 근거" — storing positions triples the index to 21.8% of
//! corpus size for a feature not in scope). Doc IDs are `PathTable` row
//! indices, so a hit maps straight back to a path with no extra lookup.
//! Spec: `docs/design/05_FAST_LOCAL_SEARCH.md`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::search::PathTable;

/// Per-thread build output, keyed by term text; merged into interned ids at the end.
type RawPostings = HashMap<String, Vec<(u32, u32)>>;
type DocLengths = HashMap<u32, u32>;

/// Terms are interned to `u32` ids. Storing the text once and referring to it
/// by id everywhere else is what keeps the index small: the per-document term
/// list used by `remove_doc` would otherwise hold a full duplicate `String` of
/// every term in every document.
type TermId = u32;

/// 05 "성능 원칙": 1MB 초과 파일은 본문 인덱싱 제외. Overridable via
/// `.vault/vault.toml` `[limits] content_bytes` (18 §7) — the doc marks it as
/// a value to tune from real use, so it can't stay a hardcoded constant.
const MAX_INDEX_BYTES: u64 = crate::config::DEFAULT_CONTENT_BYTES;
const BINARY_SNIFF_BYTES: usize = 8192; // 05 "성능 원칙": 첫 8KB 내 NUL 바이트로 바이너리 판정

// Standard BM25 defaults (Okapi BM25). Not tuned against this project's data —
// no measurement basis for different values, so don't invent one (→ 02
// "측정하지 않은 것을 측정한 척하지 않는다").
const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(BINARY_SNIFF_BYTES).any(|&b| b == 0)
}

/// Lowercased alphanumeric-run tokenizer. No stemming/stopwords — not
/// specified anywhere in the design, and BM25 term matching works fine
/// without them for code/docs; add if a real gap shows up.
fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

/// Inline `#tag` markers (Obsidian-style): a bare `#word` not preceded by a
/// word character and not a markdown heading (`# ` with a space).
///
/// This lives here, next to tokenizing, so tags come out of the *same* file
/// read as the content index. Extracting them separately meant reading every
/// file in the vault twice on open.
pub fn extract_tags(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let is_tag_char = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'/');
    let mut tags = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let prev_is_word = i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
            let next_is_tag_char = i + 1 < bytes.len() && is_tag_char(bytes[i + 1]);
            if !prev_is_word && next_is_tag_char {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && is_tag_char(bytes[end]) {
                    end += 1;
                }
                tags.push(text[start..end].to_string());
                i = end;
                continue;
            }
        }
        i += 1;
    }
    tags
}

#[derive(Debug, Clone, Copy)]
pub struct SizeBreakdown {
    pub postings: usize,
    pub term_text: usize,
    pub doc_term_ids: usize,
    pub doc_lens: usize,
    pub tags: usize,
    pub unique_terms: usize,
    pub total_postings: usize,
}

/// A doc_id slot that has been removed. Tombstoned rather than compacted so
/// the flat arrays never have to shift (which would invalidate every range
/// after it). Reclaimed on the next full build — i.e. when the vault is
/// reopened. ponytail: fine while edits per session are small; add online
/// compaction if a long session's churn ever makes the waste matter.
const TOMBSTONE: u32 = u32::MAX;

pub struct InvertedIndex {
    // Flat arena instead of a Vec per term: 130k separate Vecs cost ~3MB in
    // headers alone on a django-sized vault. Same "flat buffer + offsets"
    // rule the path table follows (05 §1층 "문자열 개별 할당 금지").
    posting_docs: Vec<u32>, // all postings grouped by term id
    posting_tfs: Vec<u8>,   // parallel to `posting_docs`; BM25 saturates, so 255 is plenty
    term_range: Vec<(u32, u32)>, // term id -> (start, len) into the arrays above
    /// Postings appended after the initial build (incremental reindex), which
    /// can't extend a term's range in place.
    overflow: HashMap<TermId, Vec<(u32, u8)>>,
    term_ids: HashMap<Box<str>, TermId>, // text -> term id; the only copy of the term text
    doc_len: DocLengths,                  // doc_id -> token count (only indexed docs)
    doc_terms: HashMap<u32, Vec<TermId>>, // doc_id -> its term ids, so remove_doc needn't scan every posting
    doc_tags: HashMap<u32, Vec<String>>,  // doc_id -> `#tags` found in it, from the same read
    avg_doc_len: f64,
    max_bytes: u64, // kept so incremental `index_doc` uses the same limit as the initial build
}

/// What one file contributes to the index, from a single read.
struct DocIndex {
    doc_len: u32,
    term_freq: HashMap<String, u32>,
    tags: Vec<String>,
}

/// Indexes one file, or `None` if it was skipped (oversized, binary,
/// unreadable, non-UTF8, empty).
fn index_one(full_path: &Path, max_bytes: u64) -> Option<DocIndex> {
    let metadata = fs::metadata(full_path).ok()?;
    if metadata.len() > max_bytes {
        return None;
    }
    let bytes = fs::read(full_path).ok()?;
    if is_binary(&bytes) {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;

    let tokens = tokenize(&text);
    if tokens.is_empty() {
        return None;
    }
    let doc_len = tokens.len() as u32;
    let mut term_freq: HashMap<String, u32> = HashMap::new();
    for token in tokens {
        *term_freq.entry(token).or_insert(0) += 1;
    }
    let mut tags = extract_tags(&text);
    tags.sort();
    tags.dedup();
    Some(DocIndex { doc_len, term_freq, tags })
}

impl InvertedIndex {
    /// Builds the index by reading every path in `table` off disk under
    /// `root`. Indexing is I/O-bound, not CPU-bound (per-file reads
    /// dominated by disk/AV latency, not tokenizing) — split across all
    /// cores so reads overlap, per 05 "성능 원칙" ("병렬 파싱, CPU 바운드").
    pub fn build(root: &Path, table: &PathTable) -> Self {
        Self::build_with(root, table, MAX_INDEX_BYTES)
    }

    /// Same, with an explicit content-size limit (`[limits] content_bytes`).
    pub fn build_with(root: &Path, table: &PathTable, max_bytes: u64) -> Self {
        let threads = std::thread::available_parallelism().map(|v| v.get()).unwrap_or(1);
        let n = table.len();
        let chunk = n.div_ceil(threads.max(1));

        type Partial = (RawPostings, DocLengths, HashMap<u32, Vec<String>>);
        let partials: Vec<Partial> = std::thread::scope(|s| {
            let mut handles = Vec::new();
            for t in 0..threads {
                let lo = t * chunk;
                let hi = ((t + 1) * chunk).min(n);
                if lo >= hi {
                    continue;
                }
                handles.push(s.spawn(move || {
                    let mut postings: RawPostings = HashMap::new();
                    let mut doc_len: DocLengths = HashMap::new();
                    let mut doc_tags: HashMap<u32, Vec<String>> = HashMap::new();
                    for i in lo..hi {
                        let doc_id = i as u32;
                        let Some(doc) = index_one(&root.join(table.path(i)), max_bytes) else { continue };
                        doc_len.insert(doc_id, doc.doc_len);
                        if !doc.tags.is_empty() {
                            doc_tags.insert(doc_id, doc.tags);
                        }
                        for (term, tf) in doc.term_freq {
                            postings.entry(term).or_default().push((doc_id, tf));
                        }
                    }
                    // No per-doc term list here: it's derived from the merged
                    // postings below, once ids exist, so no strings get copied.
                    (postings, doc_len, doc_tags)
                }));
            }
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });

        let mut merged: RawPostings = HashMap::new();
        let mut doc_len: DocLengths = HashMap::new();
        let mut doc_tags: HashMap<u32, Vec<String>> = HashMap::new();
        for (p, d, g) in partials {
            for (term, mut entries) in p {
                merged.entry(term).or_default().append(&mut entries);
            }
            doc_len.extend(d);
            doc_tags.extend(g);
        }

        let total_postings: usize = merged.values().map(|v| v.len()).sum();
        let mut index = InvertedIndex {
            posting_docs: Vec::with_capacity(total_postings),
            posting_tfs: Vec::with_capacity(total_postings),
            term_range: Vec::with_capacity(merged.len()),
            overflow: HashMap::new(),
            term_ids: HashMap::with_capacity(merged.len()),
            doc_len,
            doc_terms: HashMap::new(),
            doc_tags,
            avg_doc_len: 0.0,
            max_bytes,
        };
        for (term, entries) in merged {
            let id = index.intern(&term);
            let start = index.posting_docs.len() as u32;
            for (doc_id, tf) in entries {
                index.doc_terms.entry(doc_id).or_default().push(id);
                index.posting_docs.push(doc_id);
                index.posting_tfs.push(tf.min(u8::MAX as u32) as u8);
            }
            let len = index.posting_docs.len() as u32 - start;
            index.term_range[id as usize] = (start, len);
        }
        index.avg_doc_len = Self::compute_avg(&index.doc_len);
        index
    }

    /// Returns the id for `term`, assigning a new one (and an empty postings
    /// slot) the first time it's seen.
    fn intern(&mut self, term: &str) -> TermId {
        if let Some(&id) = self.term_ids.get(term) {
            return id;
        }
        let id = self.term_ids.len() as TermId;
        self.term_ids.insert(term.into(), id);
        self.term_range.push((0, 0));
        id
    }

    /// All (doc_id, tf) for a term: its slice of the arena plus anything
    /// appended incrementally. Tombstoned entries are skipped here so no
    /// caller has to know about them.
    fn postings_for(&self, id: TermId) -> impl Iterator<Item = (u32, u32)> + '_ {
        let (start, len) = self.term_range[id as usize];
        let (start, len) = (start as usize, len as usize);
        let base = self.posting_docs[start..start + len]
            .iter()
            .zip(&self.posting_tfs[start..start + len])
            .map(|(&doc, &tf)| (doc, tf as u32));
        let extra = self.overflow.get(&id).into_iter().flatten().map(|&(doc, tf)| (doc, tf as u32));
        base.chain(extra).filter(|&(doc, _)| doc != TOMBSTONE)
    }

    /// tag -> doc_ids that contain it, built from the index's own read.
    pub fn tags_by_doc(&self) -> &HashMap<u32, Vec<String>> {
        &self.doc_tags
    }

    fn compute_avg(doc_len: &DocLengths) -> f64 {
        if doc_len.is_empty() {
            0.0
        } else {
            doc_len.values().map(|&l| l as f64).sum::<f64>() / doc_len.len() as f64
        }
    }

    /// Removes `doc_id` from the index (file deleted, or about to be
    /// reindexed after a content change). No-op if it was never indexed
    /// (e.g. it was binary/oversized/empty — 05 "성능 원칙" exclusions).
    pub fn remove_doc(&mut self, doc_id: u32) {
        self.doc_tags.remove(&doc_id);
        let Some(term_ids) = self.doc_terms.remove(&doc_id) else { return };
        for term_id in term_ids {
            let (start, len) = self.term_range[term_id as usize];
            for slot in &mut self.posting_docs[start as usize..(start + len) as usize] {
                if *slot == doc_id {
                    *slot = TOMBSTONE;
                }
            }
            if let Some(extra) = self.overflow.get_mut(&term_id) {
                extra.retain(|&(id, _)| id != doc_id);
            }
        }
        self.doc_len.remove(&doc_id);
        self.avg_doc_len = Self::compute_avg(&self.doc_len);
    }

    /// (Re)indexes a single doc_id from disk — the incremental-indexing path
    /// (05 "이후": "변경 파일만 재파싱 → 관련 인덱스만 갱신"). Call
    /// `remove_doc` first if this doc_id was already indexed (content
    /// changed), so stale postings don't linger alongside fresh ones.
    pub fn index_doc(&mut self, root: &Path, doc_id: u32, rel_path: &str) {
        let Some(doc) = index_one(&root.join(rel_path), self.max_bytes) else { return };
        self.doc_len.insert(doc_id, doc.doc_len);
        if doc.tags.is_empty() {
            self.doc_tags.remove(&doc_id);
        } else {
            self.doc_tags.insert(doc_id, doc.tags);
        }
        let mut term_ids = Vec::with_capacity(doc.term_freq.len());
        for (term, tf) in doc.term_freq {
            let id = self.intern(&term);
            // Can't extend this term's slice of the arena in place, so new
            // postings go to the overflow map until the next full build.
            self.overflow.entry(id).or_default().push((doc_id, tf.min(u8::MAX as u32) as u8));
            term_ids.push(id);
        }
        self.doc_terms.insert(doc_id, term_ids);
        self.avg_doc_len = Self::compute_avg(&self.doc_len);
    }

    pub fn indexed_docs(&self) -> usize {
        self.doc_len.len()
    }

    /// Estimated resident bytes for the postings (doc_id + term_freq per
    /// entry) — the "+ term frequency" tier from 05's size table (budget 7.3%).
    ///
    /// Counts every structure the index actually allocates, not just the
    /// postings — an earlier version reported postings only, which hid the
    /// per-document term lists that were larger than the postings themselves.
    /// An index that under-reports its own size can't be held to a size budget.
    pub fn resident_bytes(&self) -> usize {
        let b = self.size_breakdown();
        b.postings + b.term_text + b.doc_term_ids + b.doc_lens + b.tags
    }

    /// Where the index's memory actually goes. Exposed so size work targets
    /// the biggest part instead of the most obvious one.
    pub fn size_breakdown(&self) -> SizeBreakdown {
        let overflow_entries: usize = self.overflow.values().map(|v| v.len()).sum();
        SizeBreakdown {
            // 4 bytes doc + 1 byte tf in the arena; the range table is 8 bytes
            // per term. Overflow keeps its own Vec per term until the next build.
            postings: self.posting_docs.len() * 5
                + self.term_range.len() * 8
                + overflow_entries * 5
                + self.overflow.len() * std::mem::size_of::<Vec<(u32, u8)>>(),
            term_text: self
                .term_ids
                .keys()
                .map(|t| t.len() + std::mem::size_of::<Box<str>>())
                .sum(),
            doc_term_ids: self
                .doc_terms
                .values()
                .map(|v| v.len() * 4 + std::mem::size_of::<Vec<TermId>>())
                .sum(),
            doc_lens: self.doc_len.len() * 8,
            tags: self
                .doc_tags
                .values()
                .map(|v| v.iter().map(|t| t.len() + std::mem::size_of::<String>()).sum::<usize>())
                .sum(),
            unique_terms: self.term_ids.len(),
            total_postings: self.posting_docs.len() + overflow_entries,
        }
    }

    /// BM25-ranked search over the tokenized query, best score first.
    pub fn search(&self, query: &str, top_k: usize) -> Vec<(f64, u32)> {
        let n = self.doc_len.len() as f64;
        if n == 0.0 {
            return Vec::new();
        }
        let terms = tokenize(query);
        let mut scores: HashMap<u32, f64> = HashMap::new();

        for term in &terms {
            let Some(&term_id) = self.term_ids.get(term.as_str()) else { continue };
            let df = self.postings_for(term_id).count() as f64;
            if df == 0.0 {
                continue;
            }
            let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
            for (doc_id, tf) in self.postings_for(term_id) {
                let tf = tf as f64;
                let dl = *self.doc_len.get(&doc_id).unwrap_or(&1) as f64;
                let denom = tf + BM25_K1 * (1.0 - BM25_B + BM25_B * dl / self.avg_doc_len);
                let score = idf * (tf * (BM25_K1 + 1.0)) / denom;
                *scores.entry(doc_id).or_insert(0.0) += score;
            }
        }

        let mut ranked: Vec<(f64, u32)> = scores.into_iter().map(|(id, s)| (s, id)).collect();
        ranked.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(top_k);
        ranked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vault-index-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_and_ranks_by_term_frequency() {
        let dir = temp_dir("rank");
        fs::write(dir.join("a.py"), "def parse_router(): pass").unwrap();
        fs::write(dir.join("b.py"), "router router router router router").unwrap();
        fs::write(dir.join("c.py"), "unrelated content here").unwrap();

        let table = PathTable::build(&dir);
        let index = InvertedIndex::build(&dir, &table);
        assert_eq!(index.indexed_docs(), 3);

        let hits = index.search("router", 10);
        assert_eq!(hits.len(), 2, "only a.py and b.py mention 'router'");
        let (_, top_id) = hits[0];
        assert_eq!(table.path(top_id as usize), "b.py", "higher term frequency should rank first");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn skips_binary_and_oversized_files() {
        let dir = temp_dir("skip");
        fs::write(dir.join("binary.dat"), [0u8, 1, 2, 0, 3]).unwrap();
        fs::write(dir.join("huge.txt"), vec![b'x'; (MAX_INDEX_BYTES + 1) as usize]).unwrap();
        fs::write(dir.join("normal.txt"), "hello world").unwrap();

        let table = PathTable::build(&dir);
        let index = InvertedIndex::build(&dir, &table);
        assert_eq!(index.indexed_docs(), 1, "only normal.txt should be indexed");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn empty_query_returns_no_hits() {
        let dir = temp_dir("empty-query");
        fs::write(dir.join("a.txt"), "content").unwrap();
        let table = PathTable::build(&dir);
        let index = InvertedIndex::build(&dir, &table);
        assert!(index.search("", 10).is_empty());
        assert!(index.search("nonexistentterm", 10).is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tag_extraction_ignores_headings_and_mid_word_hashes() {
        let text = "# Heading is not a tag\nSee #project/alpha and #urgent, but not foo#bar mid-word.";
        assert_eq!(extract_tags(text), vec!["project/alpha", "urgent"]);
    }

    #[test]
    fn tags_come_from_the_same_pass_as_the_content_index() {
        let dir = temp_dir("tags-in-index");
        fs::write(dir.join("a.md"), "notes about #project and #urgent").unwrap();
        fs::write(dir.join("b.md"), "no tags here").unwrap();

        let table = PathTable::build(&dir);
        let index = InvertedIndex::build(&dir, &table);
        let a_id = table.path_to_id("a.md").unwrap();
        let b_id = table.path_to_id("b.md").unwrap();

        let mut tags = index.tags_by_doc().get(&a_id).cloned().unwrap_or_default();
        tags.sort();
        assert_eq!(tags, vec!["project", "urgent"]);
        assert!(!index.tags_by_doc().contains_key(&b_id), "untagged docs shouldn't take up space");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn incremental_reindex_keeps_tags_in_sync() {
        let dir = temp_dir("tags-incremental");
        fs::write(dir.join("a.md"), "has #oldtag").unwrap();
        let table = PathTable::build(&dir);
        let mut index = InvertedIndex::build(&dir, &table);
        let id = table.path_to_id("a.md").unwrap();
        assert_eq!(index.tags_by_doc().get(&id).unwrap(), &vec!["oldtag".to_string()]);

        fs::write(dir.join("a.md"), "now has #newtag").unwrap();
        index.remove_doc(id);
        index.index_doc(&dir, id, "a.md");
        assert_eq!(index.tags_by_doc().get(&id).unwrap(), &vec!["newtag".to_string()]);

        fs::write(dir.join("a.md"), "no tags at all now").unwrap();
        index.remove_doc(id);
        index.index_doc(&dir, id, "a.md");
        assert!(!index.tags_by_doc().contains_key(&id), "stale tags must not linger");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn remove_doc_drops_its_terms_and_leaves_others_intact() {
        let dir = temp_dir("remove");
        // Note: tokenizer splits on non-alphanumeric, so "_" is a separator —
        // pick tokens with no shared substrings, not "shared_term"-style names.
        fs::write(dir.join("a.py"), "onlyinalpha bothdocs").unwrap();
        fs::write(dir.join("b.py"), "bothdocs").unwrap();

        let table = PathTable::build(&dir);
        let mut index = InvertedIndex::build(&dir, &table);
        let a_id = table.path_to_id("a.py").unwrap();

        index.remove_doc(a_id);
        assert_eq!(index.indexed_docs(), 1);
        assert!(index.search("onlyinalpha", 10).is_empty(), "a.py's own term must be gone");
        assert_eq!(index.search("bothdocs", 10).len(), 1, "b.py's posting must survive a.py's removal");

        // removing an already-removed (or never-indexed) doc_id is a no-op, not a panic.
        index.remove_doc(a_id);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn index_doc_reflects_new_content_after_a_change() {
        let dir = temp_dir("update");
        fs::write(dir.join("a.py"), "oldmarker").unwrap();

        let table = PathTable::build(&dir);
        let mut index = InvertedIndex::build(&dir, &table);
        let doc_id = table.path_to_id("a.py").unwrap();
        assert_eq!(index.search("oldmarker", 10).len(), 1);

        fs::write(dir.join("a.py"), "newmarker").unwrap();
        index.remove_doc(doc_id);
        index.index_doc(&dir, doc_id, "a.py");

        assert!(index.search("oldmarker", 10).is_empty(), "stale term must not linger");
        assert_eq!(index.search("newmarker", 10).len(), 1);

        fs::remove_dir_all(&dir).unwrap();
    }
}
