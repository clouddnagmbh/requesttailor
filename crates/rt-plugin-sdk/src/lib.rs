// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! `rt-plugin-sdk`: write RequestTailor plugins in Rust. Implements the binary interface of
//! `crates/rt-plugin/API.md` (exports `rt_alloc`, `rt_dealloc`, `rt_call`; import `rt.host`)
//! so a plugin only implements the hooks it contributes and calls typed host functions.
//!
//! ```ignore
//! use rt_plugin_sdk::api::{PluginContext, PluginRequest, PrepareReply};
//! use rt_plugin_sdk::{Plugin, Result};
//!
//! struct Hello;
//!
//! impl Plugin for Hello {
//!     fn prepare(&self, _: PluginRequest, _: PluginContext) -> Result<PrepareReply> {
//!         Ok(PrepareReply {
//!             patch: rt_plugin_sdk::set_header("X-Hello", "world"),
//!             notes: vec![rt_plugin_sdk::info("said hello")],
//!         })
//!     }
//! }
//!
//! rt_plugin_sdk::plugin!(Hello, manifest = include_bytes!("../plugin.json"));
//! ```
//!
//! Build with `cargo build --release --target wasm32-unknown-unknown` (crate type `cdylib`);
//! the result is a single `.wasm` with the manifest in its `rt-plugin` custom section. In
//! native builds the ABI is left out, so the plugin crate also builds and tests natively:
//! [`testing`] runs the hooks through the same JSON dispatch and answers host calls.
//! `plugins/README.md` is the guide.

/// The request model the API uses (`RequestSpec` in tab calls).
pub use rt_core::model;
pub use rt_core::services::NoteLevel;
/// Loss reports for importers.
pub use rt_format::import::{ImportFormat, Loss, LossEffect, LossKind, LossReport};
pub use rt_plugin::api;
pub use rt_plugin::api::{API_VERSION, MANIFEST_SECTION};

mod data;
mod dispatch;
mod error;
pub mod host;
mod manifest;
mod plugin;

#[cfg(target_arch = "wasm32")]
mod abi;
#[cfg(not(target_arch = "wasm32"))]
pub mod testing;

pub use data::{
    base64_encode, binary_body, body_bytes, file_bytes, file_text, header, text_body, text_file,
};
pub use dispatch::handle;
pub use error::{Error, Result};
pub use manifest::{check_manifest, parse_manifest};
pub use plugin::{Action, Plugin, Tab, info, note, set_header, warning};

/// Where host calls go: the `rt.host` import on wasm32, the test host natively.
mod sys {
    #[cfg(target_arch = "wasm32")]
    pub(crate) use crate::abi::host;
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) use crate::testing::host;
}

/// Connects a [`Plugin`] to the binary interface and embeds the manifest.
///
/// ```ignore
/// rt_plugin_sdk::plugin!(MyPlugin, manifest = include_bytes!("../plugin.json"));
/// ```
///
/// - The first argument is an expression that makes the plugin. It is evaluated for every
///   call, so a unit struct is the usual choice.
/// - `manifest` is the manifest JSON as a `&[u8]` constant, usually `include_bytes!` of a
///   file next to `Cargo.toml` that the tests check with [`parse_manifest`].
///
/// On wasm32 this writes the manifest into the custom section `rt-plugin` and defines the
/// `rt_call` export. Natively it expands to nothing but a compile-time use of both
/// arguments. Use it once per crate.
#[macro_export]
macro_rules! plugin {
    ($plugin:expr, manifest = $manifest:expr $(,)?) => {
        const _: () = {
            const __RT_PLUGIN_MANIFEST_JSON: &[u8] = $manifest;

            #[cfg(target_arch = "wasm32")]
            #[allow(unsafe_code)] // `link_section` (unsafe in 2024): puts the bytes into the section.
            #[unsafe(link_section = "rt-plugin")]
            #[used]
            static RT_PLUGIN_MANIFEST: [u8; __RT_PLUGIN_MANIFEST_JSON.len()] =
                $crate::__private::bytes(__RT_PLUGIN_MANIFEST_JSON);

            #[cfg(target_arch = "wasm32")]
            #[allow(unsafe_code)] // `no_mangle` is unsafe in edition 2024; the export name is the ABI.
            #[unsafe(no_mangle)]
            extern "C" fn rt_call(ptr: i32, len: i32) -> i64 {
                $crate::__private::call(&$plugin, ptr, len)
            }

            #[cfg(not(target_arch = "wasm32"))]
            const _: usize = __RT_PLUGIN_MANIFEST_JSON.len();
            #[cfg(not(target_arch = "wasm32"))]
            #[allow(dead_code)]
            fn rt_call_native() -> impl $crate::Plugin {
                $plugin
            }
        };
    };
}

/// Used by [`plugin!`]; not part of the public API.
#[doc(hidden)]
pub mod __private {
    #[cfg(target_arch = "wasm32")]
    pub use crate::abi::call;

    /// Copies a byte slice into an array at compile time (for the custom section).
    pub const fn bytes<const N: usize>(src: &[u8]) -> [u8; N] {
        let mut out = [0u8; N];
        let mut i = 0;
        while i < N {
            out[i] = src[i];
            i += 1;
        }
        out
    }
}
