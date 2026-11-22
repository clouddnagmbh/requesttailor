// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors
//
// The script engine guest: QuickJS-ng plus this glue, compiled to wasm32-wasip1 (reactor) by
// scripts/build-script-engine.mjs and run by rt-script inside wasmi. All messages are UTF-8
// JSON; buffers are packed as (ptr << 32 | len).
//
// Exports:
//   rt_alloc(len) -> ptr, rt_dealloc(ptr, len)  buffers the host fills and frees
//   rt_init(ptr, len) -> packed               { runtime: [{name, code}], shims: [...],
//                                              modules: [...] } -> { ok } | { error }
//                                              once; the host snapshots memory afterwards
//   rt_run(ptr, len) -> packed                one phase (see js/runtime) -> outcome JSON
// Imports:
//   rt.host(ptr, len) -> packed               suspending host call (send, sleep); the reply
//                                              is allocated with rt_alloc, freed here
//   rt.interrupt() -> i32                     1: stop now (time limit)
//
// Every instance serves one call and is then dropped, so neither side frees the input or the
// reply of rt_init / rt_run; replies of rt.host are freed here.

#include <malloc.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "quickjs.h"

#define IMPORT(m, n) __attribute__((import_module(m), import_name(n)))
#define EXPORT(n) __attribute__((export_name(n)))

IMPORT("rt", "host") int64_t host_call(int32_t ptr, int32_t len);
IMPORT("rt", "interrupt") int32_t host_interrupt(void);

// QuickJS stops with a RangeError below this much C stack (the shadow stack is 1 MiB).
#define JS_STACK_LIMIT (768 * 1024)
// Kept back until the first failed allocation, so the outcome can still be built.
#define RESERVE_SIZE (2 * 1024 * 1024)
#define MAX_SHIMS 256
#define MAX_REJECTIONS 64

enum { STOP_NONE = 0, STOP_TIME = 1, STOP_MEMORY = 2 };

static JSRuntime *rt;
static JSContext *ctx;
// The internal object of js/runtime (`globalThis.__rt_internal` at init, then hidden).
static JSValue internal;
static JSValue shims[MAX_SHIMS];
static int shim_count;
static int oom;
static int stop;
static int finishing;
static void *reserve;

// Unhandled promise rejections (reported as console errors).
static JSValue rejected_promises[MAX_REJECTIONS];
static JSValue rejected_reasons[MAX_REJECTIONS];
static int rejection_count;

static int64_t pack(const char *ptr, size_t len) {
    return ((int64_t)(uint32_t)(uintptr_t)ptr << 32) | (int64_t)(uint32_t)len;
}

// One byte more than asked, zeroed: QuickJS parses only NUL-terminated text.
EXPORT("rt_alloc") int32_t rt_alloc(int32_t len) {
    size_t n = len > 0 ? (size_t)len : 0;
    char *p = malloc(n + 1);
    if (p) p[n] = 0;
    return (int32_t)(uintptr_t)p;
}

EXPORT("rt_dealloc") void rt_dealloc(int32_t ptr, int32_t len) {
    (void)len;
    free((void *)(uintptr_t)ptr);
}

static void on_oom(void) {
    if (!oom) {
        oom = 1;
        free(reserve);
        reserve = NULL;
    }
}

static void *m_calloc(void *opaque, size_t count, size_t size) {
    (void)opaque;
    void *p = calloc(count, size);
    if (!p) on_oom();
    return p;
}

static void *m_malloc(void *opaque, size_t size) {
    (void)opaque;
    void *p = malloc(size);
    if (!p) on_oom();
    return p;
}

static void m_free(void *opaque, void *ptr) {
    (void)opaque;
    free(ptr);
}

static void *m_realloc(void *opaque, void *ptr, size_t size) {
    (void)opaque;
    void *p = realloc(ptr, size);
    if (!p && size) on_oom();
    return p;
}

static size_t m_usable_size(const void *ptr) {
    return malloc_usable_size((void *)ptr);
}

static const JSMallocFunctions malloc_functions = {
    m_calloc, m_malloc, m_free, m_realloc, m_usable_size,
};

static int on_interrupt(JSContext *c, void *opaque) {
    (void)c;
    (void)opaque;
    if (finishing) return 0;
    if (stop) return 1;
    if (oom) {
        stop = STOP_MEMORY;
        return 1;
    }
    if (host_interrupt()) {
        stop = STOP_TIME;
        return 1;
    }
    return 0;
}

