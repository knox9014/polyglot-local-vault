const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
import { forceSimulation, forceManyBody, forceLink, forceCenter, forceCollide, forceX, forceY } from "d3-force";
import { select } from "d3-selection";
import { drag } from "d3-drag";
import { zoom } from "d3-zoom";
import { marked } from "marked";
import { highlight, EXT_LANG } from "./highlight.js";
import { getSavedVaults, saveVault, vaultDisplayName } from "./vaults.js";
import { colorForExt, legendFor, FOLDER_COLOR } from "./lang-colors.js";

const app = document.getElementById("app");
const searchBarBtn = document.getElementById("search-bar-btn");

const vaultSwitcherBtn = document.getElementById("vault-switcher-btn");
const vaultNameEl = document.getElementById("vault-name");
const vaultMenu = document.getElementById("vault-menu");
const recentVaultsEl = document.getElementById("recent-vaults");
const openAnotherVaultBtn = document.getElementById("open-another-vault-btn");

const searchModal = document.getElementById("search-modal");
const searchInput = document.getElementById("search-input");
const filenameResultsEl = document.getElementById("filename-results");
const symbolResultsEl = document.getElementById("symbol-results");
const contentResultsEl = document.getElementById("content-results");

const viewerPane = document.getElementById("viewer-pane");
const viewerEmpty = document.getElementById("viewer-empty");
const viewerLoaded = document.getElementById("viewer-loaded");
const viewerPath = document.getElementById("viewer-path");
const viewerSaveStatus = document.getElementById("viewer-save-status");
const viewerEditToggle = document.getElementById("viewer-edit-toggle");
const viewerFrontmatter = document.getElementById("viewer-frontmatter");
const viewerRendered = document.getElementById("viewer-rendered");
const viewerContent = document.getElementById("viewer-content");
const viewerEditor = document.getElementById("viewer-editor");
const viewerEditorText = document.getElementById("viewer-editor-text");
const viewerEditorPreview = document.getElementById("viewer-editor-preview");
const viewerMedia = document.getElementById("viewer-media");
const viewerImage = document.getElementById("viewer-image");
const viewerAudio = document.getElementById("viewer-audio");
const graphPane = document.getElementById("graph-pane");
const graphToggle = document.getElementById("graph-toggle");

const fileTreeEl = document.getElementById("file-tree");
const tagListEl = document.getElementById("tag-list");

let allFiles = [];
let allTags = {};
let activeTag = null;
let currentVaultPath = null;
let currentOpenPath = null;
let currentFileContent = ""; // last content loaded from (or saved to) disk — what "discard changes" reverts to
let isEditing = false;
let isDirty = false;

// ---- app settings (user preferences; vault-independent, so localStorage.
// Vault-owned settings live in .vault/vault.toml instead — see 18 §7) ----

const APP_SETTINGS_KEY = "polyglot-vault:app-settings";
const DEFAULT_APP_SETTINGS = {
  accent: "#a3d977",
  fontSize: 15,
  monoFont: '"Cascadia Code", Consolas, monospace',
  resultLimit: 20,
};

function loadAppSettings() {
  try {
    return { ...DEFAULT_APP_SETTINGS, ...JSON.parse(localStorage.getItem(APP_SETTINGS_KEY) || "{}") };
  } catch {
    return { ...DEFAULT_APP_SETTINGS };
  }
}

let appSettings = loadAppSettings();

function applyAppSettings() {
  const root = document.documentElement;
  root.style.setProperty("--accent", appSettings.accent);
  root.style.setProperty("--file-color", appSettings.accent);
  document.body.style.fontSize = `${appSettings.fontSize}px`;
  viewerContent.style.fontFamily = appSettings.monoFont;
  localStorage.setItem(APP_SETTINGS_KEY, JSON.stringify(appSettings));
}

// ---- vault switcher ----

function renderVaultMenu() {
  recentVaultsEl.textContent = "";
  for (const path of getSavedVaults()) {
    const btn = document.createElement("button");
    btn.className = "vault-menu-item";
    const check = document.createElement("span");
    check.className = "check";
    check.textContent = path === currentVaultPath ? "✓" : "";
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = vaultDisplayName(path);
    btn.append(check, name);
    btn.title = path;
    btn.addEventListener("click", async () => {
      vaultMenu.classList.add("hidden");
      if (path !== currentVaultPath && confirmDiscardIfDirty()) await invoke("open_vault_window", { path });
    });
    recentVaultsEl.appendChild(btn);
  }
}

vaultSwitcherBtn.addEventListener("click", () => {
  renderVaultMenu();
  vaultMenu.classList.toggle("hidden");
});
document.addEventListener("click", (e) => {
  if (!vaultMenu.contains(e.target) && !vaultSwitcherBtn.contains(e.target)) {
    vaultMenu.classList.add("hidden");
  }
});
openAnotherVaultBtn.addEventListener("click", async () => {
  vaultMenu.classList.add("hidden");
  if (confirmDiscardIfDirty()) await invoke("show_launcher");
});

// Switching vaults swaps the backend's LiveIndex out from under this window
// (see open_vault_window in lib.rs) — an unsaved edit would just vanish, so
// this is the same discard-confirmation openFileInViewer already does.
function confirmDiscardIfDirty() {
  if (!isEditing || !isDirty) return true;
  return confirm("저장하지 않은 변경사항이 있습니다. 저장하지 않고 이동할까요?");
}

// ---- startup: the vault is already open (Rust loaded it before this window) ----

async function init() {
  applyAppSettings();
  const stats = await invoke("vault_stats");
  currentVaultPath = stats.path;
  saveVault(stats.path); // a vault that actually opened is one worth remembering
  vaultNameEl.textContent = `${vaultDisplayName(stats.path)} (${stats.file_count})`;
  vaultNameEl.title = stats.path;
  await loadSidebar();
  await refreshSuggestionsBadge();
}

async function loadSidebar() {
  allFiles = await invoke("list_files");
  allTags = await invoke("list_tags");
  renderTree(allFiles);
  renderTags();
}

// ---- sidebar: file tree + tags ----

function buildTree(paths) {
  const root = { children: new Map() };
  for (const path of paths) {
    const parts = path.split("/");
    let node = root;
    let acc = "";
    parts.forEach((part, i) => {
      acc = acc ? `${acc}/${part}` : part;
      const isFile = i === parts.length - 1;
      if (!node.children.has(part)) {
        node.children.set(part, { name: part, path: acc, isFile, children: new Map() });
      }
      node = node.children.get(part);
    });
  }
  return root;
}

