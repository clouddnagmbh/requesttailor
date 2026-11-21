# @requesttailor/desktop

The RequestTailor desktop app: a Tauri 2 window showing the shared UI (`apps/ui`) on the
`TauriBackend` (`src/backend.ts`). Every `Backend` method is one Rust command
(`src-tauri/src/commands.rs`), a thin layer over `rt-app`; files, keychain and network stay on
the Rust side.

## Run and build

```sh
npm ci --ignore-scripts
npm run dev -w apps/desktop        # tauri dev: Vite on 127.0.0.1:1420 + the app window
npm run build -w apps/desktop      # tauri build --no-bundle: release binary, frontend embedded
npm run check -w apps/desktop && npm test -w apps/desktop    # types, TauriBackend unit tests
cargo test -p rt-desktop           # Rust side incl. the open/create/send/save flow
```

Linux needs WebKitGTK 4.1 to build (Debian/Ubuntu:
`libwebkit2gtk-4.1-dev librsvg2-dev libxdo-dev build-essential`). Nothing is signed or
notarized; installers (`tauri build` with bundling) come later.

`rt-desktop` is a workspace member (shared lints and lockfile). It is excluded from the wasm32
build and the iOS check (`--exclude rt-desktop`).

## Security

- The webview has one capability (`src-tauri/capabilities/default.json`) that allows exactly
  the app's commands: no file system, shell, dialog or other plugin APIs. The folder picker runs
  on the Rust side (`tauri-plugin-dialog`).
- A content security policy is set in `src-tauri/tauri.conf.json` (no remote scripts, no
  network from the webview except Tauri's IPC).
- Secret values come from the OS keychain (`rt_app::secrets::keychain::KeychainStore`, service
  `RequestTailor`, collection id = canonical folder path) and are added to a send on the Rust
  side; values the webview passes are ignored.

## Files

`src-tauri/src/files.rs` is the only file I/O. Opening a folder reads every file below it as
bytes (collection YAML and body files), skipping folders named `.*` or `node_modules`, nested
collections (folders with their own `opencollection.yml`), files named `.*`, files over 16 MiB
and symbolic links; more than 20 000 files or 256 MiB is rejected. Saving writes each file to a
temporary file next to it and renames it over the target; a change with empty text deletes the
file (a deleted environment). The web app applies the same rules (`apps/web/src/fs/directory.ts`).
