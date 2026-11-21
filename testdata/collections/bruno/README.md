# Bruno fixture corpus

86 OpenCollection (YAML) collections, copied unchanged from the end-to-end test fixtures of
[Bruno](https://github.com/usebruno/bruno) at commit `87f3380c1dd3a4c629bc6cf60519215a532474ef`
(`e2e/tests/**/fixtures`). Bruno is MIT licensed; see `LICENSE-bruno.md` in this folder.

Each folder is one collection root (contains `opencollection.yml`). Folder names are the
original fixture path, flattened. Credentials and keys in these files are Bruno's public test
values, not secrets; `.gitleaks.toml` allowlists this folder only.

Used by the round-trip tests in `rt-format`: reading and writing every file must give
byte-identical output. Do not edit these files; add new cases in a separate folder.