static void on_rejection(JSContext *c, JSValueConst promise, JSValueConst reason,
                         bool is_handled, void *opaque) {
    (void)opaque;
    if (is_handled) {
        for (int i = 0; i < rejection_count; i++) {
            if (JS_VALUE_GET_PTR(rejected_promises[i]) == JS_VALUE_GET_PTR(promise)) {
                JS_FreeValue(c, rejected_promises[i]);
                JS_FreeValue(c, rejected_reasons[i]);
                rejection_count--;
                memmove(&rejected_promises[i], &rejected_promises[i + 1],
                        (size_t)(rejection_count - i) * sizeof(JSValue));
                memmove(&rejected_reasons[i], &rejected_reasons[i + 1],
                        (size_t)(rejection_count - i) * sizeof(JSValue));
                return;
            }
        }
        return;
    }
    if (rejection_count < MAX_REJECTIONS) {
        rejected_promises[rejection_count] = JS_DupValue(c, promise);
        rejected_reasons[rejection_count] = JS_DupValue(c, reason);
        rejection_count++;
    }
}

// `__rt_host(json) -> json`: the suspending host call.
static JSValue js_host(JSContext *c, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val;
    if (argc < 1) return JS_ThrowTypeError(c, "host call without a message");
    size_t len;
    const char *msg = JS_ToCStringLen(c, &len, argv[0]);
    if (!msg) return JS_EXCEPTION;
    int64_t reply = host_call((int32_t)(uintptr_t)msg, (int32_t)len);
    JS_FreeCString(c, msg);
    char *ptr = (char *)(uintptr_t)(uint32_t)(reply >> 32);
    size_t n = (size_t)(uint32_t)reply;
    JSValue s = JS_NewStringLen(c, ptr, n);
    free(ptr);
    return s;
}

// A constant reply (also when building one fails for lack of memory).
static int64_t fixed(const char *json) {
    return pack(json, strlen(json));
}

// Copies a JS string into a fresh buffer the host frees.
static int64_t reply_of(JSValue str) {
    size_t len;
    const char *s = JS_ToCStringLen(ctx, &len, str);
    JS_FreeValue(ctx, str);
    if (!s) return 0;
    char *buf = malloc(len ? len : 1);
    if (!buf) {
        JS_FreeCString(ctx, s);
        return 0;
    }
    memcpy(buf, s, len);
    JS_FreeCString(ctx, s);
    return pack(buf, len);
}

// `{"error": "<what><message of the pending exception>"}`.
static int64_t init_error(const char *what) {
    JSValue e = JS_GetException(ctx);
    const char *m = JS_ToCString(ctx, e);
    size_t wl = strlen(what), ml = m ? strlen(m) : 0;
    char *text = malloc(wl + ml + 1);
    int64_t r = 0;
    if (text) {
        memcpy(text, what, wl);
        if (m) memcpy(text + wl, m, ml);
        text[wl + ml] = 0;
        JSValue obj = JS_NewObject(ctx);
        JS_SetPropertyStr(ctx, obj, "error", JS_NewString(ctx, text));
        free(text);
        r = reply_of(JS_JSONStringify(ctx, obj, JS_UNDEFINED, JS_UNDEFINED));
        JS_FreeValue(ctx, obj);
    }
    if (m) JS_FreeCString(ctx, m);
    JS_FreeValue(ctx, e);
    return r ? r : fixed("{\"error\":\"the script engine failed to start\"}");
}

static char *cstring(JSValueConst obj, const char *prop, size_t *len) {
    JSValue v = JS_GetPropertyStr(ctx, obj, prop);
    const char *s = JS_ToCStringLen(ctx, len, v);
    JS_FreeValue(ctx, v);
    if (!s) return NULL;
    char *copy = malloc(*len + 1);
    if (copy) {
        memcpy(copy, s, *len);
        copy[*len] = 0;
    }
    JS_FreeCString(ctx, s);
    return copy;
}

