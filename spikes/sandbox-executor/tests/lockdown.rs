//! Lockdown tests: verify what is and isn't exposed in a restricted rquickjs context.
//!
//! KEY FINDING: `JS_AddIntrinsicBaseObjects` (always called by rquickjs even for Context::base())
//! includes `eval`, `Function`, `queueMicrotask`, `globalThis`, and `Reflect`.
//! These MUST be overwritten/deleted in the production sandbox wrapper after context construction.
//!
//! Strategy: check globals from the Rust side via `ctx.globals().get(name)` — no Eval needed.

use rquickjs::{Context, Runtime, Value};

fn make_restricted_context() -> (Runtime, Context) {
    let rt = Runtime::new().unwrap();
    // Context::base() calls JS_AddIntrinsicBaseObjects then nothing else.
    // This is the minimal rquickjs configuration.
    let ctx = Context::base(&rt).expect("failed to build base context");
    (rt, ctx)
}

fn is_present(_ctx: &rquickjs::Ctx, globals: &rquickjs::Object, name: &str) -> bool {
    globals
        .get::<_, Value>(name)
        .map(|v| !v.is_undefined())
        .unwrap_or(false)
}

// ── Globals that are ABSENT in Context::base() (no extra intrinsics) ─────────

/// Network/IO capabilities absent — these would be the highest-risk globals.
#[test]
fn network_globals_absent_in_base() {
    let (_rt, ctx) = make_restricted_context();
    let network_globals = ["fetch", "XMLHttpRequest", "WebSocket"];

    ctx.with(|ctx| {
        let globals = ctx.globals();
        for name in &network_globals {
            assert!(
                !is_present(&ctx, &globals, name),
                "`{}` should NOT be present in base context (no network intrinsic)",
                name
            );
        }
    });
}

/// Timer capabilities absent — setTimeout/setInterval not in QuickJS base.
#[test]
fn timer_globals_absent_in_base() {
    let (_rt, ctx) = make_restricted_context();

    ctx.with(|ctx| {
        let globals = ctx.globals();
        for name in &["setTimeout", "setInterval"] {
            assert!(
                !is_present(&ctx, &globals, name),
                "`{}` should NOT be present in base context",
                name
            );
        }
    });
}

/// Promise not in base context (requires intrinsic::Promise).
#[test]
fn promise_absent_in_base() {
    let (_rt, ctx) = make_restricted_context();

    ctx.with(|ctx| {
        let globals = ctx.globals();
        assert!(
            !is_present(&ctx, &globals, "Promise"),
            "`Promise` should NOT be present in base context (requires intrinsic::Promise)"
        );
    });
}

/// Node/Bun/Deno runtime globals absent.
#[test]
fn runtime_globals_absent_in_base() {
    let (_rt, ctx) = make_restricted_context();
    let runtime_globals = ["require", "process", "global", "Deno", "Bun", "Worker"];

    ctx.with(|ctx| {
        let globals = ctx.globals();
        for name in &runtime_globals {
            assert!(
                !is_present(&ctx, &globals, name),
                "`{}` should NOT be present in base context",
                name
            );
        }
    });
}

/// Browser-specific globals absent.
#[test]
fn browser_globals_absent_in_base() {
    let (_rt, ctx) = make_restricted_context();

    ctx.with(|ctx| {
        let globals = ctx.globals();
        for name in &["document", "window"] {
            assert!(
                !is_present(&ctx, &globals, name),
                "`{}` should NOT be present in base context",
                name
            );
        }
    });
}

/// Proxy/WeakRef/FinalizationRegistry absent (require separate intrinsics).
#[test]
fn optional_intrinsic_globals_absent_in_base() {
    let (_rt, ctx) = make_restricted_context();

    ctx.with(|ctx| {
        let globals = ctx.globals();
        for name in &["Proxy", "WeakRef", "FinalizationRegistry"] {
            assert!(
                !is_present(&ctx, &globals, name),
                "`{}` should NOT be present in base context",
                name
            );
        }
    });
}

// ── Globals that ARE present in Context::base() (JS_AddIntrinsicBaseObjects) ─
// These are FINDINGS, not failures. The production sandbox wrapper must strip them.

/// FINDING: eval IS present in Context::base() — JS_AddIntrinsicBaseObjects includes it.
/// Production sandbox MUST delete or override globals.eval after construction.
#[test]
fn eval_present_in_base_must_be_stripped_in_production() {
    let (_rt, ctx) = make_restricted_context();

    ctx.with(|ctx| {
        let globals = ctx.globals();
        // Verify it IS present (this documents the risk, not a pass/fail condition)
        let present = is_present(&ctx, &globals, "eval");
        assert!(
            present,
            "UNEXPECTED: eval is absent from base context — update this test and FINDINGS.md"
        );
        // Production mitigation: overwrite eval with undefined/throw
        globals.set("eval", rquickjs::Undefined).unwrap();
        let after: Value = globals
            .get("eval")
            .unwrap_or_else(|_| Value::new_undefined(ctx.clone()));
        assert!(
            after.is_undefined(),
            "eval should be deletable via globals.set to Undefined"
        );
    });
}

/// FINDING: Function constructor IS present in Context::base().
/// Production sandbox MUST override globals.Function.
#[test]
fn function_constructor_present_in_base_must_be_stripped() {
    let (_rt, ctx) = make_restricted_context();

    ctx.with(|ctx| {
        let globals = ctx.globals();
        let present = is_present(&ctx, &globals, "Function");
        assert!(
            present,
            "UNEXPECTED: Function constructor is absent from base context — update FINDINGS.md"
        );
        // Verify we can override it
        globals.set("Function", rquickjs::Undefined).unwrap();
        let after: Value = globals
            .get("Function")
            .unwrap_or_else(|_| Value::new_undefined(ctx.clone()));
        assert!(
            after.is_undefined(),
            "Function constructor should be overridable"
        );
    });
}

/// FINDING: queueMicrotask IS present in Context::base().
/// Lower risk than eval/Function but should be stripped in production.
#[test]
fn queue_microtask_present_in_base_must_be_stripped() {
    let (_rt, ctx) = make_restricted_context();

    ctx.with(|ctx| {
        let globals = ctx.globals();
        let present = is_present(&ctx, &globals, "queueMicrotask");
        assert!(
            present,
            "UNEXPECTED: queueMicrotask is absent from base context — update FINDINGS.md"
        );
    });
}

// ── Isolation: runtimes are independent ──────────────────────────────────────

#[test]
fn runtimes_are_isolated() {
    // Runtime A: mark a global, then drop.
    {
        let (_rt_a, ctx_a) = make_restricted_context();
        ctx_a.with(|ctx| {
            ctx.globals().set("__spike_marker__", 42_i32).unwrap();
            let v: i32 = ctx.globals().get("__spike_marker__").unwrap();
            assert_eq!(v, 42, "marker should be visible within same context");
        });
        // Drop runtime A.
    }

    // Runtime B (fresh): must NOT have the marker.
    let (_rt_b, ctx_b) = make_restricted_context();
    ctx_b.with(|ctx| {
        let globals = ctx.globals();
        assert!(
            !is_present(&ctx, &globals, "__spike_marker__"),
            "__spike_marker__ must not leak from runtime A to runtime B"
        );
    });
}
