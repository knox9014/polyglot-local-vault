//! R1 — doc inline-code token ↔ symbol (`16_SUGGESTION_ENGINE.md` "R1").
//! The general-purpose fallback rule: a backtick token in prose that
//! uniquely names one code symbol in the vault becomes a `describes`
//! candidate, file-level on both ends (`16`: "M2d는 (문서 파일, 토큰, 대상
//! 파일) 삼중항을 셌다" — the token only *locates* the target file, the
//! candidate isn't the symbol's own address).
//!
//! Noise control is 16's two rungs: a compound identifier (`_`/internal
//! uppercase/digit) always passes; a plain single word only passes if it's
//! *not* in the English dictionary (`assets/english_words.txt`, 234,448
//! words — the same Webster's Second `web2` set `research/bench`'s
//! measurement scripts already use, extracted locally from the installed
//! `english_words` Python package so the numbers stay comparable to `16`'s
//! measured candidate counts instead of drifting from a different list).
//!
//! **Known limitation, not a bug**: a real symbol name that happens to also
//! be an ordinary English word (`Cache`, `Ridge`, `Signer` — `16`'s own
//! examples) still gets dropped. There's no way to tell "the class is
//! named after the word" from "the word is just prose" without more
//! context than a bare token has; `16` accepts this as unavoidable.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::symbol_index::SymbolIndex;

static ENGLISH_WORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();

fn english_words() -> &'static HashSet<&'static str> {
    ENGLISH_WORDS.get_or_init(|| include_str!("../assets/english_words.txt").lines().collect())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub from: String, // vault://doc/file.md — file-level, no fragment (16 "R1")
    pub to: String,   // vault://code/file.py — file-level, no fragment
    pub rel: &'static str,
    pub origin: &'static str,
    pub confidence: &'static str,
    pub token: String, // the backtick token that triggered this, for the approval UI
    /// Times `token` appears in backticks in the source doc — one of 16's
    /// ranking signals ("백틱 등장 횟수 같은 문서에서 반복 언급될수록 그
    /// 문서의 주제일 개연성").
    pub mention_count: usize,
}

/// Code-symbol node types R1 matches against — headings/columns/JSON-YAML-
/// TOML keys aren't code identifiers and would just add noise (a heading
/// literally titled "Config" matching a `Config` class helps no one).
fn is_code_symbol_kind(node_type: &str) -> bool {
    matches!(node_type, "function" | "method" | "class" | "struct" | "enum" | "trait" | "interface")
}

/// `_underscored`, internal (non-leading) uppercase, or a digit — 16's
/// "복합 식별자" rung. A single leading-capital word (`Router`) does *not*
/// count as compound on its own — it falls through to the dictionary check.
fn is_compound_identifier(token: &str) -> bool {
    token.contains('_')
        || token.chars().any(|c| c.is_ascii_digit())
        || token.chars().skip(1).any(|c| c.is_uppercase())
}

/// 16's full noise-control rule: compound identifiers always pass; a plain
/// single word passes only if it's *not* an ordinary English word.
fn passes_noise_filter(token: &str) -> bool {
    is_compound_identifier(token) || !english_words().contains(token.to_lowercase().as_str())
}

/// Backtick-delimited inline code tokens (`` `Foo` ``), skipping fenced
/// code blocks — 16: "코드 펜스 안은 제외한다. 예제 코드의 토큰은 그 문서가
/// 설명하는 대상이 아니다." Markdown-only for this pass; `.rst`/`.txt` use
/// different inline-code delimiters (double backtick) and code-block
/// syntax (`::`/`.. code-block::`), which is different enough to want its
/// own extractor rather than a "close enough" shared one.
fn extract_inline_code_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        let stripped = line.trim_start();
        if stripped.starts_with("```") || stripped.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let mut chars = line.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c != '`' {
                continue;
            }
            if let Some(end) = line[i + 1..].find('`') {
                let token = &line[i + 1..i + 1 + end];
                if !token.is_empty() && !token.contains(char::is_whitespace) {
                    tokens.push(token.to_string());
                }
                // Skip past the closing backtick so a run like
                // `` `a` `b` `` doesn't have its second opener missed.
                while let Some(&(j, _)) = chars.peek() {
                    if j > i + 1 + end {
                        break;
                    }
                    chars.next();
                }
            }
        }
    }
    tokens
}