EXPORT("rt_init") int64_t rt_init(int32_t ptr, int32_t len) {
    reserve = malloc(RESERVE_SIZE);
    rt = JS_NewRuntime2(&malloc_functions, NULL);
    if (!rt) return fixed("{\"error\":\"cannot create the JavaScript runtime\"}");
    JS_SetMaxStackSize(rt, JS_STACK_LIMIT);
    JS_SetInterruptHandler(rt, on_interrupt, NULL);
    JS_SetHostPromiseRejectionTracker(rt, on_rejection, NULL);
    ctx = JS_NewContext(rt);
    if (!ctx) return fixed("{\"error\":\"cannot create the JavaScript context\"}");

    JSValue global = JS_GetGlobalObject(ctx);
    JS_SetPropertyStr(ctx, global, "__rt_host", JS_NewCFunction(ctx, js_host, "__rt_host", 1));

    JSValue input = JS_ParseJSON(ctx, (const char *)(uintptr_t)ptr, (size_t)len, "<init>");
    if (JS_IsException(input)) {
        JS_FreeValue(ctx, global);
        return init_error("invalid init message: ");
    }
    static const char *lists[] = {"runtime", "shims", "modules"};
    for (int l = 0; l < 3; l++) {
        JSValue list = JS_GetPropertyStr(ctx, input, lists[l]);
        int64_t n = 0;
        if (JS_GetLength(ctx, list, &n) < 0) n = 0;
        for (int64_t i = 0; i < n; i++) {
            JSValue file = JS_GetPropertyUint32(ctx, list, (uint32_t)i);
            size_t name_len, code_len;
            char *name = cstring(file, "name", &name_len);
            char *code = cstring(file, "code", &code_len);
            JS_FreeValue(ctx, file);
            if (!name || !code) {
                free(name);
                free(code);
                JS_FreeValue(ctx, list);
                JS_FreeValue(ctx, input);
                JS_FreeValue(ctx, global);
                return fixed("{\"error\":\"invalid init message\"}");
            }
            JSValue result;
            if (l == 0) {
                // Runtime glue: runs now, as a classic script.
                result = JS_Eval(ctx, code, code_len, name, JS_EVAL_TYPE_GLOBAL);
            } else if (l == 1) {
                // Shims: compiled now, run at the start of every phase (after `rt` exists).
                result = JS_Eval(ctx, code, code_len, name,
                                 JS_EVAL_TYPE_GLOBAL | JS_EVAL_FLAG_COMPILE_ONLY);
                if (!JS_IsException(result)) {
                    if (shim_count == MAX_SHIMS) {
                        JS_FreeValue(ctx, result);
                        result = JS_ThrowRangeError(ctx, "too many shims");
                    } else {
                        shims[shim_count++] = result;
                        result = JS_UNDEFINED;
                    }
                }
            } else {
                // Modules: a CommonJS factory each, called by `require` on first use.
                const char *head = "(function (exports, require, module, __filename, __dirname) {";
                const char *tail = "\n})";
                size_t total = strlen(head) + code_len + strlen(tail);
                char *wrapped = malloc(total + 1);
                if (!wrapped) {
                    result = JS_ThrowOutOfMemory(ctx);
                } else {
                    memcpy(wrapped, head, strlen(head));
                    memcpy(wrapped + strlen(head), code, code_len);
                    memcpy(wrapped + strlen(head) + code_len, tail, strlen(tail) + 1);
                    result = JS_Eval(ctx, wrapped, total, name, JS_EVAL_TYPE_GLOBAL);
                    free(wrapped);
                    if (!JS_IsException(result)) {
                        JSValue add = JS_GetPropertyStr(ctx, internal, "addModule");
                        // The module's name is the file name without `.js`.
                        size_t base = name_len > 3 && !strcmp(name + name_len - 3, ".js")
                                          ? name_len - 3
                                          : name_len;
                        JSValue args[2] = {JS_NewStringLen(ctx, name, base), result};
                        JSValue r = JS_Call(ctx, add, internal, 2, args);
                        JS_FreeValue(ctx, args[0]);
                        JS_FreeValue(ctx, args[1]);
                        JS_FreeValue(ctx, add);
                        result = r;
                    }
                }
            }
            free(code);
            if (JS_IsException(result)) {
                char prefix[300];
                size_t k = name_len < 200 ? name_len : 200;
                memcpy(prefix, name, k);
                strcpy(prefix + k, ": ");
                free(name);
                JS_FreeValue(ctx, list);
                JS_FreeValue(ctx, input);
                JS_FreeValue(ctx, global);
                return init_error(prefix);
            }
            JS_FreeValue(ctx, result);
            free(name);
        }
        JS_FreeValue(ctx, list);
        if (l == 0) {
            // The runtime glue is in; hide its internals from scripts.
            internal = JS_GetPropertyStr(ctx, global, "__rt_internal");
            JSAtom a = JS_NewAtom(ctx, "__rt_internal");
            JS_DeleteProperty(ctx, global, a, 0);
            JS_FreeAtom(ctx, a);
            a = JS_NewAtom(ctx, "__rt_host");
            JS_DeleteProperty(ctx, global, a, 0);
            JS_FreeAtom(ctx, a);
            if (!JS_IsObject(internal)) {
                JS_FreeValue(ctx, input);
                JS_FreeValue(ctx, global);
                return fixed("{\"error\":\"the runtime glue did not define __rt_internal\"}");
            }
        }
    }
    JS_FreeValue(ctx, input);
    JS_FreeValue(ctx, global);
    // Start every phase with a clean heap.
    JS_RunGC(rt);
    return fixed("{\"ok\":true}");
}

