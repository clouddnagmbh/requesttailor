# Script shim coverage

RequestTailor runs one script API (`rt`, see [`api.d.ts`](api.d.ts)) on every surface:
desktop, web, CLI and the collection runner. Bruno, Postman and Insomnia scripts run on top
of it through shims, small JavaScript files that define `bru`, `req`, `res`, `pm`,
`postman`, `insomnia`, the legacy Postman globals and `expect`. This page lists what the
shims cover and where they behave differently from the original tools.

- Shims: [`js/shims/`](js/shims), loaded in file-name order before the first script of a
  phase (`00-expect.js`, `01-common.js`, `10-bru.js`, `20-pm.js`).
- Modules for `require`: [`js/modules/`](js/modules).
- Machine-readable list (one line per API, read by the importers and the tests):
  [`crates/rt-format/src/import/shim-apis.txt`](../rt-format/src/import/shim-apis.txt).
- Tests: `npm run test:shims` (Node, a fake engine), and the case files in
  [`tests/cases/shims`](tests/cases/shims) and [`tests/cases/modules`](tests/cases/modules),
  which run in the real engine, natively and as WebAssembly.

## Status

| Status | Meaning |
| --- | --- |
| supported | Behaves like the original tool for the scripts in our corpus and the documented API. |
| partial | Works, with the limits in the note. |
| unsupported | Not available. Calling it throws `X is not supported in RequestTailor (…); see SHIM-COVERAGE.md`, so a test fails with a clear message instead of an `undefined is not a function`. |

When a collection is imported, its scripts are kept as they are. The loss report lists every
unsupported or partial API a script uses, with the request or folder it is in. Nothing is
rewritten.

## The APIs the corpus uses

The shims were written in the order of this table. It counts every API use in the scripts of
the test corpora (`testdata/collections/bruno`, `testdata/import/{postman,insomnia,http}`).
A use is one member path (`pm.environment.get`), one call of a global (`test()`), or one
assertion word after `expect(…)` (`expect(x).to.not.equal(1)` counts `not` and `equal`).
Insomnia's API is Postman's, so `insomnia.*` paths without an entry of their own use the
`pm.*` entry. JetBrains HTTP client handler scripts are counted, but the `.http` importer
does not import them.

Regenerate the table with `node scripts/shim-frequency.mjs --update-doc`; a test fails when
it is out of date.

<!-- frequency:start (node scripts/shim-frequency.mjs --update-doc) -->
Scripts scanned: Bruno 66 (plus 28 gRPC scripts, not run), Postman 55, Insomnia 52, JetBrains HTTP 27.

