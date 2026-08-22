//! In-memory path table (1층) + fzf-style fuzzy path/filename matching.
//! Ported from `research/bench/search/bench3_final.rs` — the exact algorithm
//! that produced the measured p95 numbers this module is held to (→ `17`,
//! "검색 성능"). Not reimplemented from scratch: the scoring constants,
//! forward/backward pass, and DP are copied unchanged: this is a validated
//! reference implementation, not a fresh design.
//! Spec: `docs/design/05_FAST_LOCAL_SEARCH.md`.

use std::collections::HashMap;
use std::path::Path;

use memchr::memchr;

const SCORE_MATCH: i32 = 16;
const BONUS_BOUNDARY: i32 = 8;
const BONUS_CAMEL: i32 = 7;
const BONUS_CONSEC: i32 = 8;
const PENALTY_GAP_START: i32 = -3;
const PENALTY_GAP_EXT: i32 = -1;
const BONUS_FILENAME: i32 = 4;

#[inline(always)]
fn is_boundary(p: u8) -> bool {
    matches!(p, b'/' | b'_' | b'-' | b'.' | b' ' | b'\\')
}

/// Search scope: filename-only is the default (05 "검색 범위 기본값" — the
/// single decision with the largest performance impact); `/` in the query
/// switches to full-path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    FilenameOnly,
    FullPath,
}

/// Picks scope from the query itself, per the confirmed design: a literal
/// `/` in the query is the user asking to search the full path.
pub fn scope_for_query(query: &str) -> Scope {
    if query.contains('/') { Scope::FullPath } else { Scope::FilenameOnly }
}

/// Flat byte buffer + offset table — no per-path heap allocation (05 §1층
/// "구조: flat byte buffer + 오프셋 테이블 (문자열 개별 할당 금지)").
pub struct PathTable {
    buf: Vec<u8>,   // all paths concatenated, "/" separators kept as stored
    lower: Vec<u8>, // ascii-lowercased copy of `buf`, same length/offsets
    offs: Vec<u32>, // path i = buf[offs[i]..offs[i+1]]
    base: Vec<u32>, // filename i = buf[base[i]..offs[i+1]] (byte after last '/')
    deleted: Vec<bool>,       // tombstones — row indices (doc_ids) must stay stable
    path_index: HashMap<String, u32>, // path -> row index, for incremental add/remove/update
}

impl PathTable {
    /// Scans `root` and builds the table from every file found (reuses
    /// `crate::scan`, which already applies `.gitignore` rules).
    pub fn build(root: &Path) -> Self {
        Self::build_with(root, true, &[])
    }

    /// Same, honoring `.vault/vault.toml`'s `[ignore]` section.
    pub fn build_with(root: &Path, use_gitignore: bool, extra_patterns: &[String]) -> Self {
        let mut table = PathTable {
            buf: Vec::new(),
            lower: Vec::new(),
            offs: vec![0],
            base: Vec::new(),
            deleted: Vec::new(),
            path_index: HashMap::new(),
        };
        for path in crate::scan::scan_files_with(root, use_gitignore, extra_patterns) {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            table.add(&rel.to_string_lossy().replace('\\', "/"));
        }
        table
    }

    /// Appends a new path, returning its doc_id. If the path already exists
    /// (e.g. a Modified event that reuses `add` instead of a dedicated
    /// update path), returns the existing doc_id unchanged — content is
    /// what changed, not identity.
    pub fn add(&mut self, path: &str) -> u32 {
        if let Some(&id) = self.path_index.get(path) {
            return id;
        }
        let id = self.base.len() as u32;
        let start = self.buf.len() as u32;
        let mut last_slash = start;
        for &b in path.as_bytes() {
            self.buf.push(b);
            self.lower.push(b.to_ascii_lowercase());
            if b == b'/' {
                last_slash = self.buf.len() as u32;
            }
        }
        self.offs.push(self.buf.len() as u32);
        self.base.push(last_slash);
        self.deleted.push(false);
        self.path_index.insert(path.to_string(), id);
        id
    }

    /// Tombstones the row for `path` — the bytes stay (doc_ids of other rows
    /// must not shift) but it's skipped by search and no longer resolvable
    /// by `path_to_id`. Returns the doc_id that was removed, if any.
    pub fn remove(&mut self, path: &str) -> Option<u32> {
        let id = self.path_index.remove(path)?;
        self.deleted[id as usize] = true;
        Some(id)
    }

    pub fn path_to_id(&self, path: &str) -> Option<u32> {
        self.path_index.get(path).copied()
    }

    /// Row count including tombstones. Use for buffer sizing, not "how many
    /// live paths" — `path_index.len()` is the live count.
    pub fn len(&self) -> usize {
        self.base.len()
    }

    pub fn is_empty(&self) -> bool {
        self.base.is_empty()
    }