static JSValue call_internal(const char *name, int argc, JSValueConst *argv) {
    JSValue f = JS_GetPropertyStr(ctx, internal, name);
    JSValue r = JS_Call(ctx, f, internal, argc, argv);
    JS_FreeValue(ctx, f);
    return r;
}

// Runs jobs and macrotasks (timers, sends) until `promise` settles (or, for
// JS_UNDEFINED, until nothing is left). Returns 0, or -1 with the error in `*error`
// (JS_UNDEFINED when the run was stopped by a limit).
static int run_loop(JSValueConst promise, JSValue *error) {
    unsigned jobs = 0;
    for (;;) {
        if (stop) return -1;
        if (!JS_IsUndefined(promise) && JS_PromiseState(ctx, promise) != JS_PROMISE_PENDING)
            return 0;
        if (JS_IsJobPending(rt)) {
            JSContext *job_ctx;
            if (++jobs % 1024 == 0 && on_interrupt(ctx, NULL)) return -1;
            if (JS_ExecutePendingJob(rt, &job_ctx) < 0) {
                JSValue e = JS_GetException(job_ctx);
                if (stop || JS_IsUncatchableError(e)) {
                    JS_FreeValue(ctx, e);
                    if (!stop) stop = oom ? STOP_MEMORY : STOP_TIME;
                    return -1;
                }
                *error = e;
                return -1;
            }
            continue;
        }
        // Macrotasks (timers, sends) run no bytecode of their own: check the limit here too.
        if (host_interrupt()) {
            stop = STOP_TIME;
            return -1;
        }
        JSValue r = call_internal("tick", 0, NULL);
        if (JS_IsException(r)) {
            JSValue e = JS_GetException(ctx);
            if (stop || JS_IsUncatchableError(e)) {
                JS_FreeValue(ctx, e);
                if (!stop) stop = oom ? STOP_MEMORY : STOP_TIME;
                return -1;
            }
            *error = e;
            return -1;
        }
        int32_t what = 0;
        JS_ToInt32(ctx, &what, r);
        JS_FreeValue(ctx, r);
        if (what == 2) {
            stop = STOP_TIME;
            return -1;
        }
        if (what == 0) return 0;
    }
}

// The exception of a failed eval or call: the error, or JS_UNDEFINED when a limit stopped it.
static JSValue take_exception(void) {
    JSValue e = JS_GetException(ctx);
    if (stop || JS_IsUncatchableError(e)) {
        JS_FreeValue(ctx, e);
        if (!stop) stop = oom ? STOP_MEMORY : STOP_TIME;
        return JS_UNDEFINED;
    }
    if (oom) stop = STOP_MEMORY;
    return e;
}

