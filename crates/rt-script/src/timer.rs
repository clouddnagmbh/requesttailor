// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Waiting without an async runtime: `rt.sleep`, the deadline of `rt.sendRequest`, and
//! yielding to the browser's event loop while a script runs. Natively a short-lived thread
//! wakes the future (works under any executor: tokio, `block_on`); in the browser
//! `setTimeout`.

use std::future::Future;
use std::pin::pin;
use std::task::Poll;

use web_time::{Duration, Instant};

/// Waits until `deadline`.
pub async fn sleep_until(deadline: Instant) {
    let now = Instant::now();
    if deadline > now {
        sleep(deadline - now).await;
    }
}

/// Lets the browser render and handle input; natively nothing to do (sends run off the UI
/// thread).
pub async fn yield_now() {
    #[cfg(target_arch = "wasm32")]
    sleep(Duration::ZERO).await;
}

/// `f`, or `None` if `deadline` passes first.
pub async fn until<F: Future>(f: F, deadline: Instant) -> Option<F::Output> {
    let mut f = pin!(f);
    let mut timeout = pin!(sleep_until(deadline));
    std::future::poll_fn(|cx| {
        if let Poll::Ready(v) = f.as_mut().poll(cx) {
            return Poll::Ready(Some(v));
        }
        if timeout.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn sleep(duration: Duration) {
    use std::sync::{Arc, Mutex};
    use std::task::Waker;

    #[derive(Default)]
    struct Shared {
        done: bool,
        waker: Option<Waker>,
    }

    let shared = Arc::new(Mutex::new(Shared::default()));
    let mut started = false;
    std::future::poll_fn(|cx| {
        let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
        if s.done {
            return Poll::Ready(());
        }
        s.waker = Some(cx.waker().clone());
        if !started {
            started = true;
            let shared = Arc::clone(&shared);
            std::thread::spawn(move || {
                std::thread::sleep(duration);
                let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
                s.done = true;
                if let Some(w) = s.waker.take() {
                    w.wake();
                }
            });
        }
        Poll::Pending
    })
    .await
}

#[cfg(target_arch = "wasm32")]
pub async fn sleep(duration: Duration) {
    use wasm_bindgen::{JsCast, JsValue};

    let ms = duration.as_millis().min(i32::MAX as u128) as i32;
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let global = js_sys::global();
        let set_timeout = js_sys::Reflect::get(&global, &JsValue::from_str("setTimeout"))
            .ok()
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
        match set_timeout {
            Some(f) => {
                let _ = f.call2(&global, &resolve, &JsValue::from(ms));
            }
            None => {
                let _ = resolve.call0(&JsValue::UNDEFINED);
            }
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_and_until() {
        let start = Instant::now();
        futures_lite_block_on(sleep(Duration::from_millis(30)));
        assert!(start.elapsed() >= Duration::from_millis(30));
        let r = futures_lite_block_on(until(
            sleep(Duration::from_secs(5)),
            Instant::now() + Duration::from_millis(20),
        ));
        assert_eq!(r, None);
        let r = futures_lite_block_on(until(async { 7 }, Instant::now()));
        assert_eq!(r, Some(7));
    }

    /// A minimal executor for the tests.
    pub fn futures_lite_block_on<F: Future>(f: F) -> F::Output {
        crate::block_on(f)
    }
}
