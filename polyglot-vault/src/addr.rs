//! `vault://` address parsing and normalization, and heading slug generation.
//! Spec: `docs/design/18_DATA_FORMATS.md` §1~2.

use unicode_normalization::UnicodeNormalization;

const SCHEME: &str = "vault://";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub path: String,
    pub fragment: Option<Fragment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fragment {
    /// qualname parts, e.g. `Config.load` -> ["Config", "load"]
    Symbol(Vec<String>),
    Heading(String),
    /// RFC 6901 JSON Pointer, leading "/" included
    Pointer(String),
    Column(String),
    Row(String),
    Cell(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddrError {
    MissingScheme,
    EmptyPath,
    AbsolutePath,
    ParentTraversal,
    EmptyFragment,
}

impl std::fmt::Display for AddrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AddrError::MissingScheme => write!(f, "address must start with \"vault://\""),
            AddrError::EmptyPath => write!(f, "path must not be empty"),
            AddrError::AbsolutePath => write!(f, "path must not be absolute"),
            AddrError::ParentTraversal => write!(f, "path must not contain \"..\""),
            AddrError::EmptyFragment => write!(f, "fragment must not be empty"),
        }
    }
}

impl std::error::Error for AddrError {}

/// Parses a full `vault://path#fragment` address.
pub fn parse(s: &str) -> Result<Address, AddrError> {
    let rest = s.strip_prefix(SCHEME).ok_or(AddrError::MissingScheme)?;
    let (path_part, fragment_part) = match rest.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (rest, None),
    };
    let path = normalize_path(&decode_escapes(path_part))?;
    let fragment = fragment_part.map(parse_fragment).transpose()?;
    Ok(Address { path, fragment })
}

fn parse_fragment(raw: &str) -> Result<Fragment, AddrError> {
    if raw.is_empty() {
        return Err(AddrError::EmptyFragment);
    }
    if let Some(value) = raw.strip_prefix("h:") {
        return Ok(Fragment::Heading(decode_escapes(value)));
    }
    if let Some(value) = raw.strip_prefix("row:") {
        return Ok(Fragment::Row(decode_escapes(value)));
    }
    if let Some(value) = raw.strip_prefix("col:") {
        return Ok(Fragment::Column(decode_escapes(value)));
    }
    if let Some(value) = raw.strip_prefix("cell:") {
        return Ok(Fragment::Cell(decode_escapes(value)));
    }
    if raw.starts_with('/') {
        return Ok(Fragment::Pointer(decode_escapes(raw)));
    }
    // Symbol: split on literal "." (qualname separator) BEFORE decoding, so an
    // escaped "%2E" (a literal dot inside one name) never acts as a separator.
    let parts = raw.split('.').map(decode_escapes).collect();
    Ok(Fragment::Symbol(parts))
}

