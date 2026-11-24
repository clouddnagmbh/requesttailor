// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18 interface (strand J fills in the display): the time segments a send reported
// (`SendReport.segments`, from a plugin's `NotesReply.segments`), for the second line under the
// timing bar, "as reported by the server". They never count towards the total.

import type { SendReport, TimingSegment } from "../commands/types";

export function segmentsOf(report: SendReport): TimingSegment[] {
  return report.segments ?? [];
}
