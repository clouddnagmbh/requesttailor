# Plugin API changelog

Every change to the plugin API (`API.md`, `src/api.rs`) is listed here, newest first.
Additive changes keep `API_VERSION`; breaking changes bump it.

## API version 1 (wave 5)

- First public version: manifest in the `rt-plugin` custom section, permissions (`network`,
  `storage`, `variables`), hooks `prepare`, `response`, `render-tab`, `action`, `detect`,
  `import`, host calls `log`, `now`, `http`, `storage-get`, `storage-set`,
  `storage-delete`, views with the blocks `heading`, `text`, `notice`, `key-value`,
  `table`, `code`, `tree`, `form`, `actions`.
- `API.md` links the developer guide `plugins/README.md` (Rust SDK `crates/rt-plugin-sdk`,
  TypeScript types `sdk/typescript`). Documentation only; the interface is unchanged.
