# CLAUDE.md

RequestTailor ist ein API-Client mit Rust-Kern, der nativ (macOS, Windows, Linux), als WebAssembly im Browser, als CLI (`rt`) und als MCP-Server läuft. Dieses Repo ist der **öffentliche Kern unter Apache 2.0**. Es baut allein die vollständige Free-Edition.

## Lizenzgrenze (nicht verhandelbar)

- Alles hier ist Apache 2.0 und kennt **keine Bezahlfunktion**: keine Lizenzprüfung, keine Platzhalter für Pro-Features, kein Login-Dialog. Pro-Code entsteht nur im privaten Repo `requesttailor-pro`.
- Der Kern kennt höchstens die Schnittstelle „Feature freigeschaltet: ja/nein“, die ohne Pro-Modul immer die Free-Antwort gibt.
- Code fließt nur von privat nach öffentlich, nie per Copy-and-paste zurück. Was einmal öffentlich ist, bleibt es.
- Keine Abhängigkeit mit GPL, AGPL, LGPL, SSPL oder anderer Copyleft-Lizenz. `cargo-deny` und der npm-Lizenzcheck erzwingen das; die Allowlist steht einmal in `deny.toml` und gilt für Rust und npm. Eine Lizenz dort aufzunehmen ist eine bewusste Entscheidung mit Begründung im Commit. Einzelne npm-Pakete mit anderer Lizenz stehen mit Begründung in `scripts/npm-license-exceptions.json` (derzeit nur `lightningcss`, MPL-2.0, Build-Werkzeug von Vite 8); `devOnly`-Ausnahmen dürfen nie ausgelieferter Code sein. Crate-Ausnahmen stehen als `[[licenses.exceptions]]` in `deny.toml` (derzeit `webpki-root-certs` und fünf MPL-2.0-Crates von Tauri 2). Neue Ausnahmen entscheidet der Projektverantwortliche, kein Agent.
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

Die Budgets werden als automatisierte Tests gemessen; eine Überschreitung bricht den Build. Die Schwellwerte stehen in `perf/budgets.toml` (dort auch `send_keystrokes`, höchstens 2). Seit Welle 4 misst `node scripts/perf/run.mjs` auf dem jeweiligen Betriebssystem und schreibt `perf/results.json`; die CI misst auf macOS, Linux und Windows, nimmt den schlechtesten Wert und prüft mit `--require-all`, also bricht auch eine fehlende Messung den Build. Definitionen in `perf/README.md`: Kaltstart = Median vom Prozessstart bis zum ersten bedienbaren Frame (Release-Build); RAM = App plus Webview-Prozesse in der Zahl des Betriebssystems (macOS footprint, Linux PSS, Windows private working set), nicht die RSS-Summe; „ohne Ruckeln“ = p95-Abstand der Animation-Frames höchstens 20 ms (ein verlorener Frame kostet mindestens 33 ms; 16,7 ms würde glatte Läufe zufällig reißen).

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
crates/rt-format   OpenCollection lesen und schreiben, Importer (`import`, ein Modul pro Quelle, Verlustbericht)
crates/rt-script   Skript-Sandbox (QuickJS)
crates/rt-plugin   Plugin-Host (WASM) und SDK
crates/rt-cli      CLI `rt`, Begleitdienst, MCP-Server
crates/rt-wasm     Kern als WebAssembly
crates/rt-app      Befehlsschicht der App (Rust-Seite): Workspace, Antwortspeicher, Secrets, Git-Typen, Import, Historie, Suche
crates/rt-git      Git nativ (gitoxide), nur Desktop/CLI/iOS, nicht wasm32
crates/rt-testserver  lokaler HTTP-Testserver (nur Testinfrastruktur, nativ)
testdata/collections/bruno  Korpus: 86 Bruno-Collections (MIT), unverändert, für Round-Trip-Tests
testdata/import    Import-Korpus pro Quelle, frei lizenziert, Herkunft und Lizenz je Datei in SOURCES.md
apps/ui            gemeinsame Oberfläche (Svelte 5, TypeScript, CodeMirror 6)
apps/desktop       Tauri-2-Hülle; Crate `rt-desktop` in `apps/desktop/src-tauri` (nur Desktop, nicht wasm32/iOS)
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

### App (ab Welle 3)