function renderTreeNode(node, container, collapse) {
  const entries = [...node.children.values()].sort((a, b) => {
    if (a.isFile !== b.isFile) return a.isFile ? 1 : -1; // folders first
    return a.name.localeCompare(b.name);
  });
  for (const child of entries) {
    const row = document.createElement("div");
    row.className = `tree-row ${child.isFile ? "file" : "folder"}`;
    const icon = document.createElement("span");
    icon.className = "tree-icon";
    icon.textContent = child.isFile ? "📄" : "📁";
    const label = document.createElement("span");
    label.textContent = child.name;
    row.append(icon, label);

    const wrapper = document.createElement("div");
    wrapper.className = "tree-node";
    wrapper.appendChild(row);

    if (child.isFile) {
      row.addEventListener("click", () => openFileInViewer(child.path));
    } else {
      const childrenEl = document.createElement("div");
      childrenEl.className = `tree-children${collapse ? " collapsed" : ""}`;
      renderTreeNode(child, childrenEl, collapse);
      wrapper.appendChild(childrenEl);
      row.addEventListener("click", () => childrenEl.classList.toggle("collapsed"));
    }
    container.appendChild(wrapper);
  }
}

// Folders start collapsed for the full tree (too many to show open at once),
// but a tag filter already narrows to a handful of files — collapsed by
// default there just hides them behind their parent folder and looks like
// the filter did nothing, so a filtered render starts fully expanded.
function renderTree(paths, { collapse = true } = {}) {
  fileTreeEl.textContent = "";
  renderTreeNode(buildTree(paths), fileTreeEl, collapse);
}

function renderTags() {
  tagListEl.textContent = "";
  for (const tag of Object.keys(allTags).sort()) {
    const li = document.createElement("li");
    li.textContent = `#${tag} (${allTags[tag].length})`;
    li.classList.toggle("active", tag === activeTag);
    li.addEventListener("click", () => toggleTagFilter(tag));
    tagListEl.appendChild(li);
  }
}

function toggleTagFilter(tag) {
  activeTag = activeTag === tag ? null : tag;
  renderTree(activeTag ? allTags[activeTag] || [] : allFiles, { collapse: !activeTag });
  renderTags();
}

// ---- search modal ----

let activeIndex = 0;

function flatResults() {
  return [
    ...[...filenameResultsEl.children].map((li) => ({ el: li, path: li.dataset.path })),
    ...[...symbolResultsEl.children].map((li) => ({ el: li, path: li.dataset.path })),
    ...[...contentResultsEl.children].map((li) => ({ el: li, path: li.dataset.path })),
  ];
}

function renderResultList(el, hits) {
  el.textContent = "";
  for (const hit of hits) {
    const li = document.createElement("li");
    li.textContent = hit.path;
    li.dataset.path = hit.path;
    li.addEventListener("click", () => selectResult(hit.path));
    el.appendChild(li);
  }
}

// Symbols carry more than a path — `Router.select` inside `src/router.py` —
// so this shows the qualname/heading/column name first with its file as a
// second line, instead of reusing renderResultList's bare-path row.
function renderSymbolResults(el, hits) {
  el.textContent = "";
  for (const hit of hits) {
    const li = document.createElement("li");
    li.dataset.path = hit.path;
    li.title = hit.address;

    const name = document.createElement("div");
    name.className = "symbol-result-name";
    name.textContent = `${hit.node_type}: ${hit.name}`;
    const path = document.createElement("div");
    path.className = "symbol-result-path";
    path.textContent = hit.path;

    li.append(name, path);
    li.addEventListener("click", () => selectResult(hit.path));
    el.appendChild(li);
  }
}

function highlightActive() {
  const items = flatResults();
  items.forEach((item, i) => item.el.classList.toggle("active", i === activeIndex));
  items[activeIndex]?.el.scrollIntoView({ block: "nearest" });
}

async function runSearch(query) {
  if (!query) {
    filenameResultsEl.textContent = "";
    symbolResultsEl.textContent = "";
    contentResultsEl.textContent = "";
    return;
  }
  // The limit goes to the backend so raising it actually returns more results;
  // slicing only on this side capped everything at the backend's default.
  const results = await invoke("search", { query, limit: appSettings.resultLimit });
  renderResultList(filenameResultsEl, results.filename_hits);
  renderSymbolResults(symbolResultsEl, results.symbol_hits);
  renderResultList(contentResultsEl, results.content_hits);
  activeIndex = 0;
  highlightActive();
}

function openSearchModal() {
  searchModal.classList.remove("hidden");
  searchInput.value = "";
  searchInput.focus();
  filenameResultsEl.textContent = "";
  symbolResultsEl.textContent = "";
  contentResultsEl.textContent = "";
}

function closeSearchModal() {
  searchModal.classList.add("hidden");
}

async function selectResult(path) {
  closeSearchModal();
  await openFileInViewer(path);
}

searchBarBtn.addEventListener("click", openSearchModal);
searchInput.addEventListener("input", (e) => runSearch(e.target.value));
searchInput.addEventListener("keydown", (e) => {
  const items = flatResults();
  if (e.key === "ArrowDown") {
    e.preventDefault();
    activeIndex = Math.min(activeIndex + 1, items.length - 1);
    highlightActive();
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    activeIndex = Math.max(activeIndex - 1, 0);
    highlightActive();
  } else if (e.key === "Enter") {
    if (items[activeIndex]) selectResult(items[activeIndex].path);
  } else if (e.key === "Escape") {
    closeSearchModal();
  }
});
searchModal.addEventListener("click", (e) => {
  if (e.target === searchModal) closeSearchModal();
});

document.addEventListener("keydown", (e) => {
  const mod = e.ctrlKey || e.metaKey;
  if (mod && e.key.toLowerCase() === "o") {
    e.preventDefault();
    openSearchModal();
  } else if (mod && e.key === ",") {
    e.preventDefault();
    openSettings();
  } else if (mod && e.key.toLowerCase() === "s" && isEditing) {
    e.preventDefault(); // otherwise the webview's native "save page" dialog wins
    saveCurrentFile();
  } else if (e.key === "Escape") {
    closeSettings();
    closeSuggestions();
  }
});

// ---- viewer ----

