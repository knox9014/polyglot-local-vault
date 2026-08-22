//! `.vault/vault.toml` — vault-owned settings.
//! Schema is fixed by `docs/design/18_DATA_FORMATS.md` §7; this file must
//! read and write exactly that shape. It lives under `.vault/` (git-committed
//! source of truth), so unknown keys are preserved on write rather than
//! dropped — a newer version's settings must survive an older version
//! saving over them (same rule as §5's "미정의 rel 은 버리지 않고 원문 보존").

use std::path::Path;
use std::{fs, io};

use serde::{Deserialize, Serialize};

pub const DEFAULT_CONTENT_BYTES: u64 = 1_048_576; // 05 "성능 원칙": 1MB 초과 본문 인덱싱 제외
pub const DEFAULT_PARSE_BYTES: u64 = 5_242_880; // 06: 5MB 초과 파싱 제외

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VaultConfig {
    #[serde(default)]
    pub vault: VaultSection,
    #[serde(default)]
    pub ignore: IgnoreSection,
    #[serde(default)]
    pub limits: LimitsSection,
    #[serde(default)]
    pub mcp: McpSection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VaultSection {
    #[serde(default)]
    pub name: String,
    #[serde(default = "one")]
    pub schema_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IgnoreSection {
    #[serde(default = "yes")]
    pub use_gitignore: bool,
    #[serde(default = "default_ignore_patterns")]
    pub patterns: Vec<String>,
}

/// 06 "파싱 정책" and 16 "필수 — vendored 경로 제외" both require these as
/// *defaults*, not just as something the user may add: 16 measured that
/// including vendored trees is a precision failure (node: 764 candidates ->
/// 155 once excluded), because the extra ones point at unrelated third-party
/// code. Shipping this empty made that exact failure real — a scan of this
/// repo produced 2,654 links of which 80.9% were inside `node_modules/`.
fn default_ignore_patterns() -> Vec<String> {
    ["vendor/", "third_party/", "deps/", "node_modules/", "target/", "dist/", "build/", ".venv/", "__pycache__/"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LimitsSection {
    #[serde(default = "default_content_bytes")]
    pub content_bytes: u64,
    #[serde(default = "default_parse_bytes")]
    pub parse_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpSection {
    /// `approve` | `immediate`. Default is `approve` on purpose (18 §7):
    /// without a fixed default, convenience makes `immediate` the norm and
    /// AI mistakes get committed to git.
    #[serde(default = "approve")]
    pub write_mode: String,
}

fn one() -> u32 {
    1
}
fn yes() -> bool {
    true
}
fn approve() -> String {
    "approve".to_string()
}
fn default_content_bytes() -> u64 {
    DEFAULT_CONTENT_BYTES
}
fn default_parse_bytes() -> u64 {
    DEFAULT_PARSE_BYTES
}

impl Default for VaultSection {
    fn default() -> Self {
        VaultSection { name: String::new(), schema_version: 1 }
    }
}
impl Default for IgnoreSection {
    fn default() -> Self {
        IgnoreSection { use_gitignore: true, patterns: default_ignore_patterns() }
    }
}
impl Default for LimitsSection {
    fn default() -> Self {
        LimitsSection { content_bytes: DEFAULT_CONTENT_BYTES, parse_bytes: DEFAULT_PARSE_BYTES }
    }
}
impl Default for McpSection {
    fn default() -> Self {
        McpSection { write_mode: approve() }
    }
}

fn config_path(root: &Path) -> std::path::PathBuf {
    root.join(".vault").join("vault.toml")
}

/// Reads `.vault/vault.toml`. Missing file → defaults (a vault that was never
/// configured is valid). Malformed file → error, not silent defaults: silently
/// discarding a user's committed settings would be worse than refusing.
pub fn read(root: &Path) -> Result<VaultConfig, String> {
    let path = config_path(root);
    match fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(VaultConfig::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn write(root: &Path, config: &VaultConfig) -> Result<(), String> {
    let path = config_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = toml::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vault-config-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_yields_documented_defaults() {
        let dir = temp_dir("missing");
        let config = read(&dir).unwrap();
        assert!(config.ignore.use_gitignore);
        assert_eq!(config.limits.content_bytes, DEFAULT_CONTENT_BYTES);
        assert_eq!(config.limits.parse_bytes, DEFAULT_PARSE_BYTES);
        assert_eq!(config.mcp.write_mode, "approve", "18 §7 fixes this default; don't let it drift");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parses_the_exact_schema_from_the_design_doc() {
        let dir = temp_dir("schema");
        fs::create_dir_all(dir.join(".vault")).unwrap();
        fs::write(
            dir.join(".vault/vault.toml"),
            r#"
[vault]
name = "MyProject"
schema_version = 1

[ignore]
use_gitignore = true
patterns = ["vendor/", "third_party/", "deps/", "node_modules/"]

[limits]
content_bytes = 1048576
parse_bytes   = 5242880

[mcp]
write_mode = "approve"
"#,
        )
        .unwrap();

        let config = read(&dir).unwrap();
        assert_eq!(config.vault.name, "MyProject");
        assert_eq!(config.ignore.patterns, vec!["vendor/", "third_party/", "deps/", "node_modules/"]);
        assert_eq!(config.limits.content_bytes, 1_048_576);
        assert_eq!(config.mcp.write_mode, "approve");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn round_trips_through_write_and_read() {
        let dir = temp_dir("roundtrip");
        let mut config = VaultConfig::default();
        config.vault.name = "Roundtrip".into();
        config.ignore.patterns = vec!["build/".into()];
        config.limits.content_bytes = 512_000;

        write(&dir, &config).unwrap();
        assert_eq!(read(&dir).unwrap(), config);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn malformed_file_errors_instead_of_silently_defaulting() {
        let dir = temp_dir("malformed");
        fs::create_dir_all(dir.join(".vault")).unwrap();
        fs::write(dir.join(".vault/vault.toml"), "this is not = = valid toml [[[").unwrap();
        assert!(read(&dir).is_err(), "a broken config must surface, not vanish behind defaults");
        fs::remove_dir_all(&dir).unwrap();
    }
}
