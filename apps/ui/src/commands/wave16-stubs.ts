// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 16 interface: placeholders for the new `AiApi` methods
// until the strands fill them in each shell's own file (desktop `src/ai.ts` and
// `src/contract.ts`, web `src/ai.ts` and `src/contract.ts`, mock `mock-ai.ts` and
// `mock-contract.ts`). Settings read the wave-8 config as one profile; everything that would
// call a provider rejects with `unsupported`; the policy allows everything.

import type { AiApi } from "./backend";
import { BackendError } from "./backend";
import type { AiConfig, AiSettings } from "./types";

/** The wave-8 part of `AiApi`, which every shell has. */
export type AiWave8Api = Pick<AiApi, "config" | "setConfig" | "hasKey" | "setKey" | "deleteKey" | "ask">;

const later = (strand: string) => () =>
  Promise.reject(new BackendError("unsupported", `This arrives in wave 16 (strand ${strand}).`));

/** `rt_app::ai::AiSettings::from_legacy`. */
export function legacySettings(config: AiConfig): AiSettings {
  const labels: Record<AiConfig["provider"], string> = {
    anthropic: "Anthropic",
    openai: "OpenAI",
    "azure-openai": "Azure OpenAI",
    gemini: "Google Gemini",
    mistral: "Mistral",
    "openai-compatible": "OpenAI-compatible endpoint",
    bedrock: "AWS Bedrock",
  };
  return { profiles: [{ id: "default", label: labels[config.provider], ...config }], roles: {}, preview: "first-use" };
}

/** `base` with the wave-16 methods as placeholders. Returns `base` itself, extended, so a shell
 *  object with extra fields (the mock's `asked`) keeps them. */
export function withAiWave16<T extends AiWave8Api>(base: T): T & AiApi {
  const no = later("A");
  const extra: Omit<AiApi, keyof AiWave8Api> = {
    settings: async () => {
      const config = await base.config();
      return config ? legacySettings(config) : null;
    },
    setSettings: no,
    hasProfileKey: async () => false,
    setProfileKey: no,
    deleteProfileKey: no,
    policy: async () => ({ file_off: false, device_off: false }),
    setDeviceOptOut: no,
    preview: no,
    confirmPreview: no,
    complete: no,
    cancel: async () => {},
  };
  return Object.assign(base, extra);
}
