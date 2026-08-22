const { invoke } = window.__TAURI__.core;
import { open as openFolderDialog } from "@tauri-apps/plugin-dialog";
import { getSavedVaults, removeSavedVault, vaultDisplayName, vaultParentPath } from "./vaults.js";

const listEl = document.getElementById("launcher-vault-list");
const openBtn = document.getElementById("launcher-open-btn");
const statusEl = document.getElementById("launcher-status");

function renderVaultList() {
  listEl.textContent = "";
  const saved = getSavedVaults();
  if (saved.length === 0) {
    const empty = document.createElement("p");
    empty.className = "launcher-empty";
    empty.textContent = "저장된 vault가 없습니다.";
    listEl.appendChild(empty);
    return;
  }
  for (const path of saved) {
    const row = document.createElement("div");
    row.className = "launcher-vault";

    const info = document.createElement("div");
    info.className = "launcher-vault-info";
    const name = document.createElement("div");
    name.className = "launcher-vault-name";
    name.textContent = vaultDisplayName(path);
    const parent = document.createElement("div");
    parent.className = "launcher-vault-path";
    parent.textContent = vaultParentPath(path);
    parent.title = path;
    info.append(name, parent);

    const remove = document.createElement("button");
    remove.className = "launcher-vault-remove";
    remove.textContent = "⋮";
    remove.title = "목록에서 제거";
    remove.addEventListener("click", (e) => {
      e.stopPropagation();
      removeSavedVault(path);
      renderVaultList();
    });

    row.append(info, remove);
    row.addEventListener("click", () => openVault(path));
    listEl.appendChild(row);
  }
}

async function openVault(path) {
  statusEl.textContent = "여는 중...";
  try {
    // Note: this call closes the launcher window on success, so nothing after
    // it is guaranteed to run. Remembering the vault is done by the main
    // window in init() instead, where it can't be cut off mid-way.
    await invoke("open_vault_window", { path });
  } catch (e) {
    statusEl.textContent = `오류: ${e}`;
  }
}

openBtn.addEventListener("click", async () => {
  const selected = await openFolderDialog({ directory: true, multiple: false, title: "Vault 폴더 선택" });
  if (selected) openVault(selected);
});

renderVaultList();
