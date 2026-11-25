// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! The engine host: QuickJS-ng compiled to WebAssembly (`engine/rt-script-engine.wasm`, built
//! by `scripts/build-script-engine.mjs`), run in wasmi on every target. ABI in
//! `engine/src/engine.c`.
//!
//! - The module is compiled once and the engine started once (`rt_init`: runtime glue, shims
//!   compiled, modules registered); its linear memory is the snapshot every run starts from
//!   (a fresh instance per run, so no state leaks from one phase to the next). The only
//!   mutable global is the shadow stack pointer, which is back at its initial value whenever
//!   an export returns.
//! - Limits: the store's memory limit; fuel handed out in slices. Between slices the host
//!   checks the wall clock and, in the browser, yields to the event loop so the page keeps
//!   rendering. QuickJS polls `rt.interrupt`, which answers "stop" once the time or fuel
//!   budget is spent: QuickJS then unwinds with an uncatchable error and the outcome (tests,
//!   logs so far) is still built. A guest that does not come back (a long native call) is
//!   dropped after a grace period.
//! - WASI: only what wasi-libc needs, with fixed answers: clocks, stdout/stderr into a small
//!   buffer, no files, no environment, no arguments.

use std::sync::OnceLock;

use rt_core::services::LocalBoxFuture;
use wasmi::errors::HostError;
use wasmi::{
    Caller, Config, Engine, Error, Extern, ExternType, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, TypedFunc, TypedResumableCall, Val, ValType,
};
use web_time::{Duration, Instant};

use crate::timer;

static ENGINE_WASM: &[u8] = include_bytes!("../engine/rt-script-engine.wasm");

mod js {
    include!(concat!(env!("OUT_DIR"), "/js_files.rs"));
}

/// Fuel per millisecond of the time limit. wasmi charges a block's fuel when it is entered, and
/// QuickJS's interpreter loop is one huge `switch`, so fuel counts far more than the
/// instructions executed (about 0.5 G fuel per millisecond of a busy loop in a native release
/// build, less for C-heavy code). The fuel budget is therefore a deterministic backstop; the
/// wall clock is what normally ends a script.
pub const FUEL_PER_MS: u64 = 500_000_000;
/// Fuel per slice; after each slice the host checks the clock (and yields in the browser).
const SLICE: u64 = 20_000_000;
/// Yield to the browser's event loop at least this often while a script runs.
const YIELD_EVERY: Duration = Duration::from_millis(25);
/// How long a guest may take to wind down after its budget is spent before it is dropped.
const GRACE: Duration = Duration::from_millis(1_000);
/// Fuel for starting the engine (runtime glue, shims, modules).
const INIT_FUEL: u64 = 20_000_000_000;
/// Fuel for one `rt_alloc` call of the host.
const ALLOC_FUEL: u64 = 100_000;
/// Largest message read from the guest.
const MAX_MESSAGE: usize = 32 * 1024 * 1024;
/// Most guest stdout/stderr kept (debugging only).
const MAX_LOG: usize = 16 * 1024;

/// How a run ended when the guest gave no outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    Time,
    Memory,
    /// Stack overflow in the engine itself.
    Stack,
    Engine(String),
}

/// Answers the guest's host calls (`rt.host`): the JSON message in, the JSON reply out.
pub trait HostCalls {
    fn call<'a>(&'a self, message: Vec<u8>, deadline: Instant) -> LocalBoxFuture<'a, Vec<u8>>;

    /// Time the host kept the guest waiting for the user (a paused debug run); handed back
    /// after each call and added to the time limit, so looking at a script costs it nothing.
    fn take_credit(&self) -> Duration {
        Duration::ZERO
    }

    /// Multiplies the fuel backstop of the run (a debug run, whose pauses run JavaScript).
    fn fuel_factor(&self) -> u64 {
        1
    }
}

fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = Config::default();
        config.consume_fuel(true);
        // QuickJS recursion is several wasm frames per JS call; its own stack check (a
        // RangeError in the script) should come first.
        config.set_max_recursion_depth(100_000);
        config.set_max_stack_height(64 * 1024 * 1024);
        // Compile everything when the module is created (once per process): lazily compiled
        // functions would be charged to the fuel of whichever script first calls them, and
        // running out of fuel there cannot be resumed.
        config.compilation_mode(wasmi::CompilationMode::Eager);
        Engine::new(&config)
    })
}

struct Prepared {
    module: Module,
    /// Linear memory right after `rt_init`.
    snapshot: Vec<u8>,
}

fn prepared_cell() -> &'static OnceLock<Result<Prepared, String>> {
    static PREPARED: OnceLock<Result<Prepared, String>> = OnceLock::new();
    &PREPARED
}

struct Data {
    limits: StoreLimits,
    /// The host call waiting for its answer while the guest is suspended.
    pending: Option<Vec<u8>>,
    log: Vec<u8>,
    deadline: Instant,
    fuel_budget: u64,
    /// Fuel handed out so far (consumed = given - remaining).
    fuel_given: u64,
    last_yield: Instant,
}

/// The marker error that suspends the guest at a host call.
#[derive(Debug)]
struct Suspend;

impl std::fmt::Display for Suspend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("host call")
    }
}

impl HostError for Suspend {}

/// The marker error that suspends the guest so the browser can render.
#[derive(Debug)]
struct Yield;

impl std::fmt::Display for Yield {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("yield")
    }
}

impl HostError for Yield {}

/// The time or fuel budget is spent.
fn over_budget(caller: &Caller<'_, Data>) -> bool {
    let data = caller.data();
    let remaining = caller.get_fuel().unwrap_or(0);
    data.fuel_given.saturating_sub(remaining) >= data.fuel_budget || Instant::now() >= data.deadline
}

const I32: ValType = ValType::I32;
const I64: ValType = ValType::I64;

fn allowed_import(module: &str, name: &str) -> Option<(&'static [ValType], &'static [ValType])> {
    Some(match (module, name) {
        ("rt", "host") => (&[I32, I32], &[I64]),
        ("rt", "interrupt") => (&[], &[I32]),
        ("wasi_snapshot_preview1", name) => match name {
            "fd_write" => (&[I32, I32, I32, I32], &[I32]),
            "fd_close" => (&[I32], &[I32]),
            "fd_fdstat_get" => (&[I32, I32], &[I32]),
            "fd_seek" => (&[I32, I64, I32, I32], &[I32]),
            "clock_time_get" => (&[I32, I64, I32], &[I32]),
            "random_get" => (&[I32, I32], &[I32]),
            "proc_exit" => (&[I32], &[]),
            _ => return None,
        },
        _ => return None,
    })
}

fn compile() -> Result<Module, String> {
    let module = Module::new(engine(), ENGINE_WASM).map_err(|e| format!("invalid engine: {e}"))?;
    for import in module.imports() {
        let (m, n) = (import.module(), import.name());
        let ok = allowed_import(m, n).is_some_and(|(params, results)| {
            matches!(import.ty(), ExternType::Func(f) if f.params() == params && f.results() == results)
        });
        if !ok {
            return Err(format!(
                "the engine imports {m}.{n}, which the host does not offer"
            ));
        }
    }
    Ok(module)
}

fn memory_of(caller: &Caller<'_, Data>) -> Result<Memory, Error> {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| Error::new("the engine has no memory"))
}

fn slice(memory: &[u8], ptr: i32, len: i32) -> Result<std::ops::Range<usize>, Error> {
    let (ptr, len) = (ptr as u32 as usize, len as u32 as usize);
    let end = ptr
        .checked_add(len)
        .filter(|&e| e <= memory.len())
        .ok_or_else(|| Error::new("out of bounds"))?;
    Ok(ptr..end)
}

fn process_start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

