// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// What a request gets when the user switches its auth to OAuth 2.0 or AWS SigV4 (wave 5).
// Mirrors `OAuth2::default()` and `AwsSigV4::default()` in rt-core.

import type { Auth } from "../../commands/types";

export const FRESH_OAUTH2: Auth = {
  type: "oauth2",
  flow: "client-credentials",
  access_token_url: "",
  refresh_token_url: "",
  client_id: "",
  client_secret: "",
  credentials_placement: "basic-auth-header",
  scope: "",
  parameters: [],
  token: { id: "", placement: { in: "header", prefix: "Bearer" }, source: "access-token" },
  auto_fetch: true,
  auto_refresh: true,
};

export const FRESH_AWS_SIGV4: Auth = {
  type: "awsv4",
  access_key_id: "",
  secret_access_key: "",
  session_token: "",
  service: "",
  region: "",
  profile_name: "",
};
