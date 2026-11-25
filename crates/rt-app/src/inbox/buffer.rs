// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The bounded, in-memory store of the inbox: the captured requests (newest last) and the event
//! log subscribers read with `events(since)`. No threads, no clock: runs everywhere.
//!
//! The limits are hard: when a request does not fit, the oldest fall out of the buffer, their
//! `Request` events leave the log and one `Dropped` event says how many (`dropped` counts them
//! since the inbox started).

use std::collections::VecDeque;

use rt_core::inbox::{
    CapturedRequest, InboxEvent, InboxPage, InboxStatus, MAX_BODY_BYTES, MAX_ENTRIES,
    MAX_PER_SECOND, MAX_TOTAL_BYTES,
};

/// The limits of an inbox. [`Limits::default`] is the product's; tests use smaller or looser
/// ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_body_bytes: usize,
    pub per_second: u32,
    pub max_entries: usize,
    pub max_total_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_body_bytes: MAX_BODY_BYTES,
            per_second: MAX_PER_SECOND,
            max_entries: MAX_ENTRIES,
            max_total_bytes: MAX_TOTAL_BYTES,
        }
    }
}

/// What a request costs in the buffer.
pub fn weight(request: &CapturedRequest) -> usize {
    let headers: usize = request
        .headers
        .iter()
        .map(|(n, v)| n.len() + v.len() + 4)
        .sum();
    request.body.as_ref().map_or(0, String::len)
        + headers
        + request.target.len()
        + request.method.len()
        + 64
}

#[derive(Debug)]
pub struct Buffer {
    limits: Limits,
    next_id: u64,
    seq: u64,
    /// Id and weight of the held requests, oldest first (the requests live in `log`, once).
    held: VecDeque<(u64, usize)>,
    total: usize,
    log: VecDeque<(u64, InboxEvent)>,
    dropped: u64,
}

impl Buffer {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            next_id: 1,
            seq: 0,
            held: VecDeque::new(),
            total: 0,
            log: VecDeque::new(),
            dropped: 0,
        }
    }

    fn event(&mut self, event: InboxEvent) {
        self.seq += 1;
        self.log.push_back((self.seq, event));
        // Keep the log of small events bounded too: drop the oldest that is not a request.
        let cap = self.limits.max_entries * 2 + 16;
        while self.log.len() > cap {
            match self
                .log
                .iter()
                .position(|(_, e)| !matches!(e, InboxEvent::Request { .. }))
            {
                Some(i) => {
                    self.log.remove(i);
                }
                None => break,
            }
        }
    }

    pub fn started(&mut self, port: u16) {
        self.event(InboxEvent::Started { port });
    }

    pub fn stopped(&mut self) {
        self.event(InboxEvent::Stopped);
    }

    /// Stores a request (the `id` is assigned here) and returns it as stored.
    pub fn push(&mut self, mut request: CapturedRequest) -> CapturedRequest {
        request.id = self.next_id;
        self.next_id += 1;
        let weight = weight(&request);
        self.total += weight;
        self.held.push_back((request.id, weight));
        let mut fell = 0u64;
        while self.held.len() > self.limits.max_entries
            || (self.total > self.limits.max_total_bytes && self.held.len() > 1)
        {
            let Some((old_id, w)) = self.held.pop_front() else {
                break;
            };
            self.total -= w;
            fell += 1;
            if let Some(i) = self.log.iter().position(
                |(_, e)| matches!(e, InboxEvent::Request { request } if request.id == old_id),
            ) {
                self.log.remove(i);
            }
        }
        self.event(InboxEvent::Request {
            request: Box::new(request.clone()),
        });
        if fell > 0 {
            self.dropped += fell;
            let count = fell;
            self.event(InboxEvent::Dropped { count });
        }
        request
    }

    pub fn clear(&mut self) {
        self.held.clear();
        self.total = 0;
        self.log
            .retain(|(_, e)| !matches!(e, InboxEvent::Request { .. } | InboxEvent::Dropped { .. }));
        self.event(InboxEvent::Cleared);
    }

    pub fn get(&self, id: u64) -> Option<CapturedRequest> {
        self.log.iter().find_map(|(_, e)| match e {
            InboxEvent::Request { request } if request.id == id => Some((**request).clone()),
            _ => None,
        })
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn events(&self, since: u64) -> InboxPage {
        InboxPage {
            events: self
                .log
                .iter()
                .filter(|(s, _)| *s > since)
                .map(|(_, e)| e.clone())
                .collect(),
            next: self.seq,
        }
    }

    pub fn held(&self) -> u64 {
        self.held.len() as u64
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn total_bytes(&self) -> usize {
        self.total
    }

    pub fn status(&self, port: Option<u16>) -> InboxStatus {
        InboxStatus {
            running: port.is_some(),
            port,
            url: port.map(|p| format!("http://127.0.0.1:{p}/")),
            held: self.held(),
            dropped: self.dropped,
        }
    }
}