- Die Oberfläche (`apps/ui`) ruft den Kern **nur** über `Backend` in `apps/ui/src/commands/backend.ts` auf. Drei Umsetzungen: Tauri (`apps/desktop`, über `rt-app`), Web (`apps/web`, über `rt-wasm` → `rt-app`), Mock (`apps/ui/src/commands/mock.ts`, für UI-Tests mit `?backend=mock`). Keine Weiche nach Plattform im Komponentencode.
- `apps/ui/src/commands/types.ts` spiegelt die serde-JSON der Rust-Typen. Der Vertragstest `cargo test -p rt-app --test contract` erzeugt `contract.generated.ts`; `npm run check -w apps/ui` scheitert, sobald beide auseinanderlaufen. Nach Änderungen an Rust-Typen: `RT_UPDATE_CONTRACT=1 cargo test -p rt-app --test contract`.
- Gemeinsamer Zustand: `apps/ui/src/state/app.svelte.ts` (`getApp()`).
- Secret-Werte stehen nie in Collection-Dateien: nativ im OS-Schlüsselbund, im Web verschlüsselt im Browser-Speicher; sie gehen nur beim Senden in den Umgebungs-Layer (`SendInput.secrets`).
- Große Antworten bleiben auf der Rust-Seite (`ResponseStore`); die UI holt nur sichtbare Zeilen.
- Hüllen: Desktop über Tauri-Befehle (`apps/desktop/src-tauri`, Ordner-Zugriff nur auf der Rust-Seite, Schlüsselbund), Web über `rt-wasm` (`Workspace`-Klasse), File System Access und verschlüsselten Browser-Speicher. Beide lesen Ordner nach denselben Regeln und speichern über `prepareSave`/`commit`; ein `FileChange` mit `deleted: true` heißt „Datei löschen“.
- Welle 4: `Backend` hat `git` (`GitApi`), `imports` (`ImportApi`), `history` (`HistoryApi`), `search`, `recentCollections`/`openRecent`. Typen in `rt_app::{git, import, history, search}` und `rt_format::import`. Jede Hülle setzt sie in eigenen Dateien um (Desktop: `src-tauri/src/{git,import,history,search,recent}.rs` und `src/{git,import,history}.ts`; Web: `src/{git/,import.ts,history.ts,recent.ts}`; Mock: `commands/mock-*.ts`). Neue Tauri-Befehle stehen dreimal: `build.rs`, `capabilities/default.json`, `generate_handler!` in `lib.rs` (der Desktop-Test prüft das).
- Git ohne Staging-Bereich: `commit(message, paths)`. Was Arbeitsdateien ändert (`checkout`, `pull`, `resolve`), verweigert bei ungespeicherten Änderungen; danach ruft die UI `AppState.reload()`. Nativ `rt-git` (gitoxide), im Web isomorphic-git, Netzwerk dort über den Begleitdienst.
- Historie speichert keine Bodies und keine Header-Werte; die URL ist die maskierte aus dem Report.
- Begleitdienst `rt agent` (`crates/rt-cli/src/agent`, Protokoll in `rt_core::transport::companion`): nur Loopback, Host- und Origin-Prüfung, Kopplung per Einmalcode, Token nur im Speicher. Sicherheitsmodell in `crates/rt-cli/README.md`.

## Befehle

Toolchain ist in `rust-toolchain.toml` gepinnt; Upgrades sind ein eigener Commit. Einmalig: `rustup target add wasm32-unknown-unknown aarch64-apple-ios`. Linux braucht für jeden Workspace-Build `libwebkit2gtk-4.1-dev librsvg2-dev libxdo-dev build-essential` (Tauri).

```sh
cargo build --workspace --all-targets --locked
cargo test --workspace --locked
cargo build --workspace --exclude rt-cli --exclude rt-testserver --exclude rt-desktop --exclude rt-git --target wasm32-unknown-unknown --locked
cargo check --workspace --exclude rt-desktop --all-targets --target aarch64-apple-ios --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo deny --all-features check
npm ci --ignore-scripts && npm run check:licenses && npm run test:scripts
npm run check -w apps/ui && npm test -w apps/ui      # Svelte/TS-Typprüfung inkl. Vertrag, Unit-Tests
npm run check -w apps/web && npm test -w apps/web   # Web-Hülle: Typen, Unit-Tests (Git-Adapter, Historie)
RT_BIN=target/debug/rt RT_TESTSERVER_BIN=target/debug/rt-testserver npm run e2e -w apps/web  # Playwright (installiertes Chrome); mit RT_BIN auch echter `rt agent` und Web-Git
npm run dev -w apps/web                              # Web-App lokal, http://127.0.0.1:5173/ (echt) oder ?backend=mock
npm run e2e:shells -w apps/web                       # nur die Hüllen-Tests (echter Kern, OPFS, rt-testserver auf 47815)
node scripts/build-wasm.mjs                          # Kern für die Web-App bauen (apps/web/public/wasm, gitignored)
npm run check -w apps/desktop && npm test -w apps/desktop
npm run dev -w apps/desktop                          # Desktop-App lokal (Tauri)
npm run build -w apps/desktop                        # Release-Build ohne Bundle
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
