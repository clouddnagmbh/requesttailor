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
