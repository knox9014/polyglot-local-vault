use std::sync::Mutex;
use tauri::State;

struct Dataset(Mutex<Vec<String>>);

/// ~100K synthetic file paths, matching the scale of the project's existing
/// 100K-file benchmarks (17_MEASUREMENT_BASIS.md). Content doesn't matter —
/// this measures IPC + webview round-trip cost, not search-algorithm quality.
fn synthetic_dataset(n: usize) -> Vec<String> {
    let dirs = ["src", "tests", "docs", "examples", "vendor", "lib", "scripts"];
    let words = [
        "router", "config", "handler", "utils", "model", "view", "controller", "service",
        "parser", "index", "auth", "session", "cache", "queue", "worker", "client",
    ];
    (0..n)
        .map(|i| {
            let d = dirs[i % dirs.len()];
            let w = words[(i / dirs.len()) % words.len()];
            format!("{d}/module_{}/{w}_{i}.rs", i / 137)
        })
        .collect()
}

#[tauri::command]
fn dataset_size(state: State<Dataset>) -> usize {
    state.0.lock().unwrap().len()
}

/// Filename-scope substring search (matches the project's confirmed decision:
/// "검색 기본 스코프 = 파일명"), capped at 50 results like a real result list.
#[tauri::command]
fn search(state: State<Dataset>, query: String) -> Vec<String> {
    let q = query.to_lowercase();
    state
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|path| {
            let name = path.rsplit('/').next().unwrap_or(path);
            name.to_lowercase().contains(&q)
        })
        .take(50)
        .cloned()
        .collect()
}

/// Writes the JS-collected latency stats to disk, then exits — the frontend
/// runs the whole benchmark unattended on load, no manual interaction needed.
#[tauri::command]
fn finish(json: String) {
    let run = std::env::var("BENCH_RUN").unwrap_or_else(|_| "1".to_string());
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../results_tauri_{run}.json"));
    std::fs::write(&out, json).expect("failed to write results");
    std::process::exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Dataset(Mutex::new(synthetic_dataset(100_000))))
        .invoke_handler(tauri::generate_handler![dataset_size, search, finish])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
