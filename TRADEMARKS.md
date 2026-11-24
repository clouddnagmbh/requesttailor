# Trademark policy

The code in this repository is licensed under the Apache License 2.0. The name
**RequestTailor**, the `rt` command-line name when used as a product name, and the
RequestTailor logos ("the marks") are not. Section 6 of the Apache License already says
that it grants no trademark rights; this page explains what is fine and what is not.

## Fine without asking

- Saying that your project works with, integrates with or is based on RequestTailor,
  in plain text ("a plugin for RequestTailor", "forked from RequestTailor").
- Using the name in articles, tutorials, talks, reviews and comparisons.
- Building and running unmodified RequestTailor from this repository, for yourself or
  inside your organization, and calling it RequestTailor.
- Publishing plugins whose names make clear they are third-party, such as
  "Acme OData Tools for RequestTailor".

## Not fine

- Distributing a modified build, or a fork, under the RequestTailor name or logo.
  **Forks are welcome under a different name.** Remove the marks from the user interface,
  installer, icons and package metadata.
- Names, logos or domains that are confusingly similar (for example "RequestTailor Pro
  Community Edition" for a fork).
- Suggesting endorsement, partnership or official status that does not exist.

## Where the marks are in this repository

The logo files live in `brand/` (see `brand/README.md`) and are copied into the app
(`apps/ui/src/brand/`, `apps/web/public/`), the desktop icons
(`apps/desktop/src-tauri/icons/`) and the website (`website/assets/`). They are not
Apache-2.0. A fork replaces all of them.

## Questions

Open an issue if you are unsure; when in doubt, choose your own name.
