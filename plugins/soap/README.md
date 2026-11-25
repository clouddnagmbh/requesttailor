<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->

# SOAP and WSDL plugin

SOAP 1.1 and 1.2 on the public plugin API. The core knows nothing about SOAP. Not bundled: install
`target/plugins/soap.wasm` (`node scripts/build-plugins.mjs soap`). It asks for no rights.

- **Import** (`wsdl` importer): WSDL 1.1 and 2.0 plus the XSD files they import. One folder per
  SOAP binding, one request per operation with the envelope, `Content-Type`, `SOAPAction` and
  sample values from the schema (optional and repeated elements as comments, first choice
  alternative, first enumeration value). The loss report names what was left out: non-SOAP
  bindings, WS-Policy (a `UsernameToken` assertion is recognized), `use="encoded"`, MIME parts,
  one-way operations, `xs:any`, cut recursive types, imports that were not among the files.
- **Request tab "SOAP"**: version, `SOAPAction`, WS-Security UsernameToken and Timestamp. The
  credentials are `{{wsse_username}}` / `{{wsse_password}}` placeholders; the app fills them in
  when sending and the plugin never sees a value. Only clear-text passwords are possible.
- **Response tab "SOAP"**: readable envelope, header blocks, faults as plain text, MTOM packages
  (attachments listed, read only).
- **Hooks**: `prepare` makes `Content-Type`/`SOAPAction` fit the envelope's version and renews an
  expired timestamp when the body holds no secret; `response` turns a fault into a note.

Not done: signature and encryption (WS-Security), PasswordDigest, writing MTOM. See
`API-WUENSCHE.md`. Test data and service: `testdata/SOURCES.md`, `tests/common/mod.rs` (ports
48812 and 48813).
