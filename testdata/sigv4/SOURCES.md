# AWS Signature Version 4 test vectors

Published SigV4 test vectors, copied byte for byte. Used by `crates/rt-core/tests/sigv4_vectors.rs`.

| Files | Origin | Licence (SPDX) | Fetched | Notes |
| --- | --- | --- | --- | --- |
| `v4/*/` except the two below: `request.txt`, `context.json`, `header-canonical-request.txt`, `header-string-to-sign.txt`, `header-signature.txt`, `header-signed-request.txt` | AWS CRT signing test suite, [awslabs/aws-c-auth `tests/aws-signing-test-suite/v4` at tag v0.9.0](https://github.com/awslabs/aws-c-auth/tree/v0.9.0/tests/aws-signing-test-suite), as redistributed in the crate [`aws-sigv4` 1.6.6](https://crates.io/crates/aws-sigv4/1.6.6) (directory `aws-signing-test-suite/v4`, from [smithy-lang/smithy-rs](https://github.com/smithy-lang/smithy-rs)) | Apache-2.0 (both repositories; "Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.") | 2026-11-22 | 38 cases. The presigned-URL variants (`query-*.txt`) are not copied: RequestTailor signs headers only. |
| `v4/double-url-encode/`, `v4/double-encode-path/` | Same crate `aws-sigv4` 1.6.6, directory `aws-signing-test-suite/v4` (added there to the CRT suite, see its README) | Apache-2.0 | 2026-11-22 | No `context.json` upstream; only the canonical request is compared (the test supplies the crate's test parameters). |

The credentials in `context.json` are the documented AWS example credentials (`AKIDEXAMPLE`, `wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY`), not real keys.

How the test runs each case, following `aws-sigv4`'s own runner:

- `normalize`, `sign_body` (adds `X-Amz-Content-Sha256`) and `omit_session_token` come from `context.json`.
- Paths are double-encoded (the AWS rule for every service but S3), except `get-space-normalized`, `get-space-unnormalized` and `get-utf8`, whose expected canonical requests encode the path once; `aws-sigv4` skips them for that reason, this test runs them in the single-encoding (S3) mode.
- The request target is sent as written in `request.txt`; `Host` becomes the URL's authority.