fn linker() -> Result<Linker<Data>, String> {
    let mut linker = Linker::<Data>::new(engine());
    let e = |e: wasmi::errors::LinkerError| e.to_string();
    linker
        .func_wrap(
            "rt",
            "host",
            |caller: Caller<'_, Data>, ptr: i32, len: i32| -> Result<i64, Error> {
                let memory = memory_of(&caller)?;
                let data = memory.data(&caller);
                let range = slice(data, ptr, len)?;
                if range.len() > MAX_MESSAGE {
                    return Err(Error::new("host call too large"));
                }
                let bytes = data[range].to_vec();
                let mut caller = caller;
                caller.data_mut().pending = Some(bytes);
                Err(Error::host(Suspend))
            },
        )
        .map_err(e)?;
    linker
        .func_wrap(
            "rt",
            "interrupt",
            |caller: Caller<'_, Data>| -> Result<i32, Error> {
                // QuickJS asks every few thousand bytecodes: a cheap place to let the browser
                // render (the guest is suspended, `drive` yields and resumes it).
                if cfg!(target_arch = "wasm32") && caller.data().last_yield.elapsed() >= YIELD_EVERY
                {
                    return Err(Error::host(Yield));
                }
                Ok(i32::from(over_budget(&caller)))
            },
        )
        .map_err(e)?;
    const WASI: &str = "wasi_snapshot_preview1";
    const EBADF: i32 = 8;
    const ESPIPE: i32 = 70;
    linker
        .func_wrap(
            WASI,
            "fd_write",
            |mut caller: Caller<'_, Data>,
             fd: i32,
             iovs: i32,
             count: i32,
             written: i32|
             -> Result<i32, Error> {
                if fd != 1 && fd != 2 {
                    return Ok(EBADF);
                }
                let memory = memory_of(&caller)?;
                let (mem, data) = memory.data_and_store_mut(&mut caller);
                let table = slice(mem, iovs, count.saturating_mul(8))?;
                let mut total: u32 = 0;
                for iov in mem[table].chunks_exact(8) {
                    let ptr = i32::from_le_bytes(iov[..4].try_into().expect("4 bytes"));
                    let len = i32::from_le_bytes(iov[4..].try_into().expect("4 bytes"));
                    let range = slice(mem, ptr, len)?;
                    total = total.saturating_add(range.len() as u32);
                    let room = MAX_LOG.saturating_sub(data.log.len());
                    let bytes = &mem[range];
                    data.log.extend_from_slice(&bytes[..bytes.len().min(room)]);
                }
                let out = slice(mem, written, 4)?;
                mem[out].copy_from_slice(&total.to_le_bytes());
                Ok(0)
            },
        )
        .map_err(e)?;
    linker
        .func_wrap(WASI, "fd_close", |_: Caller<'_, Data>, _fd: i32| EBADF)
        .map_err(e)?;
    linker
        .func_wrap(
            WASI,
            "fd_fdstat_get",
            |mut caller: Caller<'_, Data>, fd: i32, out: i32| -> Result<i32, Error> {
                if !(0..=2).contains(&fd) {
                    return Ok(EBADF);
                }
                let memory = memory_of(&caller)?;
                let mem = memory.data_mut(&mut caller);
                let range = slice(mem, out, 24)?;
                let stat = &mut mem[range];
                stat.fill(0);
                // filetype: character device; no flags, no rights beyond writing.
                stat[0] = 2;
                stat[8..16].copy_from_slice(&(1u64 << 6).to_le_bytes());
                Ok(0)
            },
        )
        .map_err(e)?;
    linker
        .func_wrap(
            WASI,
            "fd_seek",
            |_: Caller<'_, Data>, _fd: i32, _offset: i64, _whence: i32, _out: i32| ESPIPE,
        )
        .map_err(e)?;
    linker
        .func_wrap(
            WASI,
            "clock_time_get",
            |mut caller: Caller<'_, Data>, id: i32, _precision: i64, out: i32| {
                let nanos = if id == 1 {
                    // Monotonic.
                    process_start().elapsed().as_nanos() as u64
                } else {
                    web_time::SystemTime::now()
                        .duration_since(web_time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_nanos() as u64)
                };
                let memory = memory_of(&caller)?;
                let mem = memory.data_mut(&mut caller);
                let range = slice(mem, out, 8)?;
                mem[range].copy_from_slice(&nanos.to_le_bytes());
                Ok::<i32, Error>(0)
            },
        )
        .map_err(e)?;
    linker
        .func_wrap(
            WASI,
            "random_get",
            |mut caller: Caller<'_, Data>, buf: i32, len: i32| {
                let memory = memory_of(&caller)?;
                let mem = memory.data_mut(&mut caller);
                let range = slice(mem, buf, len)?;
                let mut state = crate::seed();
                for b in &mut mem[range] {
                    state = crate::splitmix(state);
                    *b = (state >> 56) as u8;
                }
                Ok::<i32, Error>(0)
            },
        )
        .map_err(e)?;
    linker
        .func_wrap(
            WASI,
            "proc_exit",
            |_: Caller<'_, Data>, code: i32| -> Result<(), Error> { Err(Error::i32_exit(code)) },
        )
        .map_err(e)?;
    Ok(linker)
}

