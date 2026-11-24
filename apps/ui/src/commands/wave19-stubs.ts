// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19 interface: placeholder APIs that reject with `unsupported` until a strand replaces
// them in its own file (mock-*.ts here, the shells' own files in apps/web and apps/desktop).
// The placeholders of a group are removed by the integrator, not by the strand.

import type {
  EgressApi,
  JournalApi,
  AccessApi,
  ShrinkApi,
  ConsumersApi,
  SequenceApi,
  SecurityApi,
  NotebookApi,
  DebuggerApi,
} from "./backend";
import { BackendError } from "./backend";

function notYet(feature: string, strand: string): () => Promise<never> {
  return () => Promise.reject(new BackendError("unsupported", `${feature} is not implemented yet (wave 19, strand ${strand}).`));
}

export function stubEgress(): EgressApi {
  const no = notYet("Egress for extensions", "B");
  return { hosts: no, allow: no, remove: no, request: no };
}

export function stubJournal(): JournalApi {
  const no = notYet("The test-data journal", "C");
  return { settings: no, setSettings: no, entries: no, clear: no, planManual: no, planCompensations: no, execute: no, pluginRules: no, preRead: no, states: no };
}

export function stubAccess(): AccessApi {
  const no = notYet("The access check", "D");
  return { check: no };
}

export function stubShrink(): ShrinkApi {
  const no = notYet("The minimal failing request", "E");
  return { run: no, cancel: no, predicate: no };
}

export function stubConsumers(): ConsumersApi {
  const no = notYet("Consumer contracts", "F");
  return { list: no, verify: no };
}

export function stubSequence(): SequenceApi {
  const no = notYet("Sequence diagrams", "G");
  return { model: no, render: no, clear: no };
}

export function stubSecurity(): SecurityApi {
  const no = notYet("Passive security checks", "R");
  return { check: no, rules: no, settings: no, setSettings: no };
}

export function stubNotebook(): NotebookApi {
  const no = notYet("API notebooks", "W");
  return { list: no, read: no, stage: no, run: no };
}

export function stubDebugger(): DebuggerApi {
  const no = notYet("The script debugger", "Y");
  return { start: no, command: no, state: no, stop: no };
}
