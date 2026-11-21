# CLAUDE.md

RequestTailor ist ein API-Client mit Rust-Kern, der nativ (macOS, Windows, Linux), als WebAssembly im Browser, als CLI (`rt`) und als MCP-Server läuft. Dieses Repo ist der **öffentliche Kern unter Apache 2.0**. Es baut allein die vollständige Free-Edition.

## Lizenzgrenze (nicht verhandelbar)

- Alles hier ist Apache 2.0 und kennt **keine Bezahlfunktion**: keine Lizenzprüfung, keine Platzhalter für Pro-Features, kein Login-Dialog. Pro-Code entsteht nur im privaten Repo `requesttailor-pro`.
- Der Kern kennt höchstens die Schnittstelle „Feature freigeschaltet: ja/nein“, die ohne Pro-Modul immer die Free-Antwort gibt.
- Code fließt nur von privat nach öffentlich, nie per Copy-and-paste zurück. Was einmal öffentlich ist, bleibt es.
- Keine Abhängigkeit mit GPL, AGPL, LGPL, SSPL oder anderer Copyleft-Lizenz. `cargo-deny` und der npm-Lizenzcheck erzwingen das; die Allowlist steht einmal in `deny.toml` und gilt für Rust und npm. Eine Lizenz dort aufzunehmen ist eine bewusste Entscheidung mit Begründung im Commit.
- Name und Logo „RequestTailor“ stehen nicht unter Apache.
- Jede Quelldatei beginnt mit dem Lizenzkopf:
  ```
  // SPDX-License-Identifier: Apache-2.0
  // Copyright 2026 The RequestTailor Authors
  ```
- Commits tragen ein DCO-Sign-off (`git commit -s`), die CI prüft das. Details für Beitragende: `CONTRIBUTING.md`; Markenregeln: `TRADEMARKS.md`.

## Arbeitsregeln

1. **Schnittstellen zuerst.** Jede Welle beginnt mit einem Schnittstellen-Commit (Typen, Traits, Dateistrukturen). Erst danach wird parallel gearbeitet.
2. **Stränge sind unabhängig.** Ein Strang ist ein eigener Branch oder Worktree. Stränge einer Welle fassen keine gemeinsamen Dateien an, außer den Schnittstellen aus Regel 1.
3. **Tests vor Code.** Zuerst die Abnahmetests aus dem Gate, dann die Umsetzung. Die Tests entscheiden, nicht der Eindruck.
4. **Gate heißt vorführen.** Fertig ist eine Welle, wenn die CI grün ist und die Abnahme einmal von Hand vorgeführt wurde.
5. **Nichts mitnehmen.** Was das Gate nicht fordert, wandert in eine spätere Welle oder ein Issue.
6. **Lizenzgrenze beachten.** Siehe oben.

Am Ende jedes Auftrags melden: welche Abnahmepunkte grün sind, welche offen, und was bewusst ausgelassen wurde.

## Produktprinzipien

Sie entscheiden jede Feature-Frage.

1. **Dateien sind die Wahrheit.** Ein Request ist eine lesbare Textdatei (OpenCollection, YAML). Diffs funktionieren ohne Spezialwerkzeug. Die App ist nur eine Sicht auf den Ordner.
2. **Kein Account, nie.** Download, öffnen, senden. Offline voll funktionsfähig. Free baut keine Verbindung zu CloudDNA-Servern auf.
3. **Secrets bleiben lokal.** Werte im OS-Schlüsselbund oder als Vault-Verweis; im Repo steht nur der Verweis.
4. **Performance ist ein Feature mit Budget.** Siehe Budgets unten; sie sind Release-Gates.
5. **Parität über alle Oberflächen.** Desktop, Web, CLI und MCP nutzen denselben Kern. Was in der App grün ist, ist in der Pipeline grün.
6. **Der Kern bleibt schlank.** Nischenprotokolle und Domänenwissen (auch SAP/OData) kommen als Plugins auf der öffentlichen Plugin-API. Der Kern enthält kein SAP-Wissen.
7. **Fairness-Garantie.** Was einmal kostenlos ist, wandert nie hinter eine Paywall.

## Performance-Budgets

| Messgröße | Budget |
| --- | --- |
| Kaltstart der App | unter 500 ms |
| RAM im Leerlauf | unter 150 MB |
| 100-MB-Antwort | rendert und scrollt ohne Ruckeln |
| Bedienung | Request senden in unter drei Tastendrücken ab App-Start |

Die Budgets werden als automatisierte Tests gemessen; eine Überschreitung bricht den Build. Die Schwellwerte stehen in `perf/budgets.toml`. Bis Welle 3 misst noch nichts, jede Messgröße meldet „not measured“. In Welle 4 schreibt die CI `perf/results.json` und ruft die Prüfung mit `--require-all` auf; dann bricht auch eine fehlende Messung den Build. Der Wert 16,7 ms (p95-Framezeit für „ohne Ruckeln“) ist ein Vorschlag und in Welle 4 zu bestätigen.

## Technische Leitplanken

| Bereich | Regel |
| --- | --- |
| Build-Matrix | Jeder Commit baut macOS, Windows, Linux, wasm32 und prüft die iOS-Kompilierung |
| Parität | Eine Konformitäts-Suite läuft identisch gegen nativen und WASM-Kern |
| Format | Nur `rt-format` kennt OpenCollection. Round-Trip: lesen und schreiben ergibt byte-identische Dateien |
| Importer | Jeder Import liefert einen Verlustbericht |
| Plugins | Ausschließlich WASM, kein nativer Code (iOS erlaubt kein Nachladen) |
| Sandbox | Skripte und Plugins haben keinen Datei- und Netzzugriff außer über die Request-API |
| Agenten | Schreibende Requests gegen Produktionsumgebungen nur mit Freigabe; lokales Audit-Log; Agenten sehen nie Secret-Werte |
| Datenschutz | Keine Telemetrie ohne Zustimmung |
| Abhängigkeiten | Jede neue Abhängigkeit muss auf allen Zielen bauen, auch wasm32 und iOS |