fn describe(e: &Error) -> Failure {
    if let Some(trap) = e.as_trap_code() {
        return match trap {
            wasmi::TrapCode::OutOfFuel => Failure::Time,
            wasmi::TrapCode::StackOverflow => Failure::Stack,
            // A shadow-stack overflow runs below address 0.
            wasmi::TrapCode::MemoryOutOfBounds => Failure::Stack,
            other => Failure::Engine(format!("the script engine trapped: {other}")),
        };
    }
    if let Some(code) = e.i32_exit_status() {
        return Failure::Engine(format!("the script engine exited with code {code}"));
    }
    Failure::Engine(format!("the script engine failed: {e}"))
}

struct Instance {
    store: Store<Data>,
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
}

impl Instance {
    fn new(
        module: &Module,
        memory_bytes: u64,
        deadline: Instant,
        fuel_budget: u64,
    ) -> Result<(Self, wasmi::Instance), Failure> {
        let data = Data {
            limits: StoreLimitsBuilder::new()
                .memory_size(usize::try_from(memory_bytes).unwrap_or(usize::MAX))
                .memories(1)
                .tables(4)
                .table_elements(100_000)
                .instances(1)
                .build(),
            pending: None,
            log: Vec::new(),
            deadline,
            fuel_budget,
            fuel_given: 0,
            last_yield: Instant::now(),
        };
        let mut store = Store::new(engine(), data);
        store.limiter(|d| &mut d.limits);
        let instance = linker()
            .map_err(Failure::Engine)?
            .instantiate_and_start(&mut store, module)
            .map_err(|e| match describe(&e) {
                Failure::Engine(m) if m.contains("limit") || m.contains("memory") => {
                    Failure::Memory
                }
                other => other,
            })?;
        let memory = instance
            .get_memory(&store, "memory")
            .ok_or_else(|| Failure::Engine("the engine has no memory".into()))?;
        let alloc = instance
            .get_typed_func::<i32, i32>(&store, "rt_alloc")
            .map_err(|e| Failure::Engine(e.to_string()))?;
        Ok((
            Self {
                store,
                memory,
                alloc,
            },
            instance,
        ))
    }

    fn give_fuel(&mut self, fuel: u64) -> Result<(), Failure> {
        let remaining = self.store.get_fuel().unwrap_or(0);
        self.store
            .set_fuel(remaining.saturating_add(fuel))
            .map_err(|e| Failure::Engine(e.to_string()))?;
        self.store.data_mut().fuel_given = self.store.data().fuel_given.saturating_add(fuel);
        Ok(())
    }

    fn consumed(&self) -> u64 {
        let remaining = self.store.get_fuel().unwrap_or(0);
        self.store.data().fuel_given.saturating_sub(remaining)
    }

    /// Copies `bytes` into memory allocated with the guest's `rt_alloc`.
    fn write(&mut self, bytes: &[u8]) -> Result<(i32, i32), Failure> {
        // `rt_alloc` runs on fuel too (a little, counted like the script's).
        self.give_fuel(ALLOC_FUEL)?;
        let len =
            i32::try_from(bytes.len()).map_err(|_| Failure::Engine("message too large".into()))?;
        let ptr = self
            .alloc
            .call(&mut self.store, len)
            .map_err(|e| describe(&e))?;
        if ptr == 0 {
            return Err(Failure::Memory);
        }
        let mem = self.memory.data_mut(&mut self.store);
        let range = slice(mem, ptr, len)
            .map_err(|_| Failure::Engine("rt_alloc returned memory out of bounds".into()))?;
        mem[range].copy_from_slice(bytes);
        Ok((ptr, len))
    }