/// Normalizes a decoded path: backslash -> "/", NFC, rejects absolute / "..".
pub fn normalize_path(raw: &str) -> Result<String, AddrError> {
    if raw.is_empty() {
        return Err(AddrError::EmptyPath);
    }
    let unified = raw.replace('\\', "/");
    let is_drive_absolute = unified.as_bytes().get(1) == Some(&b':');
    if unified.starts_with('/') || is_drive_absolute {
        return Err(AddrError::AbsolutePath);
    }
    if unified.split('/').any(|seg| seg == "..") {
        return Err(AddrError::ParentTraversal);
    }
    Ok(unified.nfc().collect())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Decodes `%XX` percent-escapes. Non-escaped bytes pass through unchanged.
pub fn decode_escapes(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Escapes `#`, `%`, `/`, and control characters (U+0000-U+001F) — the only
/// characters the address format requires escaping (18 §1.5).
pub fn encode_segment(s: &str) -> String {
    encode(s, false)
}

/// Same as `encode_segment`, plus escapes literal "." as "%2E" so it can't be
/// mistaken for the qualname separator (18 §1.5).
pub fn encode_symbol_name(s: &str) -> String {
    encode(s, true)
}

fn encode(s: &str, escape_dot: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '#' | '%' | '/' => out.push_str(&format!("%{:02X}", ch as u32)),
            '.' if escape_dot => out.push_str("%2E"),
            c if (c as u32) < 0x20 => out.push_str(&format!("%{:02X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Generates a heading slug per the 7-step rule (18 §2.1).
pub fn slugify_heading(text: &str) -> String {
    let nfc: String = text.nfc().collect(); // 1. NFC
    let lower = nfc.to_lowercase(); // 2. locale-independent unicode lowercase
    let trimmed = lower.trim(); // 3. trim

    let mut with_dashes = String::with_capacity(trimmed.len());
    let mut in_space_run = false;
    for ch in trimmed.chars() {
        if ch.is_whitespace() {
            if !in_space_run {
                with_dashes.push('-');
            }
            in_space_run = true;
        } else {
            with_dashes.push(ch);
            in_space_run = false;
        }
    } // 4. whitespace runs -> "-"

    let filtered: String = with_dashes
        .chars()
        .filter(|c| c.is_alphabetic() || c.is_numeric() || *c == '-' || *c == '_')
        .collect(); // 5. keep Letter/Number/-/_ only

    let mut collapsed = String::with_capacity(filtered.len());
    let mut in_dash_run = false;
    for ch in filtered.chars() {
        if ch == '-' {
            if !in_dash_run {
                collapsed.push('-');
            }
            in_dash_run = true;
        } else {
            collapsed.push(ch);
            in_dash_run = false;
        }
    }
    let result = collapsed.trim_matches('-'); // 6. collapse "-" runs, trim edges

    if result.is_empty() {
        "section".to_string() // 7. empty -> "section"
    } else {
        result.to_string()
    }
}

/// Assigns "-2", "-3", ... suffixes to repeated slugs in order of appearance (18 §2.2).
pub fn dedupe_slugs(slugs: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashMap::<String, u32>::new();
    slugs
        .into_iter()
        .map(|slug| {
            let count = seen.entry(slug.clone()).or_insert(0);
            *count += 1;
            if *count == 1 {
                slug
            } else {
                format!("{slug}-{count}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_examples_from_spec() {
        assert_eq!(slugify_heading("Teacher Router"), "teacher-router");
        assert_eq!(slugify_heading("주소 체계"), "주소-체계");
        assert_eq!(slugify_heading("`vault://` 문법"), "vault-문법");
        assert_eq!(slugify_heading("3. 해석 계단!"), "3-해석-계단");
        assert_eq!(slugify_heading("---"), "section");
    }

    #[test]
    fn slug_dedup_appends_running_suffix() {
        let slugs = ["addr", "addr", "other", "addr"].map(String::from);
        assert_eq!(
            dedupe_slugs(slugs),
            vec!["addr", "addr-2", "other", "addr-3"]
        );
    }

    #[test]
    fn slug_nfc_normalizes_before_slugifying() {
        // "café" written with a combining acute accent (NFD): e + U+0301
        let nfd = "cafe\u{0301}";
        assert_eq!(slugify_heading(nfd), "café");
    }

    #[test]
    fn path_normalizes_backslashes_and_nfc() {
        let addr = parse("vault://src\\cafe\u{0301}.py").unwrap();
        assert_eq!(addr.path, "src/café.py");
    }

    #[test]
    fn path_rejects_absolute_and_parent_traversal() {
        assert_eq!(parse("vault:///etc/passwd").unwrap_err(), AddrError::AbsolutePath);
        assert_eq!(parse("vault://C:/secrets").unwrap_err(), AddrError::AbsolutePath);
        assert_eq!(
            parse("vault://src/../../etc/passwd").unwrap_err(),
            AddrError::ParentTraversal
        );
    }

    #[test]
    fn missing_scheme_rejected() {
        assert_eq!(parse("src/router.py").unwrap_err(), AddrError::MissingScheme);
    }

    #[test]
    fn symbol_fragment_no_prefix() {
        let addr = parse("vault://src/router.py#TeacherRouter.select").unwrap();
        assert_eq!(
            addr.fragment,
            Some(Fragment::Symbol(vec!["TeacherRouter".into(), "select".into()]))
        );
    }

    #[test]
    fn escaped_dot_keeps_symbol_name_whole() {
        // "Config.load" as one literal name vs Config's `load` member.
        let dotted_name = parse("vault://src/lib.rs#Config%2Eload").unwrap();
        assert_eq!(
            dotted_name.fragment,
            Some(Fragment::Symbol(vec!["Config.load".into()]))
        );

        let member = parse("vault://src/lib.rs#Config.load").unwrap();
        assert_eq!(
            member.fragment,
            Some(Fragment::Symbol(vec!["Config".into(), "load".into()]))
        );
    }

    #[test]
    fn typed_fragments() {
        assert_eq!(
            parse("vault://docs/architecture.md#h:teacher-router").unwrap().fragment,
            Some(Fragment::Heading("teacher-router".into()))
        );
        assert_eq!(
            parse("vault://config/model.json#/router/threshold").unwrap().fragment,
            Some(Fragment::Pointer("/router/threshold".into()))
        );
        assert_eq!(
            parse("vault://data/train.csv#col:label").unwrap().fragment,
            Some(Fragment::Column("label".into()))
        );
        assert_eq!(
            parse("vault://data/train.csv#row:1042").unwrap().fragment,
            Some(Fragment::Row("1042".into()))
        );
        assert_eq!(
            parse("vault://experiments/run.ipynb#cell:12").unwrap().fragment,
            Some(Fragment::Cell("12".into()))
        );
    }

    #[test]
    fn encode_escapes_required_chars_only() {
        assert_eq!(encode_segment("a#b%c/d\u{0001}e"), "a%23b%25c%2Fd%01e");
        assert_eq!(encode_segment("설계.md"), "설계.md"); // non-ASCII, "." untouched
        assert_eq!(encode_symbol_name("Config.load"), "Config%2Eload");
    }

    #[test]
    fn decode_reverses_encode() {
        let raw = "a#b%c/d\u{0001}e";
        assert_eq!(decode_escapes(&encode_segment(raw)), raw);
    }
}
