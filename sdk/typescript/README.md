# @requesttailor/plugin-sdk (TypeScript)

TypeScript side of the RequestTailor [plugin API](../../crates/rt-plugin/API.md):

- `src/api.ts`: the types of every message (manifest, calls, replies, views, host calls), a
  mirror of `crates/rt-plugin/src/api.rs`. Fields Rust fills in with a default are optional.
  The request model (`RequestSpec`) and loss reports come from the app's types
  (`apps/ui/src/commands/types.ts`).
- `src/contract.generated.ts`: real JSON from the Rust types, annotated with these types.
  `npm run check -w sdk/typescript` fails when they drift apart. After a change to the API:
  `RT_UPDATE_CONTRACT=1 cargo test -p rt-plugin-sdk --test ts_contract`.
- `src/testing.ts`: a test host on the JavaScript engine's own WebAssembly. `TestPlugin.load`
  checks a module's interface (imports only `rt.host` and the WASI subset, the ABI exports,
  the `rt-plugin` section), then `call()` runs hooks and answers host calls within the
  granted permissions. Works for any plugin `.wasm`, whatever language it was written in.

```sh
npm run check -w sdk/typescript   # types against the Rust JSON
npm test -w sdk/typescript        # builds plugins/ and runs them on Node's WebAssembly
```

**Not yet: compiling TypeScript plugins to WebAssembly.** There is no build path from
TypeScript to a plugin module yet; see "TypeScript" in [plugins/README.md](../../plugins/README.md)
for why. Write plugins in Rust for now.
