// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pure edits of an OAuth 2.0 auth for `OAuth2Editor` (wave 5, strand C). Kept out of the
// component so they are unit tested. The file groups extra parameters by step
// (`additionalParameters.authorizationRequest`, `accessTokenRequest`, `refreshTokenRequest`),
// so the list is always kept in that order; the format's writer refuses any other.

import type { Auth, OAuth2Flow, OAuth2Param, OAuth2Step, ParamPlacement, TokenUse } from "../../commands/types";

export type OAuth2Auth = Extract<Auth, { type: "oauth2" }>;
export type FlowName = OAuth2Flow["flow"];

/** Flows a request can use, in the order the selector shows them. */
export const REQUEST_FLOWS: [FlowName, string][] = [
  ["client-credentials", "Client credentials"],
  ["authorization-code", "Authorization code"],
  ["password", "Password (resource owner)"],
  ["implicit", "Implicit (stored only)"],
];

/** Flows OpenCollection has no form for: only the collection default (`extensions`). */
export const COLLECTION_FLOWS: [FlowName, string][] = [
  ["device-code", "Device code"],
  ["saml2-bearer", "SAML 2.0 bearer"],
];

export const STEPS: [OAuth2Step, string][] = [
  ["authorization", "Authorization"],
  ["access-token", "Token"],
  ["refresh", "Refresh"],
];

const STEP_ORDER: OAuth2Step[] = STEPS.map(([s]) => s);

export function collectionOnly(flow: FlowName): boolean {
  return flow === "device-code" || flow === "saml2-bearer";
}

/** The requests of a flow that extra parameters can go to. */
export function stepsFor(flow: FlowName): OAuth2Step[] {
  switch (flow) {
    case "authorization-code":
      return ["authorization", "access-token", "refresh"];
    case "implicit":
      return ["authorization"];
    default:
      return ["access-token", "refresh"];
  }
}

/** The common part of every flow (everything but the flow's own fields). */
function common(auth: OAuth2Auth): Omit<OAuth2Auth, keyof OAuth2Flow> & { type: "oauth2" } {
  return {
    type: "oauth2",
    access_token_url: auth.access_token_url,
    refresh_token_url: auth.refresh_token_url,
    client_id: auth.client_id,
    client_secret: auth.client_secret,
    credentials_placement: auth.credentials_placement,
    scope: auth.scope,
    parameters: auth.parameters,
    token: auth.token,
    auto_fetch: auth.auto_fetch,
    auto_refresh: auth.auto_refresh,
  };
}

/** `auth` with another flow: client, endpoints, scope and token settings stay; the browser
 *  fields move between authorization code and implicit; parameters of steps the new flow
 *  does not have are dropped. */
export function switchFlow(auth: OAuth2Auth, flow: FlowName): OAuth2Auth {
  if (auth.flow === flow) return auth;
  const base = common(auth);
  const steps = stepsFor(flow);
  base.parameters = auth.parameters.filter((p) => steps.includes(p.step));
  const browser =
    auth.flow === "authorization-code" || auth.flow === "implicit"
      ? { authorization_url: auth.authorization_url, callback_url: auth.callback_url, state: auth.state }
      : { authorization_url: "", callback_url: "", state: "" };
  switch (flow) {
    case "client-credentials":
      return { ...base, flow };
    case "password":
      return { ...base, flow, username: "", password: "" };
    case "authorization-code":
      return { ...base, flow, ...browser, pkce: "S256" };
    case "implicit":
      // No token endpoint: the token comes back in the redirect.
      return { ...base, access_token_url: "", refresh_token_url: "", flow, ...browser };
    case "device-code":
      return { ...base, flow, device_authorization_url: "" };
    case "saml2-bearer":
      return { ...base, flow, assertion: "" };
  }
}

/** Stable sort by step in file order. */
export function groupParams(params: OAuth2Param[]): OAuth2Param[] {
  return params
    .map((p, i) => ({ p, i }))
    .sort((a, b) => STEP_ORDER.indexOf(a.p.step) - STEP_ORDER.indexOf(b.p.step) || a.i - b.i)
    .map(({ p }) => p);
}

/** A new empty parameter at the end of `step`. Browser redirects only carry query
 *  parameters, so authorization parameters start in the query. */
export function addParam(params: OAuth2Param[], step: OAuth2Step): OAuth2Param[] {
  const placement: ParamPlacement = step === "authorization" ? "query" : "body";
  return groupParams([...params, { step, name: "", value: "", placement }]);
}

/** Changes one parameter; a changed step moves it to the end of the new step. */
export function updateParam(params: OAuth2Param[], index: number, patch: Partial<OAuth2Param>): OAuth2Param[] {
  const current = params[index];
  if (!current) return params;
  const next = { ...current, ...patch };
  if (next.step === current.step) return params.map((p, i) => (i === index ? next : p));
  return groupParams([...params.filter((_, i) => i !== index), next]);
}

export function removeParam(params: OAuth2Param[], index: number): OAuth2Param[] {
  return params.filter((_, i) => i !== index);
}

/** The token placement switched to header (`Bearer` prefix) or query (`access_token`). */
export function switchTokenPlacement(token: TokenUse, kind: "header" | "query"): TokenUse {
  if (token.placement.in === kind) return token;
  return {
    ...token,
    placement: kind === "header" ? { in: "header", prefix: "Bearer" } : { in: "query", name: "access_token" },
  };
}
