# CraftMC

A Rust-based PaperMC / Minecraft server manager with a web control panel.

- Single static binary, ships its own web UI.
- Listens on `http://0.0.0.0:3000/control` (password protected).
- Manages a `server.jar` child process: start / stop / restart / kill.
- Live xterm-style console over WebSocket with throttled batching (won't choke
  the browser when the server spams output).
- Persisted, downloadable console logs.
- Initial setup wizard: PaperMC URL, RAM (min/max), bind host, on-crash policy.
- Full server file manager (browse, edit, upload, download, mkdir, delete).
- Plugin manager: upload `.jar`, install from URL (with replace), enable/disable
  via `.disabled` rename, delete.
- Plugin config editor (browses `plugins/<name>/*.{yml,json,toml,conf,properties}`).
- `server.properties` and join-message editor.
- Auto-backup of worlds on a schedule, plus manual "backup now". Supports both
  the legacy layout (`world`, `world_nether`, `world_the_end`) and the 1.21+
  layout (`world/dimensions/minecraft/{overworld,the_nether,the_end}`).
- Region (`.mca`) inspector — visual 32×32 chunk grid, click a chunk to clear
  it (zeroed in the location header so Minecraft regenerates it). Server must
  be stopped while editing region files.
- Bind host is configurable to any IP or domain.

## Build

```
cargo build --release
```

The binary embeds the static UI files at compile time.

## Run

```
./target/release/craftmc
```

Open `http://localhost:3000/control` — first run takes you through setup.

## Files

- `data/config.toml` — manager config (password hash, paths, RAM, host, etc.).
- `data/logs/` — persisted manager + server console logs.
- `data/backups/` — zip archives of worlds.
- `<server_dir>/` — Minecraft server directory (default `server/`).

## Notes

- The auto-restart policy is configurable: `stop`, `restart`, or
  `restart_with_backoff` (5 s × attempt, capped at 60 s).
- The console buffer keeps 5000 lines in memory and streams new lines in 80 ms
  batches; lagged clients see a `[manager] dropped N buffered lines` notice
  rather than blocking the server.
- The plugin URL installer uses the URL's last path segment as filename.
- Region editing currently supports clearing chunks (forcing regen). Full
  block-level NBT edits are out of scope for this build.
