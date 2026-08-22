// Per-language node colors. Started as GitHub Linguist's published language
// colors, but those clustered badly once actually plotted on a hue wheel —
// 8 different languages (go/xml/py/ts/ps1/less/md/php) landed within a
// ~35° blue band, some only 2-3° apart, so nodes for genuinely different
// languages were indistinguishable at a glance. Rescaled to even ~13.3°
// hue steps (360°/27 chromatic entries) in the *same rank order* as the
// original hues, keeping each one's original lightness/saturation — so the
// overall "family" (Python-ish blue, Ruby-ish dark red, ...) is still
// roughly where it was, just guaranteed not to collide with its neighbors.
// `c`/`rst` are untouched: saturation 0 (grayscale), so hue is moot and
// they were never going to clash with a colored node anyway.
export const LANG_COLORS = {
  py: "#3580a5",
  js: "#9df15a", mjs: "#9df15a", cjs: "#9df15a", jsx: "#9df15a",
  ts: "#3173c6", tsx: "#3173c6",
  rs: "#dec084",
  go: "#00d8c0",
  java: "#9fb019",
  c: "#555555", h: "#555555",
  cpp: "#f34bbb", cc: "#f34bbb", hpp: "#f34bbb",
  cs: "#00861e",
  rb: "#701529",
  php: "#6e4f95",
  swift: "#f03838",
  kt: "#d37bff",
  vue: "#41b890",
  html: "#e35026", htm: "#e35026",
  css: "#753d7c", scss: "#c653b9", less: "#1d1d5d",
  sh: "#51e051", bash: "#51e051",
  ps1: "#011456",
  sql: "#97e300",
  md: "#2a08a1", markdown: "#2a08a1",
  rst: "#141414",
  json: "#60cb41",
  yaml: "#cb1767", yml: "#cb1767",
  toml: "#9c5821",
  xml: "#0099ac",
  ipynb: "#dac30b",
  csv: "#237347",
};

export const FOLDER_COLOR = "#e0e0e0";
export const OTHER_FILE_COLOR = "#6d7580"; // no known language — deliberately muted

/// `overrides` is the user's per-extension picks from the graph settings
/// legend (persisted in graphSettings.langColorOverrides) — checked before
/// the built-in table so a custom pick always wins.
export function colorForExt(ext, overrides = {}) {
  return overrides[ext] || LANG_COLORS[ext] || OTHER_FILE_COLOR;
}

/// Languages actually present in the given nodes, most common first —
/// so the legend shows this vault, not the whole table.
export function legendFor(nodes, overrides = {}) {
  const counts = new Map();
  for (const n of nodes) {
    if (n.kind !== "file" || !LANG_COLORS[n.ext]) continue;
    counts.set(n.ext, (counts.get(n.ext) || 0) + 1);
  }
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([ext, count]) => ({ ext, count, color: overrides[ext] || LANG_COLORS[ext] }));
}
