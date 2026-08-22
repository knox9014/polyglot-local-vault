// Syntax highlighting via Shiki's fine-grained bundle.
//
// The convenient `shiki` entry point pulls in every language it supports —
// 100+ grammars, ~11MB of chunks — when this app maps exactly the ones below.
// Using `shiki/core` with an explicit lang map ships only these, and each is
// still a dynamic import so it's fetched the first time that language appears.

import { createHighlighterCore } from "shiki/core";
import { createOnigurumaEngine } from "shiki/engine/oniguruma";

// VS Code's actual built-in Dark+ theme.
const THEME = "dark-plus";

// Shiki language id -> its grammar module. Only these get bundled.
const LANG_LOADERS = {
  javascript: () => import("shiki/langs/javascript.mjs"),
  jsx: () => import("shiki/langs/jsx.mjs"),
  typescript: () => import("shiki/langs/typescript.mjs"),
  tsx: () => import("shiki/langs/tsx.mjs"),
  python: () => import("shiki/langs/python.mjs"),
  rust: () => import("shiki/langs/rust.mjs"),
  go: () => import("shiki/langs/go.mjs"),
  java: () => import("shiki/langs/java.mjs"),
  c: () => import("shiki/langs/c.mjs"),
  cpp: () => import("shiki/langs/cpp.mjs"),
  csharp: () => import("shiki/langs/csharp.mjs"),
  ruby: () => import("shiki/langs/ruby.mjs"),
  php: () => import("shiki/langs/php.mjs"),
  html: () => import("shiki/langs/html.mjs"),
  css: () => import("shiki/langs/css.mjs"),
  scss: () => import("shiki/langs/scss.mjs"),
  less: () => import("shiki/langs/less.mjs"),
  json: () => import("shiki/langs/json.mjs"),
  yaml: () => import("shiki/langs/yaml.mjs"),
  toml: () => import("shiki/langs/toml.mjs"),
  bash: () => import("shiki/langs/bash.mjs"),
  powershell: () => import("shiki/langs/powershell.mjs"),
  sql: () => import("shiki/langs/sql.mjs"),
  xml: () => import("shiki/langs/xml.mjs"),
  vue: () => import("shiki/langs/vue.mjs"),
  swift: () => import("shiki/langs/swift.mjs"),
  kotlin: () => import("shiki/langs/kotlin.mjs"),
  markdown: () => import("shiki/langs/markdown.mjs"),
};

// File extension -> Shiki language id.
export const EXT_LANG = {
  js: "javascript", mjs: "javascript", cjs: "javascript", jsx: "jsx",
  ts: "typescript", tsx: "tsx",
  py: "python", rs: "rust", go: "go", java: "java",
  c: "c", h: "c", cpp: "cpp", cc: "cpp", hpp: "cpp",
  cs: "csharp", rb: "ruby", php: "php",
  html: "html", htm: "html", css: "css", scss: "scss", less: "less",
  json: "json", yaml: "yaml", yml: "yaml", toml: "toml",
  sh: "bash", bash: "bash", ps1: "powershell",
  sql: "sql", xml: "xml", vue: "vue", swift: "swift", kt: "kotlin",
};

let highlighterPromise = null;
const loaded = new Set();

function getHighlighter() {
  // Created once, on first use — not at startup, so opening a vault of plain
  // text never pays for the highlighter or its wasm engine.
  highlighterPromise ??= createHighlighterCore({
    themes: [import("shiki/themes/dark-plus.mjs")],
    langs: [],
    engine: createOnigurumaEngine(import("shiki/wasm")),
  });
  return highlighterPromise;
}

/// Highlights `code` as `lang`, or returns null if that language isn't one we
/// bundle — callers fall back to plain text.
export async function highlight(code, lang) {
  const loader = LANG_LOADERS[lang];
  if (!loader) return null;

  const highlighter = await getHighlighter();
  if (!loaded.has(lang)) {
    await highlighter.loadLanguage(await loader());
    loaded.add(lang);
  }
  return highlighter.codeToHtml(code, { lang, theme: THEME });
}