    fn read_packed(&self, packed: i64) -> Result<Vec<u8>, Failure> {
        let (ptr, len) = ((packed >> 32) as i32, packed as i32);
        let mem = self.memory.data(&self.store);
        let range = slice(mem, ptr, len)
            .map_err(|_| Failure::Engine("the reply is out of bounds".into()))?;
        if range.len() > MAX_MESSAGE {
            return Err(Failure::Engine("the reply is too large".into()));
        }
        Ok(mem[range].to_vec())
    }

    /// Calls `func(input)` and drives it to the end: fuel slices, yields, host calls.
    async fn drive(
        &mut self,
        func: TypedFunc<(i32, i32), i64>,
        input: &[u8],
        mut hard_deadline: Instant,
        hard_fuel: u64,
        host: Option<&dyn HostCalls>,
    ) -> Result<Vec<u8>, Failure> {
        self.give_fuel(SLICE)?;
        let (ptr, len) = self.write(input)?;
        self.store.data_mut().last_yield = Instant::now();
        let mut state = func
            .call_resumable(&mut self.store, (ptr, len))
            .map_err(|e| describe(&e))?;
        loop {
            match state {
                TypedResumableCall::Finished(packed) => return self.read_packed(packed),
                TypedResumableCall::OutOfFuel(invocation) => {
                    if Instant::now() >= hard_deadline || self.consumed() >= hard_fuel {
                        return Err(Failure::Time);
                    }
                    if self.store.data().last_yield.elapsed() >= YIELD_EVERY {
                        timer::yield_now().await;
                        self.store.data_mut().last_yield = Instant::now();
                    }
                    self.give_fuel(SLICE.max(invocation.required_fuel()))?;
                    state = invocation
                        .resume(&mut self.store)
                        .map_err(|e| describe(&e))?;
                }
                TypedResumableCall::HostTrap(suspended) => {
                    if suspended.host_error().downcast_ref::<Yield>().is_some() {
                        timer::yield_now().await;
                        self.store.data_mut().last_yield = Instant::now();
                        let stop = self.store.data().deadline <= Instant::now()
                            || self.consumed() >= self.store.data().fuel_budget;
                        state = suspended
                            .resume(&mut self.store, &[Val::I32(i32::from(stop))])
                            .map_err(|e| describe(&e))?;
                        continue;
                    }
                    if suspended.host_error().downcast_ref::<Suspend>().is_none() {
                        return Err(describe(suspended.host_error()));
                    }
                    let message = self
                        .store
                        .data_mut()
                        .pending
                        .take()
                        .ok_or_else(|| Failure::Engine("host call without a message".into()))?;
                    let reply = match host {
                        Some(host) => host.call(message, self.store.data().deadline).await,
                        None => br#"{"error":"no host calls while the engine starts"}"#.to_vec(),
                    };
                    let credit = host.map_or(Duration::ZERO, |h| h.take_credit());
                    if !credit.is_zero() {
                        let data = self.store.data_mut();
                        data.deadline += credit;
                        hard_deadline += credit;
                    }
                    let (ptr, len) = self.write(&reply)?;
                    let packed = (i64::from(ptr as u32) << 32) | i64::from(len as u32);
                    self.store.data_mut().last_yield = Instant::now();
                    state = suspended
                        .resume(&mut self.store, &[Val::I64(packed)])
                        .map_err(|e| describe(&e))?;
                }
            }
        }
    }
}

/// The message `rt_init` gets: the embedded JavaScript.
fn init_message() -> Vec<u8> {
    let files = |list: &[(&str, &str)]| {
        list.iter()
            .map(|(name, code)| serde_json::json!({ "name": name, "code": code }))
            .collect::<Vec<_>>()
    };
    serde_json::to_vec(&serde_json::json!({
        "runtime": files(js::RUNTIME),
        "shims": files(js::SHIMS),
        "modules": files(js::MODULES),
    }))
    .expect("JSON")
}

