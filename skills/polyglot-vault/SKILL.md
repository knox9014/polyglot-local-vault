---
name: polyglot-vault
description: Use when you need to find or understand code, docs, or config in a local project without reading whole files. Searches a local Polyglot Vault index by filename, symbol, or text, reads only the needed symbol or outline, and follows import/doc-to-code links. Triggers - "where is X defined", "what does this module depend on", "which doc describes this function", large repos where grep/cat would burn tokens.
---

# Polyglot Vault (MCP)

Local, deterministic index of a folder (code, Markdown/rst, JSON/YAML/TOML, CSV, notebooks). Nothing leaves the machine; no AI inside the server.

## When to use

Use instead of `grep` + reading whole files when you need to locate or understand something. Prefer this order:

1. `vault_search` (`query`, optional `kind`: file | symbol | text) — get `vault://` URIs.
2. `vault_read` with `mode=outline` on a file URI — symbols only, no bodies. Cheapest way to decide what to read.
3. `vault_read` on a symbol URI (`vault://a.py#Class.method`) — just that symbol. Use `mode=range` + `start_line`/`end_line` only for slices.
4. `vault_neighbors` — follow `imports`/`references`/`describes` links. `direction=in` gives backlinks ("who uses / documents this").

Pass URIs exactly as returned; do not rewrite them. `neighbors_hint` in results tells you if a URI has links worth following.

`vault_propose_link` only queues a suggestion; the user must approve it in the desktop app. Use it sparingly, always with a `rationale`. Never tell the user a link exists before they approve.

Don't use it for: files outside the vault root, editing files, or exact-regex searches (use grep).

## Connect

Get `vault-mcp` (Windows x64 prebuilt: GitHub Releases `vault-mcp-windows-x64.exe`, or `cargo build --release --bin vault-mcp` in `polyglot-vault/`). The only argument is the vault root folder.

Claude Code:
```bash
claude mcp add polyglot-vault -- /path/to/vault-mcp /path/to/project
```

Cursor / Claude Desktop / other clients (`mcp.json`):
```json
{ "mcpServers": { "polyglot-vault": { "command": "/path/to/vault-mcp", "args": ["/path/to/project"] } } }
```

On startup the server logs `indexed N files` to stderr.

## If it fails

- Server not listed / won't start: run the binary by hand with the root path; wrong path prints `failed to open vault at ...`. Use absolute paths (on Windows, double backslashes in JSON).
- `No results.`: try `kind=text`, a shorter query, or a symbol's bare name; the file may be gitignored or in an unsupported format. Fall back to grep.
- Stale after edits: the index is built at startup; restart the MCP server.
- Tool error text is returned with `isError`; read it, fix the URI/argument, retry once, then fall back to normal file tools.
