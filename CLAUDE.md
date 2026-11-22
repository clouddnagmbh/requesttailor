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
crates/rt-script   Skript-Sandbox (QuickJS als WASM in wasmi), Skript-API `api.d.ts`, Shims, `SHIM-COVERAGE.md`
crates/rt-plugin   Plugin-API (`api`, öffentlich, versioniert: `API.md`, `CHANGELOG.md`) und Plugin-Host (WASM)
crates/rt-plugin-sdk  Rust-SDK für Plugins (wasm32-unknown-unknown)
crates/rt-cli      CLI `rt`, Begleitdienst, MCP-Server
crates/rt-wasm     Kern als WebAssembly
crates/rt-app      Befehlsschicht der App (Rust-Seite): Workspace, Antwortspeicher, Secrets, Git-Typen, Import, Historie, Suche
crates/rt-git      Git nativ (gitoxide), nur Desktop/CLI/iOS, nicht wasm32
crates/rt-grpc     gRPC: Proto-Dateien, Server-Reflection, JSON ↔ Protobuf (alle Ziele)
crates/rt-testserver  lokaler HTTP-Testserver (nur Testinfrastruktur, nativ)
testdata/collections/bruno  Korpus: 86 Bruno-Collections (MIT), unverändert, für Round-Trip-Tests
testdata/import    Import-Korpus pro Quelle, frei lizenziert, Herkunft und Lizenz je Datei in SOURCES.md
testdata/sigv4     AWS-SigV4-Testvektoren (Apache-2.0, Herkunft in SOURCES.md)
testdata/idp       Keycloak als Test-Identitätsanbieter für OAuth 2.0 (`idp.sh start|stop`, Port 8180)
apps/ui            gemeinsame Oberfläche (Svelte 5, TypeScript, CodeMirror 6)
apps/desktop       Tauri-2-Hülle; Crate `rt-desktop` in `apps/desktop/src-tauri` (nur Desktop, nicht wasm32/iOS)
apps/web           Web-App
website           statische Website (`/`, `/sap/`, `/download/`, `/docs/`), ohne Skripte und Fremdanfragen
examples          Beispiel-Collections S/4HANA, CAP, RAP gegen `rt-odata-testserver`
launch            Launch-Kit: Checkliste, Release Notes, Entwurf SAP-Community-Post (veröffentlicht nur der Nutzer)
plugins/example    Beispiel-Plugin (Rust); Anleitung „Ein Plugin schreiben“ in `plugins/README.md`
plugins/odata      OData-Plugin (Welle 6): Module je Strang (`metadata`, `query`, `write`, `landscape`), Modell `edm.rs`, `service.rs`; `API-WUENSCHE.md`
plugins/odata/testdata   synthetische S/4- (V2, V4 RAP) und CAP-Dienste (V4, V2), Herkunft in SOURCES.md
plugins/odata/testserver OData-Testdienst `rt-odata-testserver` (Port 47830, nur nativ)
sdk/typescript     TypeScript-Typen der Plugin-API und Test-Host (TS→WASM-Build noch offen)
ci/canary          absichtlich verbotene Abhängigkeiten; die Lizenz-Gates müssen sie ablehnen
scripts            Prüfskripte ohne Abhängigkeiten (Lizenzen, Köpfe, DCO, Performance)
perf/budgets.toml  Performance-Budgets, einzige Quelle für die Schwellwerte
```

## Kern-Schnittstellen

Stand Welle 2, ergänzt in den Wellen 3 bis 10 (unten). Änderungen daran sind Schnittstellen-Änderungen (Arbeitsregel 1).

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

### Plugins und Auth (ab Welle 5)

- Senden mit Diensten: `rt_core::send::send_with(…, &Services)`. `send` ist `send_with` ohne Dienste. `rt_core::services` hat `TokenStore` und `AuthPrompt` (OAuth), `CookieJar` und `RequestHooks` (Plugins); die Futures sind nicht `Send` (wasm32). Reihenfolge nach `prepare`: OAuth-Token, Plugin-`before_send` (maskiert), SigV4-Signatur, Client-Zertifikat (`rt_core::tls`), Cookies, Leitung, `Set-Cookie`, Plugin-`after_response`. Alles, was dabei an Werten dazukommt (Tokens, Schlüssel), wird im Report maskiert; `SendReport.notes` trägt Hinweise von Auth und Plugins.
- Modell `rt_core::model::auth`: `Auth::OAuth2` (Flows Client Credentials, Password, Authorization Code mit PKCE, Implicit nur lesend, Device Code, SAML 2.0 Bearer) und `Auth::AwsSigV4`; `ClientCertificate` in `Collection.config` und `Environment.client_certificates`; `Environment.external_secrets`. Device Code und SAML Bearer kennt OpenCollection nicht: nur als Collection-Standard, in `extensions.requesttailor.auth` von `opencollection.yml`.
- Format: je Bereich ein Modul in `rt-format/src/auth/` (`oauth2`, `awsv4`, `certs`, `secrets`). Schreiber lassen unveränderte Teile byte-gleich; die Rücklese-Prüfung jeder Änderung verweigert, was ein Schreiber nicht kann.
- Plugin-API: `rt_plugin::api` und `crates/rt-plugin/API.md`. Ein Plugin ist eine `.wasm`-Datei mit dem Manifest in der Custom Section `rt-plugin`; JSON über `rt_call`/`rt.host`; Rechte `network` (Hosts), `storage`, `variables`; Plugins sehen nie Secret-Werte. Jede Änderung an der API braucht einen Eintrag in `crates/rt-plugin/CHANGELOG.md`.
- `Backend` hat `plugins` (`PluginApi`), `auth` (`AuthApi`, Prompts über `onPrompt`), `cookies` (`CookieApi`), `git.scan` und `imports.analyzeWithPlugin`. Dateien je Strang wie in Welle 4 (Desktop `src-tauri/src/{plugins,auth,cookies}.rs`, Web `src/{plugins,auth,cookies}.ts`, Mock `commands/mock-{plugins,auth,cookies}.ts`, CLI `crates/rt-cli/src/{plugins,oauth,cookies,secrets,scan}.rs`, zusammengesetzt in `services.rs`).
- Secret-Scanner: `rt_app::secret_scan::scan`; ein Commit mit Klartext-Zugangsdaten scheitert mit `AppError::SecretsFound`; `rt scan` für Pipelines und Git-Hooks. Unterdrücken nur zeilenweise mit `rt-scan:allow` bzw. `rt-scan:allow-next-line`.
- Plugin-Host: wasmi (Interpreter, gleiche Engine nativ, im Browser und auf iOS), frische Instanz pro Aufruf, Fuel- und 64-MiB-Grenze. Pufferregel steht in API.md (Host gibt `rt_call`-Ein- und Ausgabe frei; `rt.host`-Antworten gibt das Plugin frei). Installierte Plugins: `plugins/` im Datenordner, gemeinsam für Desktop und CLI; `RT_DATA_DIR` ersetzt den Datenordner (Tests setzen ihn oder `--no-plugins`). Im Web in IndexedDB.
- Externe Secrets: nativ über die CLIs der Anbieter (`rt_app::secret_sources`, `CommandRunner`), im Web über `rt agent --allow-secrets` (`POST /v1/secrets`). Werte nur im Speicher, maskiert.
- Client-Zertifikate nur nativ; Browser und Begleitdienst verweigern mit klarem TLS-Fehler. Cookies je Collection und Umgebung, Werte im Report maskiert.
- OAuth-Tokens: nativ im Schlüsselbund (Desktop und CLI teilen die Einträge), im Web im verschlüsselten Browser-Speicher; die UI sieht nur `TokenInfo`.

### SAP und OData (ab Welle 6)

- OData und SAP leben nur im Plugin `plugins/odata` auf der öffentlichen Plugin-API; der Kern bleibt ohne SAP-Wissen. Fehlt ein Erweiterungspunkt, steht der Bedarf in `plugins/odata/API-WUENSCHE.md`, nicht im Kern.
- API-Ergänzungen (additiv, `API_VERSION` bleibt 1, Eintrag in `crates/rt-plugin/CHANGELOG.md`): `contributes.welcome`, Tab-Ort `collection` (`request_path` ist `""`), `context` in `render-tab`/`action`, `ViewReply.create_requests` und `add_environments` (Secrets ohne Wert, ungespeichert), `http` mit `session: true` (Cookie-Jar des Sendens; `PluginHost::in_session`, Desktop/Web/CLI teilen den Jar des Sendens mit den Hooks). In Tabs deckt `{request}` die aufgelöste URL des Requests bzw. bei Collection-Tabs die URL-Variablen der gewählten Umgebung (`rt_app::plugins::tab_scope` → `PluginHost::tab_with(…, TabScope)`); `credentials`/`session` bleiben in Tabs verboten.
- App-Seite: Collection-Tabs über den Header-Knopf „Collection“ und die Palette (`CollectionPanel`, nur sichtbar, wenn ein aktives Plugin einen hat). Vorschläge aus `create_requests`/`add_environments` zeigt `AdditionsDialog` zur Bestätigung (`plugins/additions.ts`: vergebene Namen bekommen „ 2“, vorhandene Umgebungen nur fehlende Variablen); angelegt über `Backend.createFolder`/`createRequest`/`updateEnvironment`, ungespeichert.
- Mitgelieferte Plugins: im Datenordner installiert, aber deaktiviert, bis der Nutzer sie beim ersten Start wählt (`PluginApi.welcome`/`chooseWelcome`, Desktop `plugin_welcome`/`plugin_choose_welcome`). Wer SAP nicht wählt, sieht kein SAP (deaktivierte Plugins: keine Tabs, keine Hooks, keine Beschreibung). Desktop: Tauri-Ressource `src-tauri/bundled-plugins/` (`npm run bundle-plugins -w apps/desktop`, Teil von `build`; Debug-Builds lesen auch `target/plugins`), Antwort in `welcome.json` im Datenordner. Web: `dist/plugins/` aus `npm run build -w apps/web` (nicht im Dev-Server), Antwort in localStorage. Installiert wird erst beim ersten `welcome()` nach dem ersten Frame; ein entferntes mitgeliefertes Plugin bleibt entfernt. Mock: `?backend=mock&welcome=1`.
- Testdaten nur synthetisch (kein `@sap/cds`, keine Hub-Daten); Testdienst `rt-odata-testserver` mit CSRF, ETag, `$batch`, Drafts, `sap-client`.

### Skripte und Tests (ab Welle 7)

- Skript-API: `crates/rt-script/api.d.ts` (`rt.request`, `rt.response`, `rt.vars`, `rt.info`, `rt.sendRequest`, `rt.sleep`, `rt.runner`, `test`, `expect`, `console`). `bru.*`/`req`/`res` und `pm.*` sind Shims darauf; Abdeckung in `crates/rt-script/SHIM-COVERAGE.md`, im Import-Verlustbericht genannt.
- Modell `rt_core::model::{Script, ScriptPhase, Assertion, VarAction, VarScope}`: `RequestSpec.{scripts, assertions, actions}`, `RequestDefaults.scripts` (Collection und Ordner). OpenCollection: `runtime.{scripts, assertions, actions}` in Request-Dateien, `request.scripts` in `opencollection.yml`/`folder.yml`; gRPC-Phasen und andere Aktionstypen bleiben unberührt in der Datei. Lesen und Schreiben in `rt-format/src/runtime.rs` (Writer byte-gleich für Unverändertes, `CollectionDoc::set_folder_scripts` für Collection und Ordner); Importer übernehmen Skripte und melden nur nicht unterstützte APIs (`import/shims.rs`, Liste `import/shim-apis.txt`).
- Ablauf in `send_with`: `before-request`-Skripte (Collection, Ordner außen nach innen, Request) auf dem Request wie geschrieben; gesetzte Variablen in die Laufzeit-Ebene, geänderter Request, dann `prepare` … Leitung … Plugin-`after_response`, danach `after-response`- und `tests`-Skripte, Assertions und Aktionen (`rt_core::assert`, ohne JavaScript). Ein Fehler vor dem Senden ist `SendError::Script`, danach eine Note. `rt_core::script` hat die Typen (`ScriptCall`, `ScriptOutcome`, `VarChange`, `TestResult`, `Flow`), `Services.scripts` ist der `ScriptRunner` (`rt_script::Sandbox`).
- `SendReport` hat `tests`, `assertions`, `logs`, `var_changes`, `flow` (leer weggelassen), alles maskiert. Skripte sehen Secret-Werte, ihre Ausgaben nie unmaskiert. Variablenänderungen behält der Aufrufer (App: Sitzung pro Umgebung, `rt_app::scripts::SessionVars`; Runner und CLI: der Lauf); Dateien schreiben Skripte nie.
- Vertrauen: Skripte laufen nur in Collections, denen der Nutzer vertraut (`rt_app::scripts`, einmal pro Collection und Gerät gefragt); sonst ohne Skripte mit Warn-Note. `rt send`/`rt run` führen Skripte aus, `--no-scripts` schaltet sie ab.
- Runner `rt_core::runner` (`plan`, `run`, `RunPlan`, `RunEvent`, `RunSummary`), eine Umsetzung für App, Web und `rt run`. Exit-Code `EXIT_TESTS` (4), wenn ein Test oder eine Assertion scheitert (`rt send` und `rt run`).
- `Backend` hat `scripts` (`ScriptsApi`: Vertrauen, Sitzungsvariablen) und `runner` (`RunnerApi`, Fortschritt per Callback; Desktop über einen Tauri-`Channel`). Dateien je Strang: Desktop `src-tauri/src/{scripts,session_vars,runner}.rs` und `src/{scripts,runner}.ts`, Web `src/{scripts,session-vars,runner}.ts`, Mock `commands/mock-{scripts,runner}.ts`, CLI `crates/rt-cli/src/run.rs`.

### Protokolle und Agenten (ab Welle 8)

- Modell: `RequestSpec.protocol` (`rt_core::model::Protocol`: `Http` Standard und in JSON weggelassen, `Grpc(GrpcSpec)`, `WebSocket(WebSocketSpec)`); gRPC-Metadaten und WebSocket-Handshake-Header sind `RequestSpec.headers`, `method` ist `GRPC`/`WS`. GraphQL ist HTTP mit `Body::GraphQl { query, variables }` (gesendet als JSON). SSE ist HTTP mit `text/event-stream`. `CollectionConfig.{protobuf, agents, http3}`: `config.protobuf` aus OpenCollection, `agents` und `http3` in `extensions.requesttailor` von `opencollection.yml`.
- Sitzungen `rt_core::stream`: `open` bereitet wie `send_with` vor und öffnet über `StreamTransport`/`StreamSession` (Futures nicht `Send`); `StreamEvent` (`opened`, `message`, `closed`, `error`), maskierte Historie `StreamLog`. gRPC-Nachrichten kodiert `rt-grpc` (JSON ↔ Protobuf), die Leitung bewegt nur Bytes ohne 5-Byte-Rahmen. Im Web laufen alle Sitzungen über den Begleitdienst: `GET /v1/stream?token=…` (WebSocket auf Loopback, `StreamClientMessage`/`StreamServerMessage` in `rt_core::transport::companion`, `Status.features` enthält `"stream"`).
- Agenten: `rt mcp` (stdio) stellt Collections als Werkzeuge bereit (`list_requests`, `get_request`, `list_environments`, `send_request`, `run_folder`). Regeln `AgentPolicy` pro Umgebung (`none`/`read`/`write`, `production`); Standard nur lesen; schreibende Requests (`rt_app::agents::effect`) in Produktion warten auf Freigabe. Freigaben und Audit-Log im Datenordner (`agents/approvals/`, `agents/audit.jsonl`), beantwortet per App-Banner oder `rt approve`/`rt deny`, Liste `rt approvals`. Agenten sehen nie Secret-Werte (maskierte Reports, Secrets nur als Namen). Vertrag der Werkzeuge: `crates/rt-cli/tests/gate_w8.rs`.
- MCP-Client `rt_app::mcp_client`: Server (stdio nur nativ, Streamable HTTP überall) testen; gespeicherte Tests sind Begleitdateien `*.mcp.yaml` (der Reader liest nur `.yml`), `rt mcp-test`.
- KI-Assistent `rt_app::ai`: `AiProvider` (`anthropic` mit eigenem Schlüssel, `openai-compatible` für lokale Modelle), kein Anbieter fest verdrahtet, aus bis konfiguriert, Aufrufe über den eigenen `Transport`. Vor jedem Aufruf `redact` mit allen Secret-Werten; Antworten sind Vorschläge (ungespeichert).
- `Backend` hat `streams`, `grpc`, `graphql`, `agents`, `ai`, `mcp`. Dateien je Strang: Mock `commands/mock-{streams,grpc,graphql,agents,mcp-client,ai}.ts`, Web und Desktop `src/{streams,grpc,graphql,agents,mcp-client,ai}.ts`; CLI `crates/rt-cli/src/{mcp,approvals,mcp_test,mock,docs}.rs`.

### Distributionen (ab Welle 9)

- Eine Distribution baut aus diesem Repo eigene Binaries mit zusätzlichen, einkompilierten Erweiterungen; die Builds dieses Repos haben keine. Erweiterungen sind kein Ersatz für Plugins (WASM, zur Laufzeit).
- Desktop: `rt_desktop::builder()` liefert den fertig konfigurierten `tauri::Builder` (Plugins, Zustand, Befehle); `main.rs` ist `builder().run(generate_context!())`; der Kontext steht nur im Binary, nicht in der Bibliothek. Dasselbe Datenverzeichnis wie `rt` (`rt_plugin::host::dir::data_dir()`, auch bei anderem Bundle-Identifier). `links = "rt-desktop"`: Build-Skripte abhängiger Crates lesen `DEP_RT_DESKTOP_COMMANDS` und `DEP_RT_DESKTOP_CAPABILITIES`. Eine Distribution hängt eigene Tauri-Plugins mit `.plugin(…)` an und läuft mit dem Kontext ihrer eigenen `tauri.conf.json` und eigener Capability-Datei.
- UI: `startApp(target, backend, { extensions })` mit `AppExtension` aus `apps/ui/src/extensions.ts` (Palette-Befehle `commands`, Kopfzeilen-Element `header`, `overlay`, `start` nach dem ersten Frame; `ExtensionContext` = Backend und `AppState`). Die Hüllen exportieren ihr Backend: `@requesttailor/desktop/backend` (`TauriBackend`), `@requesttailor/web/backend` (`createWebBackend`).
- CLI: `rt_cli::run_with(args, stdout, stderr, &[CliExtension])`; ein erstes Argument mit dem Namen einer Erweiterung startet sie, `rt --help` listet sie unter „More commands“. `run` ist `run_with` ohne Erweiterungen.

### Erweiterungspunkte (ab Welle 10)

- Sende-Wächter `rt_core::guard`: `SendGuard::check(&SendCheck) -> GuardVerdict` (`allow` oder `refuse { rule, reason }`). `SendCheck` nennt Aufrufer (`Caller`: `app`, `cli`, `agent`), Zweck (`main`, `script`), Collection, Request-Pfad, Umgebung, Methode, URL mit maskierten Secrets, `Effect` (lesend/schreibend; `effect` lebt jetzt hier, `rt_app::agents` exportiert es weiter) und ob die Umgebung laut Agent-Regeln Produktion ist. `send_with` und `stream::open` fragen `Services.guard` nach Skripten, `prepare` und Plugin-Hooks, direkt vor der Leitung; `rt.sendRequest` aus Skripten ebenso. Ablehnung: `SendError::Blocked { rule, reason }`, nichts geht raus. OAuth-Token-Abrufe fragt der Wächter nicht.
- Hüllen nehmen den Wächter so: CLI `rt_cli::run_distribution(…, &Distribution { extensions, guard })` (`rt send`, `rt run` mit Exit-Code `EXIT_BLOCKED` = 5; `rt mcp` fragt vor jeder Freigabe, schreibt `rule` ins Audit-Log, `AuditEntry.rule`), Desktop `rt_desktop::builder().manage(GuardHandle::new(…))` (`Core::with_guard`), Web `createWebBackend({ sendGuard })` (Feld `guard` der Send-Services, `crates/rt-wasm/src/guard_js.rs`, schließt bei Fehlern). Tests: `crates/rt-core/tests/guard.rs`, `crates/rt-cli/tests/guard.rs` (ohne Harness), `apps/desktop/src-tauri/tests/guard.rs`. Die Builds dieses Repos setzen keinen Wächter.
- Fremde Änderungen im Editor: `AppState.onEdit(listener)` meldet jede Bearbeitung des Nutzers (nicht die über `applyExternal`); `AppState.applyExternal(path, spec)` übernimmt einen anderswo geänderten Request wie eine Bearbeitung (im Speicher, ungespeichert) und zeigt ihn im offenen Tab sofort. Für Erweiterungen, die Bearbeitungen teilen.
- Collection öffnen aus einer Erweiterung (Desktop): `rt_desktop::AppCore` ist öffentlich; ein Tauri-Plugin der Distribution öffnet mit `app.state::<AppCore>().open(&root)` und gibt `CollectionInfo` an seine UI-Erweiterung, die `ctx.app.adopt(info)` aufruft. Die Webview bekommt dafür keinen neuen Befehl.

### Git für APIs (ab Welle 12)

- Alles in Welle 12 ist Free und läuft auf Desktop und Web; Diff, Merge, Bisect und Changelog auch in der CLI (`rt diff`, `rt merge-driver`, `rt bisect`, `rt changelog`, registriert in `crates/rt-cli/src/lib.rs`, je Befehl eine Datei). Free spricht nur mit dem Git-Host des Nutzers, nie mit CloudDNA-Servern.
- Revisionen ohne Checkout: `GitApi.{revParse, treeAt, show, fileLog, mergeBase, commits}`. Nativ `crates/rt-git/src/revisions.rs` (Schnittstelle) und `file_log.rs` (Strang D), Web `apps/web/src/git/revisions.ts`, Mock `mock-git.ts`. `GitTree` hält die Collection-Dateien eines Commits (Regeln `rt_app::git::{tree_path, drop_nested}`, wie `FileSet::from_dir`); ein Pfad mit führendem `/` ist relativ zur Repo-Wurzel (z. B. `/openapi.yaml`).
- Semantischer Diff `rt_format::diff` (`diff(old, new) -> CollectionDiff`, `ModelChange` mit `FieldChange` je Feldpfad und 1-basierter Zeile, `DiffValue::Secret` statt Werten, `files[].covered: false` → Zeilen-Diff) und Merge `rt_format::merge` (`merge(base, ours, theirs) -> MergeResult`, `resolve` mit `ConflictChoice`). Beide arbeiten auf `FileSet`, also auch im Browser. Der Konfliktzustand wird nicht gespeichert, sondern aus den Index-Stufen neu berechnet (`rt_app::merges::MergeState`, `clean` = Dateien ohne Feldkonflikt). `git.pull` merged semantisch (nativ `rt-git/src/merge.rs`, Web `mergeUpstream(…, merger)` in `src/git/repo.ts` über `merges_js.rs`).
- `rt merge-driver %O %A %B %P`: Einrichtung `git config merge.requesttailor.driver "rt merge-driver %O %A %B %P"` und `*.yml merge=requesttailor` in `.gitattributes` (steht in `--help`, `crates/rt-cli/README.md`, Website `docs/cli/`). Exit 0 sauber, 1 mit Konflikt: die Datei behält `ours` mit einem YAML-Kommentar mit dem Wert von `theirs` über jedem Konfliktfeld (keine `<<<<<<<`-Marker, die Datei bleibt lesbar), stderr nennt die Felder. Rebase und Cherry-Pick nur über die Git-Kommandozeile.
- App-Typen: `rt_app::{diffs, merges, compare, bisect, timeline, review, contract, rescue}`; `AppError::Review`. Vergleich und Bisect laufen über den Runner mit den Dateien einer Revision (`RevisionRunRequest`, `rt_app::compare::run_revision`/`compare_revisions`, `rt_app::bisect::{search, test_revision}`), Bisect auf der First-Parent-Kette; Umgebungen, Secrets, Cookies und Plugins vom heutigen Stand. Desktop `TauriBackend` bekommt den Kanal der Bisect-Schritte als viertes Konstruktor-Argument (Tests).
- Zeitreise `rt_app::timeline`: `fileLog` folgt Umbenennungen (nativ `rt-git/src/file_log.rs`, Web `src/git/file-log.ts`, `GitFileCommit.from`), Blame pro Feld aus dem semantischen Diff, alte Version über `updateRequest` wiederherstellen. Testergebnisse pro Commit stehen nie im Repo: nativ im Datenordner unter `test-results/<2 hex>/<oid>.jsonl` (höchstens 20 je Request und Commit), im Web in IndexedDB. Aufgezeichnet wird (`RecordSource`): `runner`, wenn der Lauf genau die Dateien von `HEAD` nutzte (Hook `RunRecorder` in Desktop `runner.rs`, `runRecorder` in Web `runner.ts`); `compare` und `bisect` für jeden Lauf an einer Revision unter deren Commit (Haken `on_run` in `compare_revisions`/`test_revision`, `timeline::records_of_steps`; Desktop `timeline::RevisionRecorder` in `compare.rs`, Web `onRecords` von `compare_js.rs` → `recordAtRevision` in `timeline.ts`). `rt run` und `rt bisect` zeichnen nicht auf.
- PR-Review `rt_app::review::ReviewProvider` (GitHub, GitLab): Token im Schlüsselbund bzw. verschlüsselten Browser-Speicher unter `review:tokens`/Host, Device Code nur mit vom Projektverantwortlichen registrierter OAuth-App; Aufrufe über den eigenen `Transport` (Web über den Begleitdienst); PR-Köpfe unter `refs/requesttailor/review/<kind>/<n>`; selbst gehostete Hosts (GitHub Enterprise, GitLab) als `ReviewHost` pro Gerät (`review/hosts.json` im Datenordner, im Web Browser-Speicher). Device Code ist gebaut, aber aus, solange `GITHUB_CLIENT_ID`/`GITLAB_CLIENT_ID` in `rt_app::review::device` leer sind; die UI fragt `DEVICE_LOGIN_REGISTERED` in `apps/ui/src/review/model.ts` (beides in derselben Änderung umstellen). `PullView` zeigt `SemanticDiff` (`diffs.between(merge_base, head)`, Kommentar an Zeile), `ContractNotes {merge_base, head_ref}`, `compare.run` der geänderten Requests und den CI-Status. Tests nur gegen den lokalen Fake-Host `testdata/review/fake-host.mjs` (GitHub 48041 `/api/v3`, GitLab 48042 `/api/v4`, Git über `git http-backend`); Desktop-Tests in `src-tauri/src/review.rs`, echter Web-Kern in `apps/web/e2e/review/playwright.real.config.ts` (mit `RT_BIN`, `rt agent` auf 48044). Kein `rt review`.
- Vertrag `rt_format::contract`: Spec aus `extensions.requesttailor.contract.spec` oder `openapi.{yaml,yml,json}` (neben der Collection, dann Repo-Wurzel; `rt_app::contract::locate`); `check` (Requests ohne Operation, Parameter, Body-Schema), Breaking Changes zwischen Revisionen, `rt changelog`. `ContractNotes` (nur Breaking Changes) steckt in der Diff-Ansicht `between` (`SemanticDiffView`) und im Review.
- Secret-Rettung `rt_app::rescue`: Scan der Historie (HEAD und lokale Branches), Umschreiben nativ mit gitoxide (`rt-git/src/rewrite.rs`), im Web mit derselben Logik auf isomorphic-git (`src/git/rewrite.ts`, Erkennung und Ersetzung aus `rescue_js.rs`); vorher Sicherung unter `refs/requesttailor/backup/<unix>/<branch>`; Werte erreichen die UI nie (die UI schickt nur Fund-IDs, Variable, Umgebung; den Wert liest Rust mit `secret_scan::scan_values` aus dem Blob, nur für die Rettung). Kein Force-Push aus der App: bei Upstream zeigt der Assistent `git push --force-with-lease` und den Rotations-Hinweis; Tags werden nicht umgeschrieben (`RescueResult.tags` warnt).
- Mock: `MockGit` hat für modellbasierte Mocks `specAt(rev, path)`, `specsAt(rev)` und `mergeCommits()` (Kopien, nicht `GitApi`); kein Mock liest die privaten Felder.
- UI: eine Git-Ansicht im Hauptbereich (`apps/ui/src/git/views.svelte.ts`, `GitViewHost.svelte`), Palette-Einträge und Abschnitt „Tools“ im Git-Panel aus `git/api-commands.ts`; je Strang ein Ordner `apps/ui/src/{diff,merge,compare,timeline,review,contract,rescue}/` mit `commands.ts`. `Backend` hat `diffs`, `merges`, `compare`, `timeline`, `review`, `contract`, `rescue`; Platzhalter in `commands/wave12-stubs.ts`.
- Dateien je Strang: Desktop `src-tauri/src/{diffs,merges,compare,timeline,review,contract,rescue}.rs` und `src/<gleich>.ts`, Web `src/<gleich>.ts` (gemeinsamer Zugang `src/wave12.ts`), Mock `commands/mock-<gleich>.ts`, rt-wasm `src/<gleich>_js.rs`. `backend.test.ts` prüft, dass jeder Befehl aus `COMMANDS` abgebildet ist (Kanäle von Runner, Streams, Bisect und Rescue-Scan sind Konstruktor-Argumente von `TauriBackend`).
- Playwright: Mock-Tests je Strang in `apps/web/e2e/<teil>/*.spec.ts`; echter Kern in `e2e/shells/*.e2e.ts` und eigenen Configs `e2e/{import,compare,review,rescue}/playwright.*.config.ts`, alle Teil von `npm run e2e:shells`. Die Web-Unit-Tests laufen teils gegen den gebauten WASM-Kern (`apps/web/public/wasm`), in der CI deshalb nach `build-wasm`.
- Budget `semantic_diff`: 1000 Requests, 100 geändert, unter 200 ms (`crates/rt-format/tests/diff_perf.rs`, `node scripts/perf/run.mjs --only diff`).

### Erweiterungspunkte (ab Welle 13)

- Tabs aus Erweiterungen: `AppExtension.tabs(ctx)` liefert `ExtensionTab` (`id`, `title`, `place`: `request` | `response` | `collection`, `contentTypes` für Antwort-Tabs wie bei Plugin-Tabs, `component` mit `ExtensionTabProps`: `ctx`, `requestPath` (`""` bei Collection-Tabs), `response` (`StoredResponse` bei Antwort-Tabs), `settle()` vor dem Lesen oder Ersetzen des Requests). Reaktiv wie `commands`; Schlüssel `<extension>/<tab>`. Die Shell stellt die Erweiterungen über `provideExtensions` bereit (`apps/ui/src/shell/extension-tabs.ts`); RequestEditor und ResponsePanel zeigen sie nach den Plugin-Tabs, `CollectionPanel` zeigt Plugin- und Erweiterungs-Tabs zusammen (Kopfzeilen-Knopf „Collection“, Palette „Collection: <Titel>“). Test: `apps/ui/src/shell/extension-tabs.test.ts`.
- Commit ohne Checkout: `rt_git::Repo::commit_to_branch(branch, start, &[TreeFile], message, fallback)` schreibt Dateien (`TreeFile::write`/`remove`, Pfade relativ zur Collection, mit führendem `/` relativ zur Repo-Wurzel) als Commit auf einen lokalen Branch: auf dessen Spitze, oder neu ab der Revision `start`. Nur dieser Ref bewegt sich; Arbeitsbaum, Index und `HEAD` bleiben, der ausgecheckte Branch wird verweigert, ein Commit ohne Änderung auch. Nur nativ. Test: `crates/rt-git/tests/tree_commit.rs`.
- Import in die offene Collection: `ImportApi.writeInto(id, name?)` legt einen Plan (eingebaute Importer, Plugin-Importer, `adopt`) als neuen Ordner der offenen Collection an, ungespeichert wie jede Änderung (`rt_app::import::add_to_workspace`; für Dateien ohne offene Collection `add_to_files`, unveränderte Dateien byte-gleich). Regeln: Name eindeutig („ 2“), Struktur und Reihenfolge bleiben, Skripte der importierten Collection werden Ordner-Skripte; Header, Auth und Variablen der importierten Collection und Ordner trägt jeder Request (Verlust „approximated“ in `FolderImport.losses`, zusätzlich zum Bericht des Plans); Umgebungen: neue werden angelegt, gleichnamige bekommen nur fehlende Variablen, nie Secret-Werte; andere Dateien (Bodies, Begleitdateien) liegen unter dem neuen Ordner. `ImportApi.adopt(ExternalImport)` prüft eine von einer Erweiterung erzeugte Collection (sichere Pfade, lesbar) und hält sie wie jeden Plan. Desktop `import_write_into`/`import_adopt`, Web `Importer.adopt` und `Workspace.addImport` in `rt-wasm`. Der Import-Dialog fragt bei offener Collection „As a new collection“ oder „Into … as a folder“. Tests: `crates/rt-app/tests/import_into.rs`, `apps/web/e2e/import/import.spec.ts`.

## Befehle

Toolchain ist in `rust-toolchain.toml` gepinnt; Upgrades sind ein eigener Commit. Einmalig: `rustup target add wasm32-unknown-unknown aarch64-apple-ios`. Linux braucht für jeden Workspace-Build `libwebkit2gtk-4.1-dev librsvg2-dev libxdo-dev build-essential` (Tauri).

```sh
cargo build --workspace --all-targets --locked
cargo test --workspace --locked
cargo build --workspace --exclude rt-cli --exclude rt-testserver --exclude rt-odata-testserver --exclude rt-desktop --exclude rt-git --target wasm32-unknown-unknown --locked
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
node scripts/check-website.mjs                       # Website: keine Fremdanfragen, Links, Budgets wie perf/budgets.toml
node scripts/check-dco.mjs origin/main..HEAD
node scripts/check-perf-budgets.mjs
node scripts/build-plugins.mjs                       # Beispiel-Plugins nach target/plugins/<id>.wasm (Manifest eingebettet, Importe geprüft)
cargo run -p rt-odata-testserver                     # OData-Testdienst auf 47830 (S/4 V2/V4, CAP V4/V2)
npm run check -w sdk/typescript && npm test -w sdk/typescript   # TS-Typen gegen die Rust-JSON, ABI-Test des gebauten Plugins
testdata/idp/idp.sh start && RT_IDP_URL=http://localhost:8180 cargo test -p rt-core --test oauth2_idp; testdata/idp/idp.sh stop  # OAuth gegen Keycloak (Docker)
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