const IMAGE_MIME_BY_EXT = { png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp", svg: "image/svg+xml", bmp: "image/bmp", ico: "image/x-icon" };
const AUDIO_MIME_BY_EXT = { mp3: "audio/mpeg", wav: "audio/wav", ogg: "audio/ogg", flac: "audio/flac", m4a: "audio/mp4", aac: "audio/aac" };

function extOf(path) {
  const name = path.split("/").pop();
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

async function openFileInViewer(path) {
  // Not just "a different file" — re-clicking the file you're mid-edit on
  // (same tree row, or a search hit for it) must warn too, or it silently
  // discards the edit by falling straight through to resetEditorState().
  if (!confirmDiscardIfDirty()) return;
  resetEditorState();

  graphPane.classList.add("hidden");
  viewerPane.classList.remove("hidden");
  viewerEmpty.classList.add("hidden");
  viewerLoaded.classList.remove("hidden");
  viewerPath.textContent = path;
  currentOpenPath = path;
  renderFrontmatter([]);

  const ext = extOf(path);
  const mime = IMAGE_MIME_BY_EXT[ext] || AUDIO_MIME_BY_EXT[ext];
  viewerEditToggle.classList.toggle("hidden", Boolean(mime)); // editing binary media as text makes no sense
  if (mime) {
    await showMedia(path, ext, mime);
    return;
  }

  let view;
  try {
    view = await invoke("open_file", { path });
  } catch (e) {
    showPlainText(`(열 수 없음: ${e})`);
    return;
  }
  renderFrontmatter(view.frontmatter);
  currentFileContent = view.content;
  await renderViewerContent(path, view.content);
}

async function showMedia(path, ext, mime) {
  viewerContent.classList.add("hidden");
  viewerRendered.classList.add("hidden");
  viewerMedia.classList.remove("hidden");
  viewerImage.classList.add("hidden");
  viewerAudio.classList.add("hidden");
  viewerImage.removeAttribute("src");
  viewerAudio.removeAttribute("src");

  let base64;
  try {
    base64 = await invoke("read_file_base64", { path });
  } catch (e) {
    viewerMedia.classList.add("hidden");
    showPlainText(`(열 수 없음: ${e})`);
    return;
  }
  const dataUrl = `data:${mime};base64,${base64}`;

  if (IMAGE_MIME_BY_EXT[ext]) {
    viewerImage.src = dataUrl;
    viewerImage.alt = path;
    viewerImage.classList.remove("hidden");
  } else {
    viewerAudio.src = dataUrl;
    viewerAudio.classList.remove("hidden");
  }
}

async function renderViewerContent(path, content) {
  viewerMedia.classList.add("hidden");
  const ext = extOf(path);
  if (ext === "md" || ext === "markdown") {
    await showMarkdown(content);
  } else if (EXT_LANG[ext]) {
    await showHighlightedCode(content, EXT_LANG[ext]);
  } else {
    showPlainText(content);
  }
}

// ---- editing ----

function resetEditorState() {
  isEditing = false;
  isDirty = false;
  viewerEditToggle.textContent = "편집";
  viewerEditor.classList.add("hidden");
  viewerSaveStatus.textContent = "";
}

function enterEditMode() {
  isEditing = true;
  isDirty = false;
  viewerEditToggle.textContent = "보기";
  viewerSaveStatus.textContent = "";
  viewerRendered.classList.add("hidden");
  viewerContent.classList.add("hidden");
  viewerEditor.classList.remove("hidden");

  viewerEditorText.value = currentFileContent;
  viewerEditorText.style.fontFamily = appSettings.monoFont;

  const isMarkdown = ["md", "markdown"].includes(extOf(currentOpenPath));
  viewerEditor.classList.toggle("split", isMarkdown);
  viewerEditorPreview.classList.toggle("hidden", !isMarkdown);
  if (isMarkdown) updateEditorPreview(viewerEditorText.value);

  viewerEditorText.focus();
}

// Plain marked.parse, no Shiki syntax highlighting — that's async per code
// block and too slow to re-run on every keystroke. The saved-and-reopened
// view still gets full highlighting; this is just the live-while-typing one.
function updateEditorPreview(text) {
  viewerEditorPreview.innerHTML = marked.parse(text);
}

viewerEditorText.addEventListener("input", () => {
  isDirty = true;
  viewerSaveStatus.textContent = "저장 안 됨";
  if (!viewerEditorPreview.classList.contains("hidden")) {
    updateEditorPreview(viewerEditorText.value);
  }
});

async function saveCurrentFile() {
  if (!isEditing || !currentOpenPath) return;
  const content = viewerEditorText.value;
  try {
    await invoke("save_file", { path: currentOpenPath, content });
    currentFileContent = content;
    isDirty = false;
    viewerSaveStatus.textContent = "저장됨";
  } catch (e) {
    viewerSaveStatus.textContent = `저장 실패: ${e}`;
  }
}

viewerEditToggle.addEventListener("click", async () => {
  if (!isEditing) {
    enterEditMode();
    return;
  }
  if (isDirty && !confirm("저장하지 않은 변경사항이 있습니다. 저장하지 않고 닫을까요?")) return;
  resetEditorState();
  await renderViewerContent(currentOpenPath, currentFileContent);
});

function showPlainText(text) {
  viewerContent.textContent = text;
  viewerContent.classList.remove("hidden");
  viewerRendered.classList.add("hidden");
}

async function showMarkdown(text) {
  const container = document.createElement("div");
  container.className = "markdown-body";
  container.innerHTML = marked.parse(text);

  const blocks = [...container.querySelectorAll("pre code")];
  await Promise.all(
    blocks.map(async (block) => {
      const langClass = [...block.classList].find((c) => c.startsWith("language-"));
      const lang = langClass ? langClass.replace("language-", "") : "text";
      try {
        const html = await highlight(block.textContent, lang);
        if (!html) return; // not a bundled language — leave the plain block
        const wrapper = document.createElement("div");
        wrapper.innerHTML = html;
        block.closest("pre")?.replaceWith(wrapper.firstElementChild);
      } catch {
        // grammar failed to load — plain block is a fine fallback
      }
    })
  );

  viewerRendered.replaceChildren(container);
  viewerRendered.classList.remove("hidden");
  viewerContent.classList.add("hidden");
}

async function showHighlightedCode(text, lang) {
  try {
    const html = await highlight(text, lang);
    if (!html) {
      showPlainText(text);
      return;
    }
    viewerRendered.innerHTML = html;
    viewerRendered.classList.remove("hidden");
    viewerContent.classList.add("hidden");
  } catch {
    showPlainText(text);
  }
}

function renderFrontmatter(pairs) {
  viewerFrontmatter.textContent = "";
  if (!pairs || pairs.length === 0) {
    viewerFrontmatter.classList.add("hidden");
    return;
  }
  viewerFrontmatter.classList.remove("hidden");
  for (const [key, value] of pairs) {
    const tr = document.createElement("tr");
    const td1 = document.createElement("td");
    td1.textContent = key;
    const td2 = document.createElement("td");
    td2.textContent = value;
    tr.append(td1, td2);
    viewerFrontmatter.appendChild(tr);
  }
}

// ---- graph view ----

// Versioned key: the force parameters were rescaled (center is now 0..1
// gravity, not forceCenter strength), so previously-saved values would mean
// something different. Bumping the key resets them instead of silently
// applying numbers from the old scale.
const GRAPH_SETTINGS_KEY = "polyglot-vault:graph-settings:v2";
const DEFAULT_GRAPH = {
  showFolders: true,
  showOrphans: true,
  labelThreshold: 0.6,
  nodeSize: 1,
  linkWidth: 1,
  langColors: true,
  center: 0.5,
  charge: 30,
  linkStrength: 0.7,
  linkDistance: 20,
  limitNodes: false, // off by default — a low-spec machine opts into this, it isn't forced on everyone
  maxNodes: 1500,
  folderColor: FOLDER_COLOR,
  otherFileColor: "#9fd89f", // dot color for files with no known language, when langColors is off entirely
  relColors: { contains: "#555555", references: "#5b9bd5", describes: "#e0c341", imports: "#82e0aa" },
  relDash: { contains: "solid", references: "dashed", describes: "dotted", imports: "dashdot" },
  langColorOverrides: {}, // ext -> hex, only for extensions the user has actually repicked
};

// Repulsion beyond this distance is ignored. Without a cap, every node pushes
// every other node forever and a few thousand files drift into a huge sparse
// ring — the "too far apart" problem. Also makes each tick cheaper.
const CHARGE_DISTANCE_MAX = 260;

// The "중심 장력" slider is 0..1; d3's x/y gravity wants a much smaller number,
// so scale it here instead of making the slider read 0.00–0.10.
const GRAVITY_SCALE = 0.12;

function loadGraphSettings() {
  try {
    return { ...DEFAULT_GRAPH, ...JSON.parse(localStorage.getItem(GRAPH_SETTINGS_KEY) || "{}") };
  } catch {
    return { ...DEFAULT_GRAPH };
  }
}

let graphSettings = loadGraphSettings();
let graphData = null;
let sim = null;
let filterText = "";

const gs = {
  panel: document.getElementById("graph-settings"),
  toggle: document.getElementById("graph-settings-toggle"),
  reset: document.getElementById("graph-reset"),
  filterText: document.getElementById("gs-filter-text"),
  showFolders: document.getElementById("gs-show-folders"),
  showOrphans: document.getElementById("gs-show-orphans"),
  labelThreshold: document.getElementById("gs-label-threshold"),
  nodeSize: document.getElementById("gs-node-size"),
  linkWidth: document.getElementById("gs-link-width"),
  langColors: document.getElementById("gs-lang-colors"),
  center: document.getElementById("gs-center"),
  charge: document.getElementById("gs-charge"),
  linkStrength: document.getElementById("gs-link-strength"),
  linkDistance: document.getElementById("gs-link-distance"),
  limitNodes: document.getElementById("gs-limit-nodes"),
  maxNodes: document.getElementById("gs-max-nodes"),
  truncated: document.getElementById("graph-truncated"),
  folderColor: document.getElementById("gs-folder-color"),
  otherFileColor: document.getElementById("gs-otherfile-color"),
  relContains: document.getElementById("gs-rel-contains"),
  relReferences: document.getElementById("gs-rel-references"),
  relDescribes: document.getElementById("gs-rel-describes"),
  relImports: document.getElementById("gs-rel-imports"),
  relStyleContains: document.getElementById("gs-rel-style-contains"),
  relStyleReferences: document.getElementById("gs-rel-style-references"),
  relStyleDescribes: document.getElementById("gs-rel-style-describes"),
  relStyleImports: document.getElementById("gs-rel-style-imports"),
  legend: document.getElementById("gs-legend"),
  animate: document.getElementById("gs-animate"),
  animDate: document.getElementById("gs-anim-date"),
};

const gsOut = {
  labelThreshold: document.getElementById("gs-label-out"),
  nodeSize: document.getElementById("gs-node-out"),
  linkWidth: document.getElementById("gs-link-out"),
  center: document.getElementById("gs-center-out"),
  charge: document.getElementById("gs-charge-out"),
  linkStrength: document.getElementById("gs-linkstr-out"),
  linkDistance: document.getElementById("gs-linkdist-out"),
  maxNodes: document.getElementById("gs-maxnodes-out"),
};

function syncGraphControls() {
  gs.showFolders.checked = graphSettings.showFolders;
  gs.showOrphans.checked = graphSettings.showOrphans;
  gs.langColors.checked = graphSettings.langColors;
  gs.limitNodes.checked = graphSettings.limitNodes;
  gs.maxNodes.disabled = !graphSettings.limitNodes;
  gs.folderColor.value = graphSettings.folderColor;
  gs.otherFileColor.value = graphSettings.otherFileColor;
  gs.relContains.value = graphSettings.relColors.contains;
  gs.relReferences.value = graphSettings.relColors.references;
  gs.relDescribes.value = graphSettings.relColors.describes;
  gs.relStyleContains.value = graphSettings.relDash.contains;
  gs.relStyleReferences.value = graphSettings.relDash.references;
  gs.relStyleDescribes.value = graphSettings.relDash.describes;
  gs.relImports.value = graphSettings.relColors.imports;
  gs.relStyleImports.value = graphSettings.relDash.imports;
  for (const key of ["labelThreshold", "nodeSize", "linkWidth", "center", "charge", "linkStrength", "linkDistance", "maxNodes"]) {
    gs[key].value = graphSettings[key];
    gsOut[key].textContent = graphSettings[key];
  }
}

function saveGraphSettings() {
  localStorage.setItem(GRAPH_SETTINGS_KEY, JSON.stringify(graphSettings));
}

gs.toggle.addEventListener("click", () => {
  gs.panel.classList.toggle("hidden");
});
gs.reset.addEventListener("click", () => {
  graphSettings = { ...DEFAULT_GRAPH };
  saveGraphSettings();
  syncGraphControls();
  renderGraph();
});
gs.filterText.addEventListener("input", (e) => {
  filterText = e.target.value.trim().toLowerCase();
  renderGraph();
});

// Structure-changing settings need a re-render; force settings just retune
// the running simulation, which keeps the layout instead of resetting it.
for (const key of ["showFolders", "showOrphans", "langColors"]) {
  gs[key].addEventListener("change", (e) => {
    graphSettings[key] = e.target.checked;
    saveGraphSettings();
    renderGraph();
  });
}
for (const key of ["labelThreshold", "nodeSize", "linkWidth", "center", "charge", "linkStrength", "linkDistance"]) {
  gs[key].addEventListener("input", (e) => {
    graphSettings[key] = parseFloat(e.target.value);
    gsOut[key].textContent = e.target.value;
    saveGraphSettings();
    applyGraphLiveSettings();
  });
}
// Off by default (large vaults kept getting silently cut to 1500 nodes with
// no obvious way to see why) — a low-spec machine opts in via this checkbox
// instead. Node count changes which nodes exist, not just how they're laid
// out, so both handlers re-render rather than retuning the running
// simulation.
gs.limitNodes.addEventListener("change", (e) => {
  graphSettings.limitNodes = e.target.checked;
  gs.maxNodes.disabled = !graphSettings.limitNodes;
  saveGraphSettings();
  renderGraph();
});
// Fires on "change" (mouse released), not "input" — re-rendering on every
// step of the drag is exactly the stall this setting exists to avoid.
gs.maxNodes.addEventListener("input", (e) => {
  gsOut.maxNodes.textContent = e.target.value;
});
gs.maxNodes.addEventListener("change", (e) => {
  graphSettings.maxNodes = parseInt(e.target.value, 10);
  saveGraphSettings();
  renderGraph();
});

// Color pickers: "input" for live feedback while dragging inside the native
// picker, applied via applyGraphLiveSettings (fill/stroke only, no sim
// restart) — a full renderGraph() here would visibly reset node positions
// on every drag step inside the color wheel.
gs.folderColor.addEventListener("input", (e) => {
  graphSettings.folderColor = e.target.value;
  saveGraphSettings();
  applyGraphLiveSettings();
});
gs.otherFileColor.addEventListener("input", (e) => {
  graphSettings.otherFileColor = e.target.value;
  saveGraphSettings();
  applyGraphLiveSettings();
});
const relColorInputs = { contains: gs.relContains, references: gs.relReferences, describes: gs.relDescribes, imports: gs.relImports };
for (const [rel, input] of Object.entries(relColorInputs)) {
  input.addEventListener("input", (e) => {
    graphSettings.relColors = { ...graphSettings.relColors, [rel]: e.target.value };
    saveGraphSettings();
    applyGraphLiveSettings();
  });
}
const relStyleInputs = {
  contains: gs.relStyleContains,
  references: gs.relStyleReferences,
  describes: gs.relStyleDescribes,
  imports: gs.relStyleImports,
};
for (const [rel, select] of Object.entries(relStyleInputs)) {
  select.addEventListener("change", (e) => {
    graphSettings.relDash = { ...graphSettings.relDash, [rel]: e.target.value };
    saveGraphSettings();
    applyGraphLiveSettings();
  });
}

graphToggle.addEventListener("click", async () => {
  const showingGraph = !graphPane.classList.contains("hidden");
  if (showingGraph) {
    graphPane.classList.add("hidden");
    viewerPane.classList.remove("hidden");
    return;
  }
  viewerPane.classList.add("hidden");
  graphPane.classList.remove("hidden");
  if (!graphData) {
    graphData = await invoke("graph_data");
    syncGraphControls();
  }
  renderGraph();
});

function visibleGraph() {
  let nodes = graphData.nodes;
  if (!graphSettings.showFolders) nodes = nodes.filter((n) => n.kind === "file");
  if (filterText) nodes = nodes.filter((n) => n.label.toLowerCase().includes(filterText));

  const ids = new Set(nodes.map((n) => n.id));
  let edges = graphData.edges.filter((e) => ids.has(e.source.id ?? e.source) && ids.has(e.target.id ?? e.target));

  if (!graphSettings.showOrphans) {
    const connected = new Set();
    for (const e of edges) {
      connected.add(e.source.id ?? e.source);
      connected.add(e.target.id ?? e.target);
    }
    nodes = nodes.filter((n) => connected.has(n.id));
  }

  // Node cap, opt-in (graphSettings.limitNodes). Every force tick is
  // O(nodes) for gravity/collide plus a quadtree pass for charge, and each
  // tick writes 3 DOM attributes per node/link — so a few thousand files
  // pin a slow CPU at 100% for the whole settling period. Off by default:
  // a vault with thousands of files silently rendering only 1500 of them,
  // with no visible reason, is worse than a slow but complete graph — a
  // low-spec machine turns this on deliberately instead. When it's on, keep
  // the most-connected nodes (the ones the graph is actually about) and
  // drop the long tail of leaves.
  let truncatedFrom = 0;
  if (graphSettings.limitNodes && nodes.length > graphSettings.maxNodes) {
    truncatedFrom = nodes.length;
    const degree = new Map();
    for (const e of edges) {
      const s = e.source.id ?? e.source;
      const t = e.target.id ?? e.target;
      degree.set(s, (degree.get(s) || 0) + 1);
      degree.set(t, (degree.get(t) || 0) + 1);
    }
    nodes = [...nodes].sort((a, b) => (degree.get(b.id) || 0) - (degree.get(a.id) || 0)).slice(0, graphSettings.maxNodes);
    const kept = new Set(nodes.map((n) => n.id));
    edges = edges.filter((e) => kept.has(e.source.id ?? e.source) && kept.has(e.target.id ?? e.target));
  }

  // d3 mutates source/target into object refs; hand it fresh copies so
  // re-rendering after a filter change doesn't reuse stale bindings.
  return {
    nodes: nodes.map((n) => ({ ...n })),
    edges: edges.map((e) => ({ source: e.source.id ?? e.source, target: e.target.id ?? e.target, rel: e.rel })),
    truncatedFrom,
  };
}

// `contains` (folder structure) defaults to a plain gray solid line — it's
// the overwhelming majority of edges and is background structure, not
// information. `references`/`describes` are real links (R2 auto-applied,
// R1 approved) and default to a dash so they read as a distinct, smaller
// category standing out against the folder tree. Both color and line style
// per rel are user-customizable (graphSettings.relColors/relDash).
const LINE_STYLES = { solid: null, dashed: "6,3", dotted: "1,3", dashdot: "6,3,1,3" };

function linkColor(d) {
  return graphSettings.relColors[d.rel] || "#555";
}
function linkDash(d) {
  return LINE_STYLES[graphSettings.relDash[d.rel]] ?? null;
}
function nodeFillColor(d) {
  if (d.kind === "folder") return graphSettings.folderColor;
  if (!graphSettings.langColors) return graphSettings.otherFileColor;
  return colorForExt(d.ext, graphSettings.langColorOverrides);
}

let graphSelections = null;

function applyGraphLiveSettings() {
  if (!sim || !graphSelections) return;
  const { node, link, labels, degree } = graphSelections;
  node.attr("r", (d) => nodeRadius(d, degree)).attr("fill", nodeFillColor);
  link.attr("stroke-width", graphSettings.linkWidth).attr("stroke", linkColor).attr("stroke-dasharray", linkDash);
  labels.style("display", (d) => (labelVisible(d, degree) ? null : "none"));

  sim.force("charge").strength(-graphSettings.charge);
  sim.force("link").strength(graphSettings.linkStrength).distance(graphSettings.linkDistance);
  // forceCenter only recenters the centroid — it can't pull nodes together.
  // The x/y forces are the ones that actually act as gravity.
  sim.force("x").strength(graphSettings.center * GRAVITY_SCALE);
  sim.force("y").strength(graphSettings.center * GRAVITY_SCALE);
  sim.alpha(0.5).restart();
}

// Tuned by eye against Obsidian's graph look (small leaf dots, hubs only a few
// times larger) — not a copy of its internal formula, which isn't published.
// sqrt keeps hubs from exploding: 1 link -> 2.6px, 100 links -> ~9px.
function nodeRadius(d, degree) {
  return (1.8 + Math.sqrt(degree.get(d.id) || 1) * 0.75) * graphSettings.nodeSize;
}

function labelVisible(d, degree) {
  // Higher threshold = only well-connected nodes keep their label, which is
  // how the graph stays readable at thousands of nodes.
  const maxDeg = degree.maxDegree || 1;
  const norm = (degree.get(d.id) || 0) / maxDeg;
  return norm >= 1 - graphSettings.labelThreshold;
}

function renderGraph() {
  if (!graphData) return;
  stopAnimation(); // a running animation holds selections we're about to discard
  const data = visibleGraph();
  const svgEl = document.getElementById("graph-svg");
  const width = svgEl.clientWidth || 900;
  const height = svgEl.clientHeight || 700;

  const degree = new Map();
  for (const e of data.edges) {
    degree.set(e.source, (degree.get(e.source) || 0) + 1);
    degree.set(e.target, (degree.get(e.target) || 0) + 1);
  }
  degree.maxDegree = Math.max(1, ...degree.values());

  const svg = select(svgEl).attr("viewBox", [0, 0, width, height]);
  svg.selectAll("*").remove();
  const root = svg.append("g");
  svg.call(zoom().on("zoom", (event) => root.attr("transform", event.transform)));

  const link = root
    .append("g")
    .attr("stroke-opacity", 0.6)
    .selectAll("line")
    .data(data.edges)
    .join("line")
    .attr("stroke", linkColor)
    .attr("stroke-dasharray", linkDash)
    .attr("stroke-width", graphSettings.linkWidth);

  const node = root
    .append("g")
    .selectAll("circle")
    .data(data.nodes)
    .join("circle")
    .attr("r", (d) => nodeRadius(d, degree))
    .attr("fill", nodeFillColor)
    .style("cursor", (d) => (d.kind === "file" ? "pointer" : "default"))
    .on("click", (_event, d) => {
      if (d.kind === "file") openFileInViewer(d.id);
    })
    .call(
      drag()
        .on("start", (event, d) => {
          if (!event.active) sim.alphaTarget(0.3).restart();
          d.fx = d.x;
          d.fy = d.y;
        })
        .on("drag", (event, d) => {
          d.fx = event.x;
          d.fy = event.y;
        })
        .on("end", (event, d) => {
          if (!event.active) sim.alphaTarget(0);
          d.fx = null;
          d.fy = null;
        })
    );

  node.append("title").text((d) => d.id || "(vault)");

  const labels = root
    .append("g")
    .selectAll("text")
    .data(data.nodes)
    .join("text")
    .text((d) => d.label)
    .attr("font-size", 6.5)
    .attr("fill", "#aaa")
    .attr("text-anchor", "middle")
    .attr("pointer-events", "none")
    .style("display", (d) => (labelVisible(d, degree) ? null : "none"));

  sim = forceSimulation(data.nodes)
    .force(
      "link",
      forceLink(data.edges).id((d) => d.id).distance(graphSettings.linkDistance).strength(graphSettings.linkStrength)
    )
    .force("charge", forceManyBody().strength(-graphSettings.charge).distanceMax(CHARGE_DISTANCE_MAX))
    .force("center", forceCenter(width / 2, height / 2))
    .force("x", forceX(width / 2).strength(graphSettings.center * GRAVITY_SCALE))
    .force("y", forceY(height / 2).strength(graphSettings.center * GRAVITY_SCALE))
    .force("collide", forceCollide((d) => nodeRadius(d, degree) + 1));

  sim.on("tick", () => {
    link
      .attr("x1", (d) => d.source.x)
      .attr("y1", (d) => d.source.y)
      .attr("x2", (d) => d.target.x)
      .attr("y2", (d) => d.target.y);
    node.attr("cx", (d) => d.x).attr("cy", (d) => d.y);
    labels.attr("x", (d) => d.x).attr("y", (d) => d.y - nodeRadius(d, degree) - 2);
  });

  graphSelections = { node, link, labels, degree };
  renderLegend(data.nodes);

  gs.truncated.classList.toggle("hidden", !data.truncatedFrom);
  if (data.truncatedFrom) {
    gs.truncated.textContent = `${data.truncatedFrom.toLocaleString()}개 중 연결이 많은 ${data.nodes.length.toLocaleString()}개만 표시 중입니다.`;
  }
}

// ---- timeline animation: reveal nodes in the order the files were created,
// the way Obsidian's "애니메이션 재생" does. The layout is computed once and
// left alone — only visibility changes, so nodes grow into a settled graph
// instead of the whole thing rearranging while you watch. ----

const ANIM_DURATION_MS = 9000;
let animFrame = null;

function stopAnimation() {
  if (animFrame !== null) {
    cancelAnimationFrame(animFrame);
    animFrame = null;
  }
  gs.animate.textContent = "▶ 애니메이션 재생";
  gs.animDate.textContent = "";
  if (graphSelections) {
    graphSelections.node.style("display", null).style("opacity", 1);
    graphSelections.link.style("display", null).style("opacity", null);
    graphSelections.labels.style("display", (d) => (labelVisible(d, graphSelections.degree) ? null : "none"));
  }
}

function formatDate(ms) {
  const d = new Date(ms);
  return `${d.getFullYear()}. ${String(d.getMonth() + 1).padStart(2, "0")}. ${String(d.getDate()).padStart(2, "0")}`;
}

function playAnimation() {
  if (!graphSelections) return;
  if (animFrame !== null) {
    stopAnimation();
    return;
  }

  const { node, link, labels, degree } = graphSelections;
  const times = node.data().map((d) => d.time).filter((t) => t > 0);
  if (times.length === 0) {
    gs.animDate.textContent = "시간 정보 없음";
    return;
  }
  const min = Math.min(...times);
  const max = Math.max(...times);
  // All files created at once (fresh clone) would make a 0-length timeline;
  // give it a span so the animation still plays instead of dividing by zero.
  const span = Math.max(max - min, 1);

  gs.animate.textContent = "■ 정지";
  const start = performance.now();

  const step = (now) => {
    const progress = Math.min((now - start) / ANIM_DURATION_MS, 1);
    const cursor = min + span * progress;

    node.style("display", (d) => (d.time <= cursor ? null : "none"));
    labels.style("display", (d) => (d.time <= cursor && labelVisible(d, degree) ? null : "none"));
    link.style("display", (d) => (d.source.time <= cursor && d.target.time <= cursor ? null : "none"));
    gs.animDate.textContent = formatDate(cursor);

    if (progress < 1) {
      animFrame = requestAnimationFrame(step);
    } else {
      animFrame = null;
      gs.animate.textContent = "▶ 애니메이션 재생";
    }
  };
  animFrame = requestAnimationFrame(step);
}

gs.animate.addEventListener("click", playAnimation);

function renderLegend(nodes) {
  gs.legend.textContent = "";
  const items = legendFor(nodes, graphSettings.langColorOverrides).slice(0, 14);
  if (items.length === 0) {
    gs.legend.textContent = "—";
    return;
  }
  for (const { ext, count, color } of items) {
    const item = document.createElement("label");
    item.className = "legend-item";
    const swatch = document.createElement("input");
    swatch.type = "color";
    swatch.className = "legend-swatch";
    swatch.value = color;
    // "input" (not "change") for live feedback while the picker is open —
    // cheap here since it only touches fill/stroke, not the simulation.
    swatch.addEventListener("input", (e) => {
      graphSettings.langColorOverrides = { ...graphSettings.langColorOverrides, [ext]: e.target.value };
      saveGraphSettings();
      applyGraphLiveSettings();
    });
    const label = document.createElement("span");
    label.textContent = `${ext} ${count}`;
    item.append(swatch, label);
    gs.legend.appendChild(item);
  }
}

// ---- settings modal ----

const settingsModal = document.getElementById("settings-modal");
const settingsBtn = document.getElementById("settings-btn");
const settingsClose = document.getElementById("settings-close");
const setAccent = document.getElementById("set-accent");
const setFontSize = document.getElementById("set-font-size");
const setMonoFont = document.getElementById("set-mono-font");
const setResultLimit = document.getElementById("set-result-limit");
const setUseGitignore = document.getElementById("set-use-gitignore");
const setIgnorePatterns = document.getElementById("set-ignore-patterns");
const setContentKb = document.getElementById("set-content-kb");
const settingsSave = document.getElementById("settings-save");
const settingsSaveStatus = document.getElementById("settings-save-status");
const aboutTable = document.getElementById("about-table");

let vaultConfig = null;

// ---- suggestions (R1: doc backtick token ↔ symbol, needs approval) ----

const suggestionsModal = document.getElementById("suggestions-modal");
const suggestionsToggle = document.getElementById("suggestions-toggle");
const suggestionsClose = document.getElementById("suggestions-close");
const suggestionsList = document.getElementById("suggestions-list");
const suggestionsEmpty = document.getElementById("suggestions-empty");
const suggestionsCount = document.getElementById("suggestions-count");

function addrToPath(addr) {
  return addr.replace(/^vault:\/\//, "");
}

async function refreshSuggestionsBadge() {
  const items = await invoke("list_suggestions");
  suggestionsCount.textContent = items.length;
  suggestionsCount.classList.toggle("hidden", items.length === 0);
}

function renderSuggestions(items) {
  suggestionsList.textContent = "";
  suggestionsEmpty.classList.toggle("hidden", items.length > 0);
  for (const item of items) {
    const li = document.createElement("li");
    li.className = "suggestion-row";

    const desc = document.createElement("div");
    desc.className = "suggestion-desc";
    // Built with textContent, not innerHTML — item.token/from/to come from
    // arbitrary vault file content (a doc's backtick text), so treating it
    // as HTML would let a file's own content inject markup into the app.
    const tokenEl = document.createElement("code");
    tokenEl.textContent = item.token;
    const fromEl = document.createElement("span");
    fromEl.className = "suggestion-from";
    fromEl.textContent = addrToPath(item.from);
    const toEl = document.createElement("span");
    toEl.className = "suggestion-to";
    toEl.textContent = addrToPath(item.to);
    desc.append(tokenEl, " — ", fromEl, " → ", toEl);
    if (item.mention_count > 1) {
      const badge = document.createElement("span");
      badge.className = "suggestion-mentions";
      badge.textContent = `${item.mention_count}회 언급`;
      desc.appendChild(badge);
    }

    const actions = document.createElement("div");
    actions.className = "suggestion-actions";
    const accept = document.createElement("button");
    accept.className = "primary";
    accept.textContent = "승인";
    accept.addEventListener("click", () => decideSuggestion(item, "accept", li));
    const reject = document.createElement("button");
    reject.textContent = "거절";
    reject.addEventListener("click", () => decideSuggestion(item, "reject", li));
    actions.append(accept, reject);

    li.append(desc, actions);
    suggestionsList.appendChild(li);
  }
}

async function decideSuggestion(item, verdict, li) {
  li.classList.add("deciding");
  try {
    await invoke("decide_suggestion", { from: item.from, to: item.to, verdict });
    li.remove();
    suggestionsEmpty.classList.toggle("hidden", suggestionsList.children.length > 0);
    if (verdict === "accept") graphData = null; // a new real link exists; rebuild on next graph open
    await refreshSuggestionsBadge();
  } catch (e) {
    li.classList.remove("deciding");
    alert(`처리 실패: ${e}`);
  }
}

async function openSuggestions() {
  const items = await invoke("list_suggestions");
  renderSuggestions(items);
  suggestionsModal.classList.remove("hidden");
}

function closeSuggestions() {
  suggestionsModal.classList.add("hidden");
}

suggestionsToggle.addEventListener("click", openSuggestions);
suggestionsClose.addEventListener("click", closeSuggestions);
suggestionsModal.addEventListener("click", (e) => {
  if (e.target === suggestionsModal) closeSuggestions();
});

for (const tab of document.querySelectorAll(".settings-tab")) {
  tab.addEventListener("click", () => {
    document.querySelectorAll(".settings-tab").forEach((t) => t.classList.toggle("active", t === tab));
    document
      .querySelectorAll(".settings-panel")
      .forEach((p) => p.classList.toggle("hidden", p.dataset.panel !== tab.dataset.tab));
  });
}

async function openSettings() {
  setAccent.value = appSettings.accent;
  setFontSize.value = appSettings.fontSize;
  setMonoFont.value = appSettings.monoFont;
  setResultLimit.value = appSettings.resultLimit;

  try {
    vaultConfig = await invoke("get_vault_config");
    setUseGitignore.checked = vaultConfig.ignore.use_gitignore;
    setIgnorePatterns.value = vaultConfig.ignore.patterns.join("\n");
    setContentKb.value = Math.round(vaultConfig.limits.content_bytes / 1024);
    settingsSaveStatus.textContent = "";
  } catch (e) {
    settingsSaveStatus.textContent = `vault.toml 읽기 실패: ${e}`;
  }

  await renderAbout();
  settingsModal.classList.remove("hidden");
}

function closeSettings() {
  settingsModal.classList.add("hidden");
}

async function renderAbout() {
  aboutTable.textContent = "";
  const rows = [["앱", "폴리곤 0.1.0"]];
  try {
    const stats = await invoke("vault_stats");
    rows.push(
      ["Vault 경로", stats.path],
      ["파일", `${stats.file_count.toLocaleString()}개`],
      ["본문 인덱싱된 파일", `${stats.indexed_docs.toLocaleString()}개`],
      ["태그", `${stats.tag_count.toLocaleString()}개`],
      ["경로 테이블", `${(stats.path_table_bytes / 1048576).toFixed(2)} MB`],
      ["본문 인덱스", `${(stats.index_bytes / 1048576).toFixed(2)} MB`]
    );
  } catch (e) {
    rows.push(["오류", String(e)]);
  }
  for (const [k, v] of rows) {
    const tr = document.createElement("tr");
    const td1 = document.createElement("td");
    td1.textContent = k;
    const td2 = document.createElement("td");
    td2.textContent = v;
    tr.append(td1, td2);
    aboutTable.appendChild(tr);
  }
}

settingsBtn.addEventListener("click", openSettings);
settingsClose.addEventListener("click", closeSettings);
settingsModal.addEventListener("click", (e) => {
  if (e.target === settingsModal) closeSettings();
});

// App-level settings apply immediately — no save button, like Obsidian.
setAccent.addEventListener("input", (e) => {
  appSettings.accent = e.target.value;
  applyAppSettings();
});
setFontSize.addEventListener("input", (e) => {
  appSettings.fontSize = parseInt(e.target.value, 10);
  applyAppSettings();
});
setMonoFont.addEventListener("change", (e) => {
  appSettings.monoFont = e.target.value;
  applyAppSettings();
});
setResultLimit.addEventListener("change", (e) => {
  appSettings.resultLimit = parseInt(e.target.value, 10) || DEFAULT_APP_SETTINGS.resultLimit;
  applyAppSettings();
});

// Vault settings need an explicit save: they change what's indexed, so the
// index is rebuilt and the whole sidebar/graph must refresh.
settingsSave.addEventListener("click", async () => {
  if (!vaultConfig) return;
  settingsSaveStatus.textContent = "저장하고 다시 인덱싱 중...";
  vaultConfig.ignore.use_gitignore = setUseGitignore.checked;
  vaultConfig.ignore.patterns = setIgnorePatterns.value
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean);
  vaultConfig.limits.content_bytes = Math.max(1, parseInt(setContentKb.value, 10) || 1024) * 1024;

  try {
    const count = await invoke("save_vault_config", { config: vaultConfig });
    settingsSaveStatus.textContent = `완료 — ${count}개 파일`;
    graphData = null; // structure changed; rebuild on next graph open
    activeTag = null;
    await loadSidebar();
    vaultNameEl.textContent = `${vaultDisplayName(currentVaultPath)} (${count})`;
    await renderAbout();
  } catch (e) {
    settingsSaveStatus.textContent = `오류: ${e}`;
  }
});

// ---- live updates: the backend watches the vault filesystem and emits this
// whenever the watcher or its periodic reconcile backstop actually changes
// the index (see `spawn_watcher` in lib.rs) ----
let liveRefreshTimer = null;
let pendingChangedPaths = new Set();
listen("vault-changed", (event) => {
  for (const p of event.payload || []) pendingChangedPaths.add(p);
  clearTimeout(liveRefreshTimer);
  liveRefreshTimer = setTimeout(() => {
    const paths = pendingChangedPaths;
    pendingChangedPaths = new Set();
    applyLiveChange(paths);
  }, 150);
});

async function applyLiveChange(changedPaths) {
  await loadSidebar();
  graphData = null; // folder/file set may have changed; rebuild on next graph open
  if (!graphPane.classList.contains("hidden")) {
    // The graph is on screen right now — invalidating alone silently leaves
    // it stale forever, since nothing else re-renders it until it's closed
    // and reopened. Most visibly: R1/R2's first pass runs on the watcher
    // thread after the window already opened (kept off the open-vault
    // critical path), so a graph opened quickly shows zero real links until
    // this fires — it must actually redraw, not just mark itself dirty.
    graphData = await invoke("graph_data");
    renderGraph();
  }
  await refreshSuggestionsBadge();
  if (!suggestionsModal.classList.contains("hidden")) {
    renderSuggestions(await invoke("list_suggestions"));
  }

  if (!searchModal.classList.contains("hidden") && searchInput.value) {
    await runSearch(searchInput.value);
  }
  if (currentOpenPath && !isEditing) {
    if (!allFiles.includes(currentOpenPath)) {
      currentOpenPath = null; // removed out from under the viewer
    } else if (changedPaths.has(currentOpenPath)) {
      // Only reopen if the open file itself changed — reopening on every
      // unrelated vault change re-fetches and resets <img>/<audio> src,
      // which flickers even though the shown file never changed.
      await openFileInViewer(currentOpenPath);
    }
  }
}

function initUpdateToast() {
  const toast = document.getElementById("update-toast");
  const applyBtn = document.getElementById("update-toast-apply");
  const dismissBtn = document.getElementById("update-toast-dismiss");
  listen("update-available", () => toast.classList.remove("hidden"));
  dismissBtn.addEventListener("click", () => toast.classList.add("hidden"));
  applyBtn.addEventListener("click", () => {
    applyBtn.textContent = "업데이트 중...";
    applyBtn.disabled = true;
    invoke("apply_update"); // backend exits the app once the rebuild script is spawned
  });
}
initUpdateToast();

init();