/// Compiles the engine and runs `rt_init` once per process; later calls reuse the snapshot.
async fn prepared() -> Result<&'static Prepared, String> {
    let cell = prepared_cell();
    if let Some(p) = cell.get() {
        return p.as_ref().map_err(Clone::clone);
    }
    let result = prepare().await;
    // Two runs may prepare at the same time (browser); the first result wins.
    let _ = cell.set(result);
    cell.get()
        .expect("set above")
        .as_ref()
        .map_err(Clone::clone)
}

async fn prepare() -> Result<Prepared, String> {
    let module = compile()?;
    let far = Instant::now() + Duration::from_secs(3600);
    let (mut instance, handle) =
        Instance::new(&module, 256 * 1024 * 1024, far, u64::MAX).map_err(|f| format!("{f:?}"))?;
    instance
        .give_fuel(INIT_FUEL)
        .map_err(|f| format!("{f:?}"))?;
    let initialize = handle
        .get_typed_func::<(), ()>(&instance.store, "_initialize")
        .map_err(|e| e.to_string())?;
    initialize
        .call(&mut instance.store, ())
        .map_err(|e| format!("_initialize failed: {e}"))?;
    let init = handle
        .get_typed_func::<(i32, i32), i64>(&instance.store, "rt_init")
        .map_err(|e| e.to_string())?;
    let reply = instance
        .drive(init, &init_message(), far, u64::MAX, None)
        .await
        .map_err(|f| match f {
            Failure::Engine(m) => m,
            other => format!("{other:?}"),
        })?;
    let reply: serde_json::Value = serde_json::from_slice(&reply).map_err(|e| e.to_string())?;
    if let Some(e) = reply.get("error").and_then(|e| e.as_str()) {
        let log = String::from_utf8_lossy(&instance.store.data().log).into_owned();
        return Err(format!(
            "{e}{}",
            if log.is_empty() {
                String::new()
            } else {
                format!("\n{log}")
            }
        ));
    }
    let snapshot = instance.memory.data(&instance.store).to_vec();
    Ok(Prepared { module, snapshot })
}

/// Starts the engine now (it otherwise starts on the first run). Errors are returned again
/// by every run.
pub async fn warm_up() -> Result<(), String> {
    prepared().await.map(|_| ())
}

/// Runs `rt_run` with `input` in a fresh instance restored from the snapshot.
pub async fn run(
    input: &[u8],
    time_ms: u64,
    memory_bytes: u64,
    host: &dyn HostCalls,
) -> Result<Vec<u8>, Failure> {
    let prepared = prepared().await.map_err(Failure::Engine)?;
    let started = Instant::now();
    let limit = Duration::from_millis(time_ms);
    let deadline = started + limit;
    let fuel_budget = time_ms
        .saturating_mul(FUEL_PER_MS)
        .saturating_mul(host.fuel_factor());
    let (mut instance, handle) =
        Instance::new(&prepared.module, memory_bytes, deadline, fuel_budget)?;
    // Restore the snapshot: grow to its size, copy it over.
    let page = 65_536u64;
    let have = instance.memory.size(&instance.store);
    let want = (prepared.snapshot.len() as u64).div_ceil(page);
    if want > have {
        instance
            .memory
            .grow(&mut instance.store, want - have)
            .map_err(|_| Failure::Memory)?;
    }
    instance.memory.data_mut(&mut instance.store)[..prepared.snapshot.len()]
        .copy_from_slice(&prepared.snapshot);
    let run = handle
        .get_typed_func::<(i32, i32), i64>(&instance.store, "rt_run")
        .map_err(|e| Failure::Engine(e.to_string()))?;
    let hard_fuel = fuel_budget
        .saturating_add(fuel_budget / 4)
        .saturating_add(SLICE * 50);
    instance
        .drive(run, input, deadline + GRACE, hard_fuel, Some(host))
        .await
}
