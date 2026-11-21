# Contributing

Thanks for helping. Before you open a pull request:

1. **Sign off every commit** (Developer Certificate of Origin, https://developercertificate.org).
   Use `git commit -s`, which adds `Signed-off-by: Your Name <you@example.org>`. The email
   must match the commit author. You keep your copyright; there is no CLA.
   Forgot it? `git rebase --signoff <base>` and force-push your branch.
2. **Start every source file with the license header:**
   ```
   // SPDX-License-Identifier: Apache-2.0
   // Copyright 2026 The RequestTailor Authors
   ```
3. **No copyleft dependencies.** CI rejects any dependency whose license is not on the
   allowlist in `deny.toml` (no GPL, AGPL, LGPL). Propose additions in the PR with a reason.
4. **Run the checks** listed in `CLAUDE.md` (section "Befehle"); CI runs the same ones.

Contributions are accepted under the Apache License 2.0. See `TRADEMARKS.md` for the use of the name.