    pub fn path(&self, i: usize) -> &str {
        std::str::from_utf8(&self.buf[self.offs[i] as usize..self.offs[i + 1] as usize]).unwrap_or("")
    }

    /// Every live (non-tombstoned) path, in row order.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        (0..self.base.len()).filter(|&i| !self.deleted[i]).map(move |i| self.path(i))
    }

    /// Resident bytes for the flat buffers only (05 목표: 100K 파일 < 20MB).
    pub fn resident_bytes(&self) -> usize {
        self.buf.len() * 2 + self.offs.len() * 4 + self.base.len() * 4
    }

    /// Searches with `top_k` results, using all available cores — matches
    /// the configuration `17`'s 7.4ms/100K figure was measured under.
    pub fn search(&self, query: &str, top_k: usize) -> Vec<(i32, u32)> {
        let threads = std::thread::available_parallelism().map(|v| v.get()).unwrap_or(1);
        self.search_with(query, top_k, scope_for_query(query), threads)
    }

    pub fn search_with(&self, query: &str, top_k: usize, scope: Scope, threads: usize) -> Vec<(i32, u32)> {
        let needle: Vec<u8> = query.to_ascii_lowercase().into_bytes();
        if needle.is_empty() {
            return Vec::new();
        }
        let n = self.len();
        let mut all = if threads <= 1 {
            self.search_range(&needle, 0, n, top_k, scope)
        } else {
            let chunk = n.div_ceil(threads);
            std::thread::scope(|s| {
                let mut handles = Vec::new();
                for t in 0..threads {
                    let lo = t * chunk;
                    let hi = ((t + 1) * chunk).min(n);
                    if lo >= hi {
                        continue;
                    }
                    let nd = &needle;
                    handles.push(s.spawn(move || self.search_range(nd, lo, hi, top_k, scope)));
                }
                handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
            })
        };
        if all.len() > top_k {
            all.select_nth_unstable_by(top_k, |a, b| b.0.cmp(&a.0));
            all.truncate(top_k);
        }
        all.sort_unstable_by_key(|b| std::cmp::Reverse(b.0));
        all
    }

    fn search_range(&self, needle: &[u8], lo: usize, hi: usize, top_k: usize, scope: Scope) -> Vec<(i32, u32)> {
        let mut out: Vec<(i32, u32)> = Vec::with_capacity(1024);
        let (mut ps, mut pd, mut cs, mut cd) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());

        for i in lo..hi {
            if self.deleted[i] {
                continue;
            }
            let off = if scope == Scope::FilenameOnly { self.base[i] as usize } else { self.offs[i] as usize };
            let endp = self.offs[i + 1] as usize;
            let hay = &self.lower[off..endp];

            // 1. forward pass: SIMD byte search to the next query char, bail out fast.
            let Some(end_rel) = forward_end(hay, needle) else { continue };
            // 2. backward pass: tightest matching span ending at end_rel.
            let start_rel = backward_start(hay, needle, end_rel);

            // 3. DP scoring only over the tight span, not the whole path.
            let score = score_dp(
                &self.buf,
                &self.lower,
                needle,
                off + start_rel,
                off + end_rel,
                self.base[i] as usize,
                endp - off,
                &mut ps,
                &mut pd,
                &mut cs,
                &mut cd,
            );
            out.push((score, i as u32));
        }
        // 4. bounded top-K selection instead of a full sort.
        if out.len() > top_k * 4 {
            out.select_nth_unstable_by(top_k, |a, b| b.0.cmp(&a.0));
            out.truncate(top_k);
        }
        out
    }
}

#[inline(always)]
fn forward_end(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let mut pos = 0usize;
    for &q in needle {
        match memchr(q, &hay[pos..]) {
            Some(off) => pos += off + 1,
            None => return None,
        }
    }
    Some(pos)
}

#[inline(always)]
fn backward_start(hay: &[u8], needle: &[u8], end: usize) -> usize {
    let mut qi = needle.len();
    let mut i = end;
    while i > 0 && qi > 0 {
        i -= 1;
        if hay[i] == needle[qi - 1] {
            qi -= 1;
        }
    }
    i
}

