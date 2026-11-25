# Test data of the consumer contracts (wave 19, strand F)

Synthetic, written for this repository (Apache-2.0). Three small consumer collections that
the tests in `crates/rt-app/tests/consumers.rs` and `crates/rt-cli/tests/verify_consumers.rs`
run against a tiny in-process provider (ports 48726 and 48727 belong to strand F). The provider's
`opencollection.yml` is written by the tests, since it names paths and URLs of the moment.

| Folder | What it expects |
| --- | --- |
| `billing` | `GET /orders/1` answers 200 with `items[0].price` a number; `POST /orders` answers 201 |
| `portal` | `GET /orders` answers 200 with `count` a number |
| `mobile` | `GET /health` answers 200 (cloned from a Git repository in the tests) |
