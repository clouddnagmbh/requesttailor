# Test data of wave 19 (insight)

Everything here is synthetic and written for this repository (Apache-2.0); nothing is copied
from a vendor or a third party. The services listen on loopback only and hold no real data.

| File | Strand | Default port | What it is |
| --- | --- | --- | --- |
| `orders-api.mjs` | C, D, E, F | 48780 | in-memory orders API with `Location`/`ETag`, two users, one deliberate access bug, one poisoned input |
| `fake-trace-host.mjs` | A | 48782 | Jaeger-style trace lookup with three spans |
| `vulnerable-api.mjs` | R, S | 48806 | one path per finding of the passive security checks, plus `/good` |

Ports 48700 to 48847 belong to wave 19 (see the pro repository's `docs/welle19.md`); a strand
that starts a service takes its port from its own range and says so in its test.

- `sequence/*.mmd`, `sequence/*.md`: golden outputs of the Mermaid and Markdown writers (wave 19, strand G); written by `RT_UPDATE_GOLDEN=1 cargo test -p rt-app --test sequence`, own work, Apache-2.0.
## notebooks/ (wave 19, strand W)

`notebooks/orders.notebook.md`: a synthetic API notebook (login, list, a request file) written
for the round-trip test of `rt_app::notebook`; no real host, no personal data. Ports 48830 to
48835 belong to strand W (tests bind loopback ports of the system).

## plugins/soap and plugins/sap-integration (wave 19, strands T and U)

Their test data lives with the plugins (`plugins/soap/testdata/SOURCES.md`,
`plugins/sap-integration/testdata/SOURCES.md`); everything is synthetic, written for this
repository (Apache-2.0). The test services listen on loopback only. Ports: `plugins/soap` 48812
and 48813 (48817 reserved), `plugins/sap-integration` 48818 to 48820.
