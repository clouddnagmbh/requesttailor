// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Wave 19, strand G: the web shell's sequence diagrams (`Backend.sequence`, `rt_app::sequence`).
//! One [`Recorder`] lives for the page; every browser send hands its masked exchange to it
//! (`services_js` puts [`observers`] into the send's services), so a diagram of a run, of the
//! session or of one request's pages is built from what the core really sent.
//!
//! ```text
//! sequenceModel(source, options) -> SequenceModel      // the recorder's model
//! sequenceRender(model, options) -> SequenceText       // Mermaid text and a Markdown block
//! sequenceClear()                                      // forget everything recorded
//! sequenceBeginRun() -> number                         // the runner marks a run's start ...
//! sequenceEndRun()                                     // ... and its end
//! ```
//!
//! The web has no audit log (`mcp` sources reject) and keeps its history in the shell
//! (`history` sources reject); both answer with a clear message.

use std::sync::{Arc, OnceLock};

use rt_app::sequence::{self, Recorder, SequenceModel, SequenceOptions, SequenceSource};
use rt_core::observe::ObserverSet;
use wasm_bindgen::prelude::*;

use crate::workspace_js::{from_js, named_error, to_js};

fn recorder() -> &'static Arc<Recorder> {
    static RECORDER: OnceLock<Arc<Recorder>> = OnceLock::new();
    RECORDER.get_or_init(|| Arc::new(Recorder::new()))
}

/// The observers of every browser send: the recorder only.
pub fn observers() -> &'static ObserverSet {
    static SET: OnceLock<ObserverSet> = OnceLock::new();
    SET.get_or_init(|| ObserverSet::new().with(recorder().clone()))
}

/// The model of `source` from what this page sent.
#[wasm_bindgen(js_name = sequenceModel)]
pub fn sequence_model(source: JsValue, options: JsValue) -> Result<JsValue, JsValue> {
    let source: SequenceSource = from_js(&source, "source")?;
    let options: SequenceOptions = from_js(&options, "options")?;
    let model = match &source {
        SequenceSource::Mcp { .. } => {
            return Err(named_error(
                "unsupported",
                "MCP runs are in the audit log of the desktop app and `rt mcp`; the browser has none.",
            ));
        }
        SequenceSource::History { .. } => {
            return Err(named_error(
                "unsupported",
                "Diagrams of the history are built by the desktop app.",
            ));
        }
        _ => recorder()
            .model(&source, &options)
            .map_err(|e| named_error("shell", &e))?,
    };
    to_js(&model)
}

/// Mermaid text and Markdown of `model`.
#[wasm_bindgen(js_name = sequenceRender)]
pub fn sequence_render(model: JsValue, options: JsValue) -> Result<JsValue, JsValue> {
    let model: SequenceModel = from_js(&model, "model")?;
    let options: SequenceOptions = from_js(&options, "options")?;
    to_js(&sequence::render(&model, &options))
}

/// Forgets everything recorded.
#[wasm_bindgen(js_name = sequenceClear)]
pub fn sequence_clear() {
    recorder().clear();
}

/// A run starts: its exchanges belong together. Returns the run's id.
#[wasm_bindgen(js_name = sequenceBeginRun)]
pub fn sequence_begin_run() -> u32 {
    recorder().begin_run()
}

/// The run ended.
#[wasm_bindgen(js_name = sequenceEndRun)]
pub fn sequence_end_run() {
    recorder().end_run();
}
