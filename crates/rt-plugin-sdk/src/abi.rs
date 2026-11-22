// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The binary interface of `API.md` on wasm32: the exports `rt_alloc` and `rt_dealloc`, the
//! body of `rt_call` (the export itself comes from [`plugin!`](crate::plugin), which knows
//! the plugin type), and the import `rt.host`.
//!
//! This is the only module of the SDK with `unsafe` code. All of it moves bytes across the
//! module boundary; everything else works on owned JSON.
//!
//! Buffer ownership (what the host does, and what this module relies on):
//! - `rt_call(ptr, len)`: the host wrote the call into a buffer from `rt_alloc(len)` and frees
//!   it with `rt_dealloc(ptr, len)` after `rt_call` returns. The reply is a buffer of exactly
//!   the returned length; the host frees it with `rt_dealloc` after reading it.
//! - `rt.host(ptr, len)`: the plugin owns the request buffer. The host writes the reply into a
//!   buffer it gets from `rt_alloc(reply_len)` and returns that pointer and length; the
//!   plugin takes it over and frees it.
//!
//! Every buffer is a `Box<[u8]>`, so allocation and release always use the same layout.

use std::sync::Once;

use crate::{Error, Plugin, Result};

#[allow(unsafe_code)] // Declaring an import is unsafe in edition 2024; `rt.host` is the one import of API.md.
#[link(wasm_import_module = "rt")]
unsafe extern "C" {
    #[link_name = "host"]
    fn rt_host(ptr: i32, len: i32) -> i64;
}

/// Export of API.md: a buffer of `len` bytes for the host to write into.
#[allow(unsafe_code)] // `no_mangle` is unsafe in edition 2024; the export name is the ABI.
#[unsafe(no_mangle)]
pub extern "C" fn rt_alloc(len: i32) -> i32 {
    let len = usize::try_from(len).unwrap_or(0);
    into_raw(vec![0u8; len].into_boxed_slice()).0
}

/// Export of API.md: frees a buffer from `rt_alloc` or the reply of `rt_call`.
#[allow(unsafe_code)] // `no_mangle` is unsafe in edition 2024; the export name is the ABI.
#[unsafe(no_mangle)]
pub extern "C" fn rt_dealloc(ptr: i32, len: i32) {
    drop(from_raw(ptr, len));
}

/// The body of the `rt_call` export: reads the call, runs the hook, returns the reply.
pub fn call<P: Plugin + ?Sized>(plugin: &P, ptr: i32, len: i32) -> i64 {
    install_panic_hook();
    let reply = crate::dispatch::handle(plugin, borrow(ptr, len));
    let (ptr, len) = into_raw(reply.into_boxed_slice());
    pack(ptr, len)
}

/// One `rt.host` call: request bytes in, reply bytes out.
pub fn host(request: &[u8]) -> Result<Vec<u8>> {
    let len = i32::try_from(request.len()).map_err(|_| Error::new("host call too large"))?;
    // The pointer is a wasm32 address; the import reads `len` bytes from it during the call.
    let ptr = request.as_ptr() as usize as i32;
    #[allow(unsafe_code)] // Calling an import; the host only reads `len` bytes at `ptr`.
    let packed = unsafe { rt_host(ptr, len) };
    let (ptr, len) = unpack(packed);
    if ptr == 0 {
        return Err(Error::new("the host returned no reply"));
    }
    Ok(from_raw(ptr, len).into_vec())
}

/// A panic traps the module; log the message first so the user sees why.
fn install_panic_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            crate::host::log(
                rt_core::services::NoteLevel::Error,
                format!("plugin panicked: {info}"),
            );
        }));
    });
}

fn pack(ptr: i32, len: i32) -> i64 {
    (((ptr as u32 as u64) << 32) | len as u32 as u64) as i64
}

fn unpack(packed: i64) -> (i32, i32) {
    let packed = packed as u64;
    ((packed >> 32) as u32 as i32, packed as u32 as i32)
}

fn into_raw(buf: Box<[u8]>) -> (i32, i32) {
    let len = buf.len() as i32;
    (Box::into_raw(buf).cast::<u8>() as usize as i32, len)
}

#[allow(unsafe_code)] // `ptr`/`len` come from `into_raw` (a `Box<[u8]>` of that length), see the module doc.
fn from_raw(ptr: i32, len: i32) -> Box<[u8]> {
    let len = len as u32 as usize;
    if ptr == 0 || len == 0 {
        return Box::default();
    }
    let slice = std::ptr::slice_from_raw_parts_mut(ptr as u32 as usize as *mut u8, len);
    unsafe { Box::from_raw(slice) }
}

#[allow(unsafe_code)] // The host keeps the call buffer alive until `rt_call` returns, see the module doc.
fn borrow<'a>(ptr: i32, len: i32) -> &'a [u8] {
    let len = len as u32 as usize;
    if ptr == 0 || len == 0 {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(ptr as u32 as usize as *const u8, len) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pack_round_trips() {
        assert_eq!(super::unpack(super::pack(0x1000, 42)), (0x1000, 42));
    }
}