| Dialect | API | Uses | Files | Status | Note |
| --- | --- | ---: | ---: | --- | --- |
| bruno | `expect().equal` | 214 | 28 | supported |  |
| postman | `pm.test` | 89 | 5 | supported | `done` callbacks and async functions are awaited |
| bruno | `test()` | 71 | 32 | supported | async functions are awaited |
| insomnia | `insomnia.environment.set` | 70 | 3 | supported | environment scope, in memory (files are never written) |
| postman | `pm.expect` | 68 | 4 | supported | see expect() |
| bruno | `bru.getEnvVar` | 45 | 3 | supported | environment scope; see "Variable types" |
| postman | `pm.collectionVariables.get` | 44 | 1 | supported | collection scope |
| postman | `pm.response.json` | 42 | 4 | supported |  |
| jetbrains | `client` | 35 | 9 | unsupported | JetBrains handler scripts are not imported |
| jetbrains | `response` | 32 | 9 | unsupported | JetBrains handler scripts are not imported |
| bruno | `bru.setVar` | 30 | 18 | supported | runtime scope |
| postman | `pm.response.to.status` | 26 | 3 | supported | `pm.response.to.have.status(code or reason)` |
| bruno | `bru.getGlobalEnvVar` | 25 | 2 | supported | global scope |
| bruno | `bru.setEnvVar` | 25 | 9 | supported | environment scope, in memory; `persist` is ignored (files are never written) |
| postman | `expect().property` | 25 | 2 | supported |  |
| bruno | `expect().deep` | 24 | 1 | supported |  |
| postman | `expect().equal` | 19 | 3 | supported |  |
| bruno | `setTimeout()` | 17 | 8 | supported | runs on rt.sleep; the wait counts against the time limit |
| insomnia | `insomnia.parentFolders` | 15 | 1 | unsupported |  |
| postman | `pm.request.body` | 15 | 1 | supported | raw text bodies; update() takes raw, urlencoded and graphql |
| bruno | `bru.getVar` | 14 | 8 | supported | runtime variables; see "Variable types" |
| insomnia | `insomnia.expect` | 14 | 2 | supported | see expect() |
| postman | `expect().jsonSchema` | 14 | 1 | partial | JSON Schema draft 4 to 2020-12 without `format` and remote `$ref` |
| postman | `pm.collectionVariables.set` | 14 | 1 | supported | collection scope, in memory |
| bruno | `bru.getRequestVar` | 13 | 1 | partial | the resolved value (a runtime variable can shadow the request's) |
| postman | `pm.response.to.jsonSchema` | 13 | 1 | partial | no `format`, no remote `$ref` |
| bruno | `bru.setGlobalEnvVar` | 12 | 3 | supported | global scope, in memory |
| postman | `pm.collectionVariables.unset` | 11 | 1 | supported |  |
| bruno | `req.setHeader` | 10 | 5 | supported |  |
| bruno | `res.getBody` | 10 | 7 | supported |  |
| postman | `expect().eql` | 9 | 1 | supported |  |
| bruno | `bru.getCollectionVar` | 8 | 2 | supported | collection scope |
| bruno | `res.getStatus` | 8 | 8 | supported |  |
| bruno | `bru.getFolderVar` | 6 | 1 | partial | the resolved value (a closer layer can shadow the folder's) |
| bruno | `req.deleteHeader` | 6 | 6 | supported |  |
| bruno | `res.status` | 6 | 6 | supported |  |
| insomnia | `insomnia.request.certificate` | 6 | 1 | unsupported |  |
| bruno | `__dirname` | 5 | 4 | unsupported | scripts have no file system access |
| bruno | `req.deleteHeaders` | 5 | 5 | supported |  |
| bruno | `res.body` | 5 | 5 | supported | parsed JSON when the body is JSON, the text otherwise |
| insomnia | `insomnia.baseEnvironment.set` | 5 | 1 | supported | the collection scope, in memory |
| insomnia | `insomnia.variables.get` | 5 | 2 | supported | the resolved value (all layers) |
| bruno | `__filename` | 4 | 4 | unsupported | scripts have no file system access |
| bruno | `expect().contain` | 4 | 1 | supported |  |
| bruno | `expect().not` | 4 | 1 | supported |  |
| insomnia | `expect().keys` | 4 | 1 | supported |  |
| insomnia | `insomnia.sendRequest` | 4 | 1 | partial | URL string or request object (raw, urlencoded, graphql bodies; no formdata or file); callback or promise |
| insomnia | `insomnia.test` | 4 | 3 | supported | `done` callbacks and async functions are awaited |
| insomnia | `insomnia.vault` | 4 | 1 | unsupported |  |
| insomnia | `setTimeout()` | 4 | 1 | supported | runs on rt.sleep; the wait counts against the time limit |
| postman | `_` | 4 | 1 | partial | the lodash subset (see require('lodash')) |
| bruno | `bru.sendRequest` | 3 | 3 | supported | axios-style options and response; every status resolves; callback or promise |
| bruno | `expect().above` | 3 | 1 | supported |  |
| insomnia | `insomnia.variables.set` | 3 | 1 | supported | runtime scope (Postman's local variables) |
| insomnia | `require('uuid')` | 3 | 1 | partial | v4, NIL, MAX, validate, version, parse, stringify; no v1, v3, v5, v6, v7 |
| postman | `pm.globals.set` | 3 | 1 | supported | global scope, in memory |
| postman | `pm.response.headers` | 3 | 2 | supported | HeaderList, read-only |
| postman | `pm.response.to.header` | 3 | 2 | supported |  |
| bruno | `bru.isSafeMode` | 2 | 1 | supported | always true (RequestTailor has one sandbox, like Bruno's safe mode) |
| bruno | `bru.runner.skipRequest` | 2 | 2 | supported |  |
| bruno | `bru.setCollectionVar` | 2 | 1 | supported | collection scope, in memory |
| bruno | `expect().a` | 2 | 2 | supported |  |
| bruno | `expect().eql` | 2 | 2 | supported |  |
| insomnia | `expect().a` | 2 | 1 | supported |  |
| insomnia | `expect().all` | 2 | 1 | supported |  |
| insomnia | `expect().any` | 2 | 1 | supported |  |
| insomnia | `expect().eql` | 2 | 1 | supported |  |
| insomnia | `expect().oneOf` | 2 | 1 | supported |  |
| insomnia | `insomnia.environment.unset` | 2 | 1 | supported |  |
| insomnia | `insomnia.globals.set` | 2 | 1 | supported | global scope, in memory |
| insomnia | `insomnia.request.auth` | 2 | 1 | unsupported | auth is configured on the request, not in scripts |
| insomnia | `insomnia.request.headers` | 2 | 1 | supported | HeaderList: get, has, add, upsert, remove, each, toObject, … |
| insomnia | `insomnia.request.proxy` | 2 | 1 | unsupported |  |
| insomnia | `require('chai')` | 2 | 1 | partial | expect and the common assert functions; no `should` or plugins |
| insomnia | `require('cheerio')` | 2 | 1 | unsupported | not a built-in module of RequestTailor |
| insomnia | `require('crypto-js')` | 2 | 1 | partial | hashes (MD5, SHA-1, SHA-2), HMAC, PBKDF2, encoders; no ciphers (AES, DES, …), SHA-3 or RIPEMD-160 |
| insomnia | `require('csv-parse/lib/sync')` | 2 | 1 | unsupported | not a built-in module of RequestTailor |
| insomnia | `require('insomnia-collection')` | 2 | 1 | unsupported | not a built-in module of RequestTailor |
| insomnia | `require('lodash')` | 2 | 1 | partial | about 130 common functions; no chaining (`_(x).map()`) or `_.template` |
| insomnia | `require('moment')` | 2 | 1 | partial | English, local and UTC; no time zones, locales, durations or relative time |
| insomnia | `require('tv4')` | 2 | 1 | partial | validate, validateResult, validateMultiple; no `format`, addSchema or remote `$ref` |
| insomnia | `require('url')` | 2 | 1 | unsupported | not a built-in module of RequestTailor |
| insomnia | `require('xml2js')` | 2 | 1 | unsupported | not a built-in module of RequestTailor |
| postman | `expect().include` | 2 | 1 | supported |  |
| postman | `expect().true` | 2 | 2 | supported |  |
| postman | `pm.environment.get` | 2 | 1 | supported | environment scope; see "Variable types" |
| postman | `pm.environment.set` | 2 | 1 | supported | environment scope, in memory (files are never written) |
| postman | `pm.globals.get` | 2 | 1 | supported | global scope |
| postman | `pm.variables.get` | 2 | 1 | supported | the resolved value (all layers) |
| bruno | `bru.deleteEnvVar` | 1 | 1 | supported |  |
| bruno | `bru.runRequest` | 1 | 1 | unsupported | use the collection runner |
| bruno | `bru.sleep` | 1 | 1 | supported | counts against the time limit |
| bruno | `expect().greaterThan` | 1 | 1 | supported |  |
| bruno | `req.onFail` | 1 | 1 | unsupported |  |
| bruno | `require('fs')` | 1 | 1 | unsupported | not a built-in module of RequestTailor |
| bruno | `require('path')` | 1 | 1 | unsupported | not a built-in module of RequestTailor |
| insomnia | `expect().below` | 1 | 1 | supported |  |
| insomnia | `expect().exist` | 1 | 1 | supported |  |
| insomnia | `expect().include` | 1 | 1 | supported |  |
| insomnia | `expect().lengthOf` | 1 | 1 | supported |  |
| insomnia | `expect().not` | 1 | 1 | supported |  |
| insomnia | `expect().property` | 1 | 1 | supported |  |
| insomnia | `fetch()` | 1 | 1 | unsupported | use pm.sendRequest, bru.sendRequest or rt.sendRequest |
| insomnia | `insomnia.baseGlobals.get` | 1 | 1 | supported | the global scope |
| insomnia | `insomnia.baseGlobals.set` | 1 | 1 | supported | the global scope, in memory |
| insomnia | `insomnia.clientCertificates` | 1 | 1 | unsupported |  |
| insomnia | `insomnia.collectionVariables.set` | 1 | 1 | supported | collection scope, in memory |
| insomnia | `insomnia.globals.get` | 1 | 1 | supported | global scope |
| insomnia | `insomnia.request.body.update` | 1 | 1 | supported | raw, urlencoded and graphql |
| insomnia | `insomnia.request.method` | 1 | 1 | supported |  |
| insomnia | `insomnia.request.url` | 1 | 1 | supported | Url object: toString, getHost, getPath, getQueryString, query.add/upsert/remove, … |
| insomnia | `insomnia.response.json` | 1 | 1 | supported |  |
| insomnia | `require('ajv')` | 1 | 1 | partial | compile, validate, errorsText; errors are `{ message }`; no `format` or custom keywords |
| insomnia | `require('assert')` | 1 | 1 | unsupported | not a built-in module of RequestTailor |
| insomnia | `require('atob')` | 1 | 1 | supported |  |
| insomnia | `require('btoa')` | 1 | 1 | supported |  |
| insomnia | `require('buffer')` | 1 | 1 | unsupported | not a built-in module of RequestTailor |
| insomnia | `require('events')` | 1 | 1 | unsupported | not a built-in module of RequestTailor |
| insomnia | `require('path')` | 1 | 1 | unsupported | not a built-in module of RequestTailor |
| insomnia | `require('punycode')` | 1 | 1 | unsupported | not a built-in module of RequestTailor |
| insomnia | `require('querystring')` | 1 | 1 | unsupported | not a built-in module of RequestTailor |
| insomnia | `require('stream')` | 1 | 1 | unsupported | not a built-in module of RequestTailor |
| insomnia | `require('string_decoder')` | 1 | 1 | unsupported | not a built-in module of RequestTailor |
| insomnia | `require('timers')` | 1 | 1 | unsupported | not a built-in module of RequestTailor |
| insomnia | `require('util')` | 1 | 1 | unsupported | not a built-in module of RequestTailor |
| insomnia | `URL()` | 1 | 1 | unsupported | no URL class in the sandbox |
| jetbrains | `request.variables` | 1 | 1 | unsupported | JetBrains handler scripts are not imported |
| postman | `expect().gt` | 1 | 1 | supported |  |
| postman | `expect().not` | 1 | 1 | supported |  |
| postman | `pm.response.to.ok` | 1 | 1 | supported |  |
| postman | `pm.variables.set` | 1 | 1 | supported | runtime scope (Postman's local variables) |
<!-- frequency:end -->

APIs the corpus does not use are listed with their status in
[`shim-apis.txt`](../rt-format/src/import/shim-apis.txt).

## Variable scopes

Every write goes into one layer of RequestTailor's variables, in memory. Files are never
written: a value set by a script lives for the send, for the session in the app, or for the
run in the runner. Reads return the value as the request would resolve it, unless the script
set the variable in the same phase (then it reads back what it set).

| Script API | RequestTailor layer |
| --- | --- |
| `bru.setVar`, `bru.getVar` | runtime |
| `bru.setEnvVar`, `bru.getEnvVar` | environment |
| `bru.setGlobalEnvVar`, `bru.getGlobalEnvVar` | global |
| `bru.setCollectionVar`, `bru.getCollectionVar` | collection |
| `bru.getFolderVar`, `bru.getRequestVar` | the resolved value (folder and request variables are read-only) |
| `pm.variables.set` | runtime (Postman's local variables) |
| `pm.variables.get` | the resolved value (all layers) |
| `pm.environment.*`, `postman.*EnvironmentVariable` | environment |
| `pm.collectionVariables.*` | collection |
| `pm.globals.*`, `postman.*GlobalVariable` | global |
| `pm.iterationData.*` | the runner's data row (`rt.info.iterationData`); writes stay in the script |
| `insomnia.environment.*` | environment |
| `insomnia.baseEnvironment.*` | collection |
| `insomnia.globals.*`, `insomnia.baseGlobals.*` | global |
| `insomnia.collectionVariables.*`, `insomnia.variables.*` | as `pm.collectionVariables`, `pm.variables` |

Layer precedence is the one of the request: global < collection < environment < folder <
request < runtime. So `pm.environment.get("x")` returns the request's value of `x`, which a
runtime variable of the same name shadows; the engine has no read of a single layer.

## Differences from the original tools

- **Variable types.** Variables are strings. A number, boolean or object set by a script is
  stored as JSON. In the same phase the script reads back the value it set, type and all
  (`bru.setVar("n", 1); bru.getVar("n") === 1`). In a later phase or request it reads the
  string (`"1"`); Bruno would keep the number there. Compare with `Number(…)` or `==` when it
  matters.
- **Persistence.** `bru.setEnvVar(name, value, { persist: true })` and Postman's
  environment writes do not change files (principle: files are the truth, the app is a
  view). The value lasts for the session or run.
- **Timers.** `setTimeout`, `setInterval` and `bru.sleep` run on `rt.sleep`. The wait counts
  against the time limit of the phase (default 5 s).
- **`sendRequest`.** `pm.sendRequest`, `bru.sendRequest` and `insomnia.sendRequest` use the
  same transport as the request, without scripts, plugins or inherited auth. Bodies are raw
  text, `urlencoded` or `graphql`; `formdata` and `file` are not supported. Bruno's
  `sendRequest` resolves for every status, like Bruno with `validateStatus` off.
- **The request after the response.** In post-response scripts and tests, `req`,
  `pm.request` and `insomnia.request` show what was sent and are read-only; a write throws.
- **`res.setBody`** changes what later code of the same phase reads, not the response shown
  in the app.
- **No Node.js.** There is no `fs`, `path`, `Buffer`, `process`, `URL`, `fetch` or other
  Node.js or browser API, and `require` loads only the modules below. Scripts have no file
  access, and network access only through `sendRequest`.
- **gRPC and WebSocket scripts** of Bruno (`bru.grpc`, `onMessage`) do not run; RequestTailor
  has no gRPC or WebSocket requests.
- **Safe mode.** `bru.isSafeMode()` returns `true`: RequestTailor has one sandbox for every
  script, like Bruno's safe mode.
- **Messages.** `expect` and the response assertions (`pm.response.to.have.status(200)`)
  fail with chai's and Postman's messages, such as `expected 404 to equal 200` or
  `expected response to have status code 200 but got 404`.
- **JSON Schema** (`pm.response.to.have.jsonSchema`, `expect(x).to.have.jsonSchema`,
  `tv4`, `ajv`): drafts 4 to 2020-12 with local `$ref`; `format` is not checked and remote
  `$ref` is refused. Errors read like Ajv's: `data.id should be integer`.

## Modules

`require(name)` returns one of these modules. They are written for RequestTailor (Apache
2.0, no third-party code) and cover what the corpus uses; anything else throws
`Cannot find module 'name'`.

| Module | Status | Contents |
| --- | --- | --- |
| `crypto-js` | partial | `MD5`, `SHA1`, `SHA224`, `SHA256`, `SHA384`, `SHA512`, `HmacMD5` … `HmacSHA512`, `PBKDF2`, `algo.*.create()` with `update`/`finalize`, `algo.HMAC`, `lib.WordArray`, encoders `Hex`, `Latin1`, `Utf8`, `Utf16`, `Utf16LE`, `Utf16BE`, `Base64`, `Base64url`. No ciphers (`AES`, `DES`, `TripleDES`, `Rabbit`, `RC4`), `SHA3` or `RIPEMD160`: they throw. |
| `lodash` | partial | About 130 functions: collections (`map`, `filter`, `find`, `groupBy`, `sortBy`, …), arrays, objects (`get`, `set`, `pick`, `merge`, `cloneDeep`, `isEqual`, …), strings (`camelCase`, `startCase`, `trim`, …), `random`, `times`, `uniqueId`. Iteratee shorthands work. No chaining (`_(x).map()`), `_.template` or `_.debounce`. |
| `moment` | partial | Parsing (ISO 8601, formats, Unix), `format` tokens, `add`/`subtract`, `startOf`/`endOf`, `diff`, comparisons, `utc`/`local`, getters and setters. English only; no time zones, durations or relative time (`fromNow`). |
| `uuid` | partial | `v4`, `NIL`, `MAX`, `validate`, `version`, `parse`, `stringify`. No `v1`, `v3`, `v5`, `v6`, `v7`. |
| `chai` | partial | `expect` (as above) and the common `assert` functions. No `should`, no plugins (`chai.use`). |
| `tv4` | partial | `validate`, `validateResult`, `validateMultiple`, `error`. No `addSchema`, no `format`. |
| `ajv` | partial | `new Ajv().compile(schema)`, `validate`, `errors`, `errorsText`. No `format`, no custom keywords. |
| `atob`, `btoa` | supported | Also globals. |

In Postman scripts, `_`, `CryptoJS` and `tv4` are also globals, as in Postman's sandbox.

## Reporting a gap

If an imported script uses an API the shims do not support, or a supported one behaves
differently from the original tool:

1. Check the loss report of the import and the table above.
2. Open an issue with the script (or the smallest part of it that shows the problem), the
   tool it comes from, what it does there, and what RequestTailor does.
3. Contributions are welcome: a shim change comes with a case file in
   `tests/cases/shims` (it runs natively and in WebAssembly), an entry in
   `crates/rt-format/src/import/shim-apis.txt`, and the table above regenerated.