## Struktur

```
crates/rt-core     HTTP-Engine, Variablen, Auth, Runner
crates/rt-format   OpenCollection lesen und schreiben, Importer
crates/rt-script   Skript-Sandbox (QuickJS)
crates/rt-plugin   Plugin-Host (WASM) und SDK
crates/rt-cli      CLI `rt`, Begleitdienst, MCP-Server
crates/rt-wasm     Kern als WebAssembly
crates/rt-testserver  lokaler HTTP-Testserver (nur Testinfrastruktur, nativ)
testdata/collections/bruno  Korpus: 86 Bruno-Collections (MIT), unverändert, für Round-Trip-Tests
apps/ui            gemeinsame Oberfläche (TypeScript)
apps/desktop       Tauri-2-Hülle
apps/web           Web-App
plugins/odata      OData V2/V4, CSRF, $batch
ci/canary          absichtlich verbotene Abhängigkeiten; die Lizenz-Gates müssen sie ablehnen
scripts            Prüfskripte ohne Abhängigkeiten (Lizenzen, Köpfe, DCO, Performance)
perf/budgets.toml  Performance-Budgets, einzige Quelle für die Schwellwerte
```

## Kern-Schnittstellen

Stand Welle 2. Änderungen daran sind Schnittstellen-Änderungen (Arbeitsregel 1).

- `rt_core::model`: internes Request-Modell (Collection, Folder, RequestSpec, Environment). Formatneutral.
- `rt_core::transport::Transport`: Grenze zum Netz. Nativ eigener HTTP-Stack, im Browser `fetch`. Antwort-Body als Stream.
- `rt_core::vars::Scope`: geschichtete Variablen mit Herkunft (`VarSource`), Rangfolge wie Bruno: Global < Collection < Umgebung < Ordner < Request < Laufzeit.
- `rt_core::prepare::prepare`: RequestContext → fertiger `HttpRequest` (Variablen, Vererbung, Auth, Body).
- `rt_core::send::send`: der eine Einstieg für „Request senden“; `SendReport` ist zugleich die JSON-Ausgabe von `rt send` und das Ergebnis von `rt-wasm`.
- `rt_format::{FileSet, read_collection, CollectionDoc}`: Dateien ↔ Modell, arbeitet im Speicher, damit es auch im Browser läuft.
- Zeit immer über `web_time::Instant`, nie `std::time::Instant` (panikt in wasm32).
- OpenCollection erlaubt keine unbekannten Felder (`additionalProperties: false`), außer `extensions` in `opencollection.yml`. Eigene Daten gehören dorthin oder in Begleitdateien.

## Befehle

Toolchain ist in `rust-toolchain.toml` gepinnt; Upgrades sind ein eigener Commit. Einmalig: `rustup target add wasm32-unknown-unknown aarch64-apple-ios`.

```sh
cargo build --workspace --all-targets --locked
cargo test --workspace --locked
cargo build --workspace --exclude rt-cli --exclude rt-testserver --target wasm32-unknown-unknown --locked
cargo check --workspace --all-targets --target aarch64-apple-ios --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo deny --all-features check
npm ci --ignore-scripts && npm run check:licenses && npm run test:scripts
node scripts/check-license-headers.mjs
node scripts/check-dco.mjs origin/main..HEAD
node scripts/check-perf-budgets.mjs
gitleaks git --no-banner --redact --config .gitleaks.toml .
ci/canary/run.sh
cargo test -p rt-cli --test conformance --test cli --locked
scripts/conformance-wasm.sh
```

Konformitäts-Suite (`testdata/conformance`, Fälle in `cases.json`, Vergleich in `harness.rs`): nativ über die beiden `rt-cli`-Tests, im Browser über `scripts/conformance-wasm.sh` (startet `rt-testserver` auf Port 47811, Headless Chrome). Dafür einmalig `cargo install wasm-bindgen-cli --version <wasm-bindgen aus Cargo.lock> --locked` und ein zu Chrome passender `chromedriver` in `CHROMEDRIVER` oder im `PATH` (`npx @puppeteer/browsers install chromedriver@<Chrome-Version>`). Weitere Argumente gehen an `cargo test`, z. B. `scripts/conformance-wasm.sh -- conformance_browser`. `rt` selbst: `cargo run -p rt-cli -- send <request-file> [--env NAME] [--var NAME=VALUE]...`, Details in `crates/rt-cli/README.md`.

CI (`.github/workflows/ci.yml`) führt genau diese Befehle aus. Der Job `ci-ok` ist der einzige Pflicht-Status für Branch-Schutz. Warnungen sind in der CI Fehler (`RUSTFLAGS=-D warnings`).

## Konventionen

- Code, Kommentare, Commit-Messages und öffentliche Doku auf Englisch (das Repo wird öffentlich); diese Datei ist auf Deutsch.
- `unsafe` ist workspace-weit verboten (`deny`); eine Ausnahme braucht `#[allow(unsafe_code)]` mit Begründung direkt daneben.
- Neue Crates erben `version`, `edition`, `license` und `[lints]` aus dem Workspace.