#[allow(clippy::too_many_arguments)]
fn score_dp(
    raw: &[u8],
    low: &[u8],
    needle: &[u8],
    s: usize,
    e: usize,
    fbase: usize,
    plen: usize,
    ps: &mut Vec<i32>,
    pd: &mut Vec<i32>,
    cs: &mut Vec<i32>,
    cd: &mut Vec<i32>,
) -> i32 {
    let n = e - s;
    let m = needle.len();
    ps.clear();
    ps.resize(n + 1, 0);
    pd.clear();
    pd.resize(n + 1, i32::MIN / 2);
    cs.clear();
    cs.resize(n + 1, i32::MIN / 2);
    cd.clear();
    cd.resize(n + 1, i32::MIN / 2);

    for &q in needle.iter().take(m) {
        cs[0] = i32::MIN / 2;
        cd[0] = i32::MIN / 2;
        let mut in_gap = false;
        for hi in 0..n {
            let idx = s + hi;
            let gap = if in_gap { cs[hi] + PENALTY_GAP_EXT } else { cs[hi] + PENALTY_GAP_START };
            let mut bm = i32::MIN / 2;
            if low[idx] == q {
                let mut b = SCORE_MATCH;
                if idx == 0 || is_boundary(raw[idx - 1]) {
                    b += BONUS_BOUNDARY;
                } else if raw[idx].is_ascii_uppercase() && raw[idx - 1].is_ascii_lowercase() {
                    b += BONUS_CAMEL;
                }
                if idx >= fbase {
                    b += BONUS_FILENAME;
                }
                let consec = if pd[hi] > i32::MIN / 4 { BONUS_CONSEC } else { 0 };
                bm = ps[hi].max(pd[hi]) + b + consec;
            }
            if bm >= gap {
                cs[hi + 1] = bm;
                cd[hi + 1] = bm;
                in_gap = false;
            } else {
                cs[hi + 1] = gap;
                cd[hi + 1] = i32::MIN / 2;
                in_gap = true;
            }
        }
        std::mem::swap(ps, cs);
        std::mem::swap(pd, cd);
    }
    let best = ps[1..].iter().copied().max().unwrap_or(i32::MIN);
    best - (plen as i32 / 16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vault-search-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scope_switches_on_slash() {
        assert_eq!(scope_for_query("router"), Scope::FilenameOnly);
        assert_eq!(scope_for_query("src/router"), Scope::FullPath);
    }

    #[test]
    fn finds_filename_match_ranked_above_noise() {
        let dir = temp_dir("rank");
        fs::create_dir_all(dir.join("rust/tests/ui/lint")).unwrap();
        fs::write(dir.join("rust/tests/ui/lint/outer-forbid.rs"), "").unwrap();
        fs::write(dir.join("router.js"), "").unwrap();
        fs::write(dir.join("unrelated.txt"), "").unwrap();

        let table = PathTable::build(&dir);
        assert_eq!(table.len(), 3);

        let results = table.search("router", 3);
        assert!(!results.is_empty(), "expected at least one match for 'router'");
        let (_, top_idx) = results[0];
        assert_eq!(
            table.path(top_idx as usize),
            "router.js",
            "router.js should rank first, not the scattered match in outer-forbid.rs"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn filename_scope_ignores_directory_names() {
        let dir = temp_dir("scope");
        fs::create_dir_all(dir.join("router")).unwrap();
        fs::write(dir.join("router/unrelated.txt"), "").unwrap();
        fs::write(dir.join("other.txt"), "").unwrap();

        let table = PathTable::build(&dir);
        let filename_results = table.search_with("router", 10, Scope::FilenameOnly, 1);
        assert!(
            filename_results.is_empty(),
            "filename-only scope must not match 'router' via the directory name"
        );

        let path_results = table.search_with("router", 10, Scope::FullPath, 1);
        assert!(!path_results.is_empty(), "full-path scope should match via the directory name");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resident_bytes_is_reasonable() {
        let dir = temp_dir("mem");
        fs::write(dir.join("a.txt"), "").unwrap();
        let table = PathTable::build(&dir);
        assert!(table.resident_bytes() > 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn incremental_add_assigns_stable_growing_ids() {
        let mut table = PathTable { buf: Vec::new(), lower: Vec::new(), offs: vec![0], base: Vec::new(), deleted: Vec::new(), path_index: HashMap::new() };
        let a = table.add("a.txt");
        let b = table.add("b.txt");
        assert_eq!((a, b), (0, 1));
        assert_eq!(table.path_to_id("a.txt"), Some(0));
        // re-adding an existing path is idempotent, not a duplicate row.
        assert_eq!(table.add("a.txt"), 0);
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn remove_tombstones_without_shifting_other_ids() {
        let mut table = PathTable { buf: Vec::new(), lower: Vec::new(), offs: vec![0], base: Vec::new(), deleted: Vec::new(), path_index: HashMap::new() };
        table.add("a.txt");
        let b = table.add("router.txt");
        table.add("c.txt");

        assert_eq!(table.remove("a.txt"), Some(0));
        assert_eq!(table.path_to_id("a.txt"), None, "removed path must not resolve anymore");
        assert_eq!(table.path_to_id("router.txt"), Some(b), "surviving doc_ids must not shift");

        let hits = table.search("router", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1, b);

        assert!(table.remove("a.txt").is_none(), "removing twice is a no-op, not a panic");
    }
}