/// Generates R1 candidates for every `.md` file in `doc_paths` against the
/// code symbols already in `symbols`, ranked per 16 "후보 우선순위": compound
/// identifiers first (lower false-positive risk than a dictionary-cleared
/// single word), then longer tokens (16: "짧을수록 흔한 단어일 확률이
/// 높음"), then how often the token is repeated in that doc (16: repeated
/// mentions signal it's the doc's actual topic). Docstring presence and
/// filename/symbol overlap are 16's other two signals — skipped here, they
/// need docstring extraction and a path-token-overlap heuristic neither of
/// which exist yet; add them as tie-breakers once they do.
pub fn generate(root: &Path, doc_paths: impl IntoIterator<Item = impl AsRef<str>>, symbols: &SymbolIndex) -> Vec<Candidate> {
    let mut name_to_files: HashMap<&str, HashSet<&str>> = HashMap::new();
    for entry in symbols.entries() {
        if !is_code_symbol_kind(&entry.node_type) {
            continue;
        }
        let leaf = entry.id.rsplit('.').next().unwrap_or(&entry.id);
        name_to_files.entry(leaf).or_default().insert(&entry.path);
    }

    let mut candidates = Vec::new();
    for doc_path in doc_paths {
        let doc_path = doc_path.as_ref();
        if !doc_path.ends_with(".md") {
            continue;
        }
        let Ok(text) = fs::read_to_string(root.join(doc_path)) else { continue };
        let tokens = extract_inline_code_tokens(&text);

        let mut mention_count: HashMap<&str, usize> = HashMap::new();
        for t in &tokens {
            *mention_count.entry(t.as_str()).or_default() += 1;
        }

        // One candidate per (doc, target file) pair — if several matched
        // tokens in this doc point at the same file, keep the strongest one
        // (most mentions) rather than presenting duplicates for one file.
        let mut best_per_file: HashMap<&str, &str> = HashMap::new();
        for token in mention_count.keys() {
            if !passes_noise_filter(token) {
                continue;
            }
            let Some(files) = name_to_files.get(token) else { continue };
            if files.len() != 1 {
                continue; // ambiguous across the vault — R1 only fires on a unique name match
            }
            let target_file = *files.iter().next().unwrap();
            if target_file == doc_path {
                continue; // a file documenting its own symbol isn't a cross-reference
            }
            best_per_file
                .entry(target_file)
                .and_modify(|best| {
                    if mention_count[token] > mention_count[*best] {
                        *best = token;
                    }
                })
                .or_insert(token);
        }

        for (target_file, token) in best_per_file {
            candidates.push(Candidate {
                from: format!("vault://{doc_path}"),
                to: format!("vault://{target_file}"),
                rel: "describes",
                origin: "suggested", // 18 §4.6: unapproved suggestion-engine candidate, not `extracted` (that's R6/R2's confirmed-reference origin)
                confidence: "probable",
                mention_count: mention_count[token],
                token: token.to_string(),
            });
        }
    }

    candidates.sort_by(|a, b| {
        is_compound_identifier(&b.token)
            .cmp(&is_compound_identifier(&a.token))
            .then(b.token.len().cmp(&a.token.len()))
            .then(b.mention_count.cmp(&a.mention_count))
    });
    candidates
}

// ---- storage: .vault-ai/suggestions/r1.jsonl + .vault/decisions.jsonl (18 §4.3, §4.6) ----

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub key: String,
    pub verdict: String, // "accept" | "reject"
    pub rule: String,
    pub from: String,
    pub to: String,
    pub ts: String,
}

