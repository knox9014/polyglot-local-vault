// Saved vault list, shared by the launcher and main windows (same origin, so
// localStorage is shared). Obsidian-style: an entry stays until the user
// removes it — not silently evicted by recency.

const SAVED_KEY = "polyglot-vault:saved-vaults";
const SAVED_CAP = 20; // safety cap, not a UX-visible "recent" cutoff

export function getSavedVaults() {
  try {
    return JSON.parse(localStorage.getItem(SAVED_KEY) || "[]");
  } catch {
    return [];
  }
}

export function saveVault(path) {
  const list = [path, ...getSavedVaults().filter((p) => p !== path)].slice(0, SAVED_CAP);
  localStorage.setItem(SAVED_KEY, JSON.stringify(list));
}

export function removeSavedVault(path) {
  localStorage.setItem(SAVED_KEY, JSON.stringify(getSavedVaults().filter((p) => p !== path)));
}

export function vaultDisplayName(path) {
  return path.split(/[\\/]/).filter(Boolean).pop() || path;
}

export function vaultParentPath(path) {
  const parts = path.split(/[\\/]/).filter(Boolean);
  parts.pop();
  return parts.join("\\");
}
