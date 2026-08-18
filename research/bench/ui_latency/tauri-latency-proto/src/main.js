const { invoke } = window.__TAURI__.core;

function percentile(sorted, p) {
  const idx = Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length));
  return sorted[idx];
}

// Simulate a user typing several realistic queries, keystroke by keystroke.
// This is the thing we actually care about: cost of ONE keystroke -> updated
// result list, going through the real webview IPC bridge each time.
const TYPED_QUERIES = ["router", "config_12345", "index_99999", "vendor_worker_500", "auth"];

function renderResults(listEl, results) {
  // Matches the Qt side's QListWidget.clear()+addItems(): actually update the
  // visible DOM, not just receive the data. Rebuild via textContent (no
  // innerHTML/XSS risk) — same shape of work a real result list would do.
  listEl.textContent = "";
  const frag = document.createDocumentFragment();
  for (const r of results) {
    const li = document.createElement("li");
    li.textContent = r;
    frag.appendChild(li);
  }
  listEl.appendChild(frag);
}

async function runBenchmark() {
  const statusEl = document.querySelector("#status");
  const inputEl = document.querySelector("#query-input");
  const listEl = document.querySelector("#results");
  const size = await invoke("dataset_size");
  statusEl.textContent = `dataset: ${size} files. warming up...`;

  // Warm-up (JIT/allocator warmup, not measured).
  for (let i = 0; i < 50; i++) {
    const results = await invoke("search", { query: "warmup" + i });
    inputEl.value = "warmup" + i;
    renderResults(listEl, results);
  }

  const samples = [];
  const rounds = 30; // 30 rounds x ~40 keystrokes total per round = ~1200 measured calls
  for (let r = 0; r < rounds; r++) {
    for (const full of TYPED_QUERIES) {
      let partial = "";
      for (const ch of full) {
        partial += ch;
        const t0 = performance.now();
        const results = await invoke("search", { query: partial });
        inputEl.value = partial;
        renderResults(listEl, results); // include DOM update in the timed span, like the Qt side
        samples.push(performance.now() - t0);
      }
    }
    statusEl.textContent = `round ${r + 1}/${rounds}, ${samples.length} samples so far...`;
  }

  const sorted = [...samples].sort((a, b) => a - b);
  const stats = {
    framework: "tauri (webview2 + rust ipc)",
    dataset_size: size,
    samples: samples.length,
    p50_ms: percentile(sorted, 50),
    p95_ms: percentile(sorted, 95),
    p99_ms: percentile(sorted, 99),
    max_ms: sorted[sorted.length - 1],
    min_ms: sorted[0],
  };

  statusEl.textContent = `done. p50=${stats.p50_ms.toFixed(2)}ms p95=${stats.p95_ms.toFixed(2)}ms p99=${stats.p99_ms.toFixed(2)}ms max=${stats.max_ms.toFixed(2)}ms`;
  await invoke("finish", { json: JSON.stringify(stats, null, 2) });
}

window.addEventListener("DOMContentLoaded", () => {
  runBenchmark().catch((e) => {
    document.querySelector("#status").textContent = "ERROR: " + e;
  });
});