/// 18 §4.3: `sha256(rule + "\n" + from + "\n" + to)`, first 16 hex chars.
/// Content-addressed, not candidate-ID-addressed, so a rejected candidate
/// stays rejected across re-scans (the whole point of `decisions.jsonl`) —
/// only a changed `from`/`to` address produces a new key and gets asked
/// about again, which 18 calls out as intended: a different target deserves
/// a fresh judgment.
pub fn decision_key(rule: &str, from: &str, to: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(rule.as_bytes());
    hasher.update(b"\n");
    hasher.update(from.as_bytes());
    hasher.update(b"\n");
    hasher.update(to.as_bytes());
    hasher.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Drops any candidate that already has a recorded verdict (accept or
/// reject) — 16: "거절한 후보를 다시 제안하지 않기 위한 것이다." An accepted
/// candidate is also filtered here since accepting turns it into a real
/// link (not this module's job to write); leaving it in the suggestion list
/// would just be a stale duplicate of something already resolved.
pub fn filter_undecided(candidates: Vec<Candidate>, decisions: &[Decision]) -> Vec<Candidate> {
    let decided: HashSet<&str> = decisions.iter().map(|d| d.key.as_str()).collect();
    candidates.into_iter().filter(|c| !decided.contains(decision_key("R1", &c.from, &c.to).as_str())).collect()
}

/// Overwrites `.vault-ai/suggestions/r1.jsonl` with the current candidate
/// set — this file is a pure derivative (18 §4.6: `suggested` origin lives
/// under `.vault-ai/`), regenerated whole on every scan rather than
/// diffed/appended like `decisions.jsonl`.
pub fn write_suggestions(path: &Path, candidates: &[Candidate]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut out = String::from(r#"{"_type":"suggestions","_v":1,"rule":"R1"}"#);
    out.push('\n');
    for c in candidates {
        out.push_str(&serde_json::to_string(c)?);
        out.push('\n');
    }
    fs::write(path, out)
}

/// Reads `decisions.jsonl`, skipping the `_type`/`_v` header line. A
/// missing file just means no decisions yet — not an error.
pub fn read_decisions(path: &Path) -> io::Result<Vec<Decision>> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    text.lines()
        .filter(|line| !line.contains("\"_type\""))
        .map(|line| serde_json::from_str(line).map_err(io::Error::other))
        .collect()
}

/// Appends one verdict to `decisions.jsonl` — a running log, not a
/// regenerated derivative (18 §4.3 lives under `.vault/`, source of truth).
/// Writes the header line first if the file doesn't exist yet.
pub fn append_decision(path: &Path, decision: &Decision) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let is_new = !path.exists();
    let mut file = fs::OpenOptions::new().create(true).append(true).open(path)?;
    if is_new {
        writeln!(file, r#"{{"_type":"decisions","_v":1}}"#)?;
    }
    writeln!(file, "{}", serde_json::to_string(decision)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::links::now_rfc3339;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vault-suggest-r1-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn matches_a_unique_compound_symbol_name_to_its_file() {
        let dir = temp_dir("unique");
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/router.py"), "class TeacherRouter:\n    pass\n").unwrap();
        fs::write(dir.join("docs.md"), "See `TeacherRouter` for details.\n").unwrap();

        let symbols = SymbolIndex::build(&dir, ["src/router.py", "docs.md"]);
        let candidates = generate(&dir, ["src/router.py", "docs.md"], &symbols);

        assert_eq!(
            candidates,
            vec![Candidate {
                from: "vault://docs.md".into(),
                to: "vault://src/router.py".into(),
                rel: "describes",
                origin: "suggested",
                confidence: "probable",
                token: "TeacherRouter".into(),
                mention_count: 1,
            }]
        );
    }

    #[test]
    fn ambiguous_name_across_two_files_produces_no_candidate() {
        let dir = temp_dir("ambiguous");
        fs::write(dir.join("a.py"), "def process_data():\n    pass\n").unwrap();
        fs::write(dir.join("b.py"), "def process_data():\n    pass\n").unwrap();
        fs::write(dir.join("docs.md"), "Calls `process_data` internally.\n").unwrap();

        let symbols = SymbolIndex::build(&dir, ["a.py", "b.py", "docs.md"]);
        let candidates = generate(&dir, ["a.py", "b.py", "docs.md"], &symbols);
        assert!(candidates.is_empty());
    }

    #[test]
    fn token_inside_a_fenced_code_block_is_ignored() {
        let dir = temp_dir("fenced");
        fs::write(dir.join("src.py"), "class TeacherRouter:\n    pass\n").unwrap();
        fs::write(dir.join("docs.md"), "```python\nTeacherRouter()\n```\n").unwrap();

        let symbols = SymbolIndex::build(&dir, ["src.py", "docs.md"]);
        let candidates = generate(&dir, ["src.py", "docs.md"], &symbols);
        assert!(candidates.is_empty(), "a token only inside a code fence must not count: {candidates:?}");
    }

    #[test]
    fn plain_single_word_not_in_the_dictionary_still_passes() {
        // "Config" has no underscore/digit/internal-uppercase, so it only
        // gets through via the dictionary rung — it isn't an English word.
        let dir = temp_dir("single-word-pass");
        fs::write(dir.join("config.py"), "class Config:\n    pass\n").unwrap();
        fs::write(dir.join("docs.md"), "See `Config` for details.\n").unwrap();

        let symbols = SymbolIndex::build(&dir, ["config.py", "docs.md"]);
        let candidates = generate(&dir, ["config.py", "docs.md"], &symbols);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].to, "vault://config.py");
    }

    #[test]
    fn plain_single_word_that_is_an_english_word_is_dropped() {
        // "Cache" is a real word in the dictionary — 16's documented,
        // accepted limitation: a symbol named after an ordinary word is
        // indistinguishable from the word itself without more context.
        let dir = temp_dir("single-word-drop");
        fs::write(dir.join("cache.py"), "class Cache:\n    pass\n").unwrap();
        fs::write(dir.join("docs.md"), "Uses a `Cache` internally.\n").unwrap();

        let symbols = SymbolIndex::build(&dir, ["cache.py", "docs.md"]);
        let candidates = generate(&dir, ["cache.py", "docs.md"], &symbols);
        assert!(candidates.is_empty());
    }

    #[test]
    fn non_code_symbol_kinds_are_not_matched() {
        // A heading or JSON key sharing a name with a class isn't a code
        // reference — matching it would misfile the candidate's target.
        let dir = temp_dir("non-code-kind");
        fs::write(dir.join("config.json"), r#"{"Router": true}"#).unwrap();
        fs::write(dir.join("docs.md"), "See `Router_Config` for details.\n").unwrap();
        fs::write(dir.join("router.py"), "def Router_Config():\n    pass\n").unwrap();

        let symbols = SymbolIndex::build(&dir, ["config.json", "docs.md", "router.py"]);
        let candidates = generate(&dir, ["config.json", "docs.md", "router.py"], &symbols);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].to, "vault://router.py");
    }

    #[test]
    fn ranks_more_repeated_mentions_above_a_single_mention() {
        // Equal-length tokens, both compound — isolates the mention-count
        // signal (the third sort key) from token length (the second).
        let dir = temp_dir("ranking");
        fs::write(dir.join("a.py"), "def handle_data():\n    pass\n").unwrap();
        fs::write(dir.join("b.py"), "def format_data():\n    pass\n").unwrap();
        fs::write(
            dir.join("docs.md"),
            "Calls `format_data` once. `handle_data` is used here, and again: `handle_data`.\n",
        )
        .unwrap();

        let symbols = SymbolIndex::build(&dir, ["a.py", "b.py", "docs.md"]);
        let candidates = generate(&dir, ["a.py", "b.py", "docs.md"], &symbols);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].token, "handle_data", "2 mentions must rank above 1: {candidates:?}");
        assert_eq!(candidates[0].mention_count, 2);
        assert_eq!(candidates[1].token, "format_data");
        assert_eq!(candidates[1].mention_count, 1);
    }

    #[test]
    fn decision_key_is_stable_for_the_same_triple_and_changes_when_the_target_does() {
        let k1 = decision_key("R1", "vault://docs.md", "vault://src/router.py");
        let k2 = decision_key("R1", "vault://docs.md", "vault://src/router.py");
        assert_eq!(k1, k2);
        assert_eq!(k1.len(), 16);

        let k3 = decision_key("R1", "vault://docs.md", "vault://src/other.py");
        assert_ne!(k1, k3, "a different target must get a different key — 18: 대상이 달라졌으면 판단도 다시 받아야 한다");
    }

    #[test]
    fn a_rejected_candidate_does_not_reappear() {
        let candidate = Candidate {
            from: "vault://docs.md".into(),
            to: "vault://src/router.py".into(),
            rel: "describes",
            origin: "suggested",
            confidence: "probable",
            token: "TeacherRouter".into(),
            mention_count: 1,
        };
        let decisions = vec![Decision {
            key: decision_key("R1", &candidate.from, &candidate.to),
            verdict: "reject".into(),
            rule: "R1".into(),
            from: candidate.from.clone(),
            to: candidate.to.clone(),
            ts: "2026-08-19T00:00:00Z".into(),
        }];

        let filtered = filter_undecided(vec![candidate], &decisions);
        assert!(filtered.is_empty());
    }

    #[test]
    fn suggestions_and_decisions_round_trip_through_disk() {
        let dir = temp_dir("storage");
        let suggestions_path = dir.join("suggestions.jsonl");
        let decisions_path = dir.join("decisions.jsonl");

        let candidate = Candidate {
            from: "vault://docs.md".into(),
            to: "vault://src/router.py".into(),
            rel: "describes",
            origin: "suggested",
            confidence: "probable",
            token: "TeacherRouter".into(),
            mention_count: 1,
        };
        write_suggestions(&suggestions_path, &[candidate.clone()]).unwrap();
        let text = fs::read_to_string(&suggestions_path).unwrap();
        assert!(text.starts_with(r#"{"_type":"suggestions","_v":1,"rule":"R1"}"#));
        assert!(text.contains("TeacherRouter"));

        assert!(read_decisions(&decisions_path).unwrap().is_empty(), "a missing file must read as no decisions, not an error");

        let decision = Decision {
            key: decision_key("R1", &candidate.from, &candidate.to),
            verdict: "accept".into(),
            rule: "R1".into(),
            from: candidate.from.clone(),
            to: candidate.to.clone(),
            ts: now_rfc3339(),
        };
        append_decision(&decisions_path, &decision).unwrap();
        let read_back = read_decisions(&decisions_path).unwrap();
        assert_eq!(read_back, vec![decision]);

        fs::remove_dir_all(&dir).unwrap();
    }
}