EXPORT("rt_run") int64_t rt_run(int32_t ptr, int32_t len) {
    JS_UpdateStackTop(rt);
    JSValue error = JS_UNDEFINED;
    JSValue input = JS_ParseJSON(ctx, (const char *)(uintptr_t)ptr, (size_t)len, "<run>");
    if (JS_IsException(input)) return fixed("{\"error\":\"invalid run message\"}");

    // Build `rt`, `test`, `console`, `require` for this phase.
    JSValue begin = call_internal("begin", 1, (JSValueConst *)&input);
    if (JS_IsException(begin)) {
        error = take_exception();
        goto finish;
    }
    JS_FreeValue(ctx, begin);

    for (int i = 0; i < shim_count && !stop; i++) {
        JSValue r = JS_EvalFunction(ctx, JS_DupValue(ctx, shims[i]));
        if (JS_IsException(r)) {
            error = take_exception();
            goto finish;
        }
        JS_FreeValue(ctx, r);
    }

    JSValue scripts = JS_GetPropertyStr(ctx, input, "scripts");
    int64_t count = 0;
    if (JS_GetLength(ctx, scripts, &count) < 0) count = 0;
    for (int64_t i = 0; i < count && !stop; i++) {
        JSValue script = JS_GetPropertyUint32(ctx, scripts, (uint32_t)i);
        size_t origin_len, code_len;
        char *origin = cstring(script, "origin", &origin_len);
        char *code = cstring(script, "code", &code_len);
        JS_FreeValue(ctx, script);
        if (!origin || !code) {
            free(origin);
            free(code);
            stop = STOP_MEMORY;
            break;
        }
        JSValue origin_value = JS_NewStringLen(ctx, origin, origin_len);
        JSValue r = call_internal("enter", 1, (JSValueConst *)&origin_value);
        JS_FreeValue(ctx, origin_value);
        JS_FreeValue(ctx, r);
        // Top-level `await` works: an async global script evaluates to a promise.
        JSValue promise = JS_Eval(ctx, code, code_len, origin,
                                  JS_EVAL_TYPE_GLOBAL | JS_EVAL_FLAG_ASYNC);
        free(origin);
        free(code);
        if (JS_IsException(promise)) {
            error = take_exception();
            break;
        }
        r = call_internal("scriptPromise", 1, (JSValueConst *)&promise);
        JS_FreeValue(ctx, r);
        if (run_loop(promise, &error) < 0) {
            JS_FreeValue(ctx, promise);
            break;
        }
        JSPromiseStateEnum state = JS_PromiseState(ctx, promise);
        if (state == JS_PROMISE_REJECTED) {
            error = JS_PromiseResult(ctx, promise);
            if (oom) stop = STOP_MEMORY;
            JS_FreeValue(ctx, promise);
            break;
        }
        JS_FreeValue(ctx, promise);
        if (state == JS_PROMISE_PENDING) {
            // Nothing is left that could settle it (e.g. a promise nobody resolves).
            break;
        }
    }
    JS_FreeValue(ctx, scripts);
    // The phase ends when everything has settled: async tests, timers, sends.
    if (!stop && JS_IsUndefined(error)) run_loop(JS_UNDEFINED, &error);

finish:
    finishing = 1;
    if (stop) {
        JS_FreeValue(ctx, error);
        error = JS_UNDEFINED;
    }
    JSValue rejections = JS_NewArray(ctx);
    for (int i = 0; i < rejection_count; i++) {
        JSValue pair = JS_NewArray(ctx);
        JS_SetPropertyUint32(ctx, pair, 0, JS_DupValue(ctx, rejected_promises[i]));
        JS_SetPropertyUint32(ctx, pair, 1, JS_DupValue(ctx, rejected_reasons[i]));
        JS_SetPropertyUint32(ctx, rejections, (uint32_t)i, pair);
    }
    const char *stopped = stop == STOP_TIME ? "time" : stop == STOP_MEMORY ? "memory" : "";
    JSValue args[3] = {error, JS_NewString(ctx, stopped), rejections};
    JSValue out = call_internal("finish", 3, args);
    JS_FreeValue(ctx, args[0]);
    JS_FreeValue(ctx, args[1]);
    JS_FreeValue(ctx, args[2]);
    JS_FreeValue(ctx, input);
    if (JS_IsException(out)) {
        JS_FreeValue(ctx, JS_GetException(ctx));
        return stop == STOP_TIME ? fixed("{\"stopped\":\"time\"}")
               : oom || stop == STOP_MEMORY ? fixed("{\"stopped\":\"memory\"}")
                                            : fixed("{\"error\":\"the script engine failed\"}");
    }
    int64_t r = reply_of(out);
    if (r) return r;
    return stop == STOP_TIME ? fixed("{\"stopped\":\"time\"}") : fixed("{\"stopped\":\"memory\"}");
}
