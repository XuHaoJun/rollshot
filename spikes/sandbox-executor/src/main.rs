use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use rquickjs::context::intrinsic;
use rquickjs::prelude::Opt;
use rquickjs::{Context, Exception, Function, Object, Runtime};

fn main() {
    println!("=== rquickjs Sandbox Executor Spike ===\n");

    probe_base_globals();
    test_infinite_loop();
    test_memory_bomb();
    test_deep_recursion();
    test_host_callbacks();
    test_fresh_context_cost();
    test_binary_footprint_note();
}

/// Enumerate which dangerous globals are present in Context::base().
/// This informs what the lockdown layer must strip after construction.
fn probe_base_globals() {
    println!("--- Probe: which globals exist in Context::base()? ---");
    let rt = Runtime::new().expect("runtime");
    let ctx = Context::base(&rt).expect("base context");

    let dangerous = &[
        "eval",
        "Function",
        "setTimeout",
        "setInterval",
        "queueMicrotask",
        "Promise",
        "fetch",
        "XMLHttpRequest",
        "WebSocket",
        "require",
        "process",
        "global",
        "globalThis",
        "Proxy",
        "Reflect",
        "WeakRef",
        "FinalizationRegistry",
        "Worker",
        "Deno",
        "Bun",
        "document",
        "window",
    ];

    ctx.with(|ctx| {
        let globals = ctx.globals();
        let mut present = Vec::new();
        let mut absent = Vec::new();

        for &name in dangerous {
            let is_present: bool = globals
                .get::<_, rquickjs::Value>(name)
                .map(|v| !v.is_undefined())
                .unwrap_or(false);

            if is_present {
                let ty = globals
                    .get::<_, rquickjs::Value>(name)
                    .map(|v| format!("{:?}", v.type_of()))
                    .unwrap_or_else(|_| "unknown".to_string());
                present.push((name, ty));
            } else {
                absent.push(name);
            }
        }

        println!("Present in Context::base() (must be stripped or overridden for production lockdown):");
        for (name, ty) in &present {
            println!("  [PRESENT] {} ({})", name, ty);
        }
        println!("Absent (safe, not exposed):");
        for name in &absent {
            println!("  [absent]  {}", name);
        }
    });
    println!();
}

/// Step 5a: Infinite loop interruption via set_interrupt_handler.
/// Uses Context::base() + intrinsic::Eval — the Eval intrinsic is required to call
/// ctx.eval() from Rust. In production, JS code is compiled to bytecode ahead of
/// time (so Eval is not needed at runtime), but for the spike driver we include it
/// here only to run the payloads. The resource-limit behaviour is independent of
/// which intrinsics are loaded.
fn test_infinite_loop() {
    println!("--- Step 5a: Infinite loop interruption ---");
    let rt = Runtime::new().expect("runtime");

    let interrupted = Arc::new(AtomicBool::new(false));
    let interrupted_clone = interrupted.clone();

    let mut counter = 0u32;
    rt.set_interrupt_handler(Some(Box::new(move || {
        counter += 1;
        if counter > 1000 {
            interrupted_clone.store(true, Ordering::SeqCst);
            true // signal interrupt → QuickJS raises uncatchable exception
        } else {
            false
        }
    })));

    // Context::base() + Eval intrinsic — minimal footprint needed for ctx.eval().
    // Production sandbox uses ctx.eval() only at bytecode-compile time; runtime
    // contexts can omit Eval entirely.
    let ctx = Context::builder()
        .with::<intrinsic::Eval>()
        .build(&rt)
        .expect("context");
    // Use () to avoid Value lifetime issues; the error is what matters
    let result = ctx.with(|ctx| ctx.eval::<(), _>("while (true) {}"));

    match result {
        Ok(()) => println!("FAIL: infinite loop completed without interruption"),
        Err(e) => {
            println!("PASS: infinite loop interrupted");
            println!("  error: {:?}", e);
        }
    }
    println!("  interrupted flag: {}", interrupted.load(Ordering::SeqCst));
    println!();
}

/// Step 5b: Memory bomb OOM via set_memory_limit.
/// Uses Context::base() + Eval intrinsic only (same rationale as 5a).
fn test_memory_bomb() {
    println!("--- Step 5b: Memory bomb OOM ---");
    let rt = Runtime::new().expect("runtime");
    rt.set_memory_limit(4 * 1024 * 1024); // 4 MB

    let ctx = Context::builder()
        .with::<intrinsic::Eval>()
        .build(&rt)
        .expect("context");
    let result = ctx.with(|ctx| {
        ctx.eval::<(), _>(
            "const a = []; for (let i = 0; i < 1000000; i++) { a.push(new Array(1000).fill(i)); }",
        )
    });

    match result {
        Ok(()) => println!("FAIL: memory bomb did not hit OOM"),
        Err(e) => {
            println!("PASS: memory bomb OOM triggered");
            println!("  error: {:?}", e);
        }
    }
    println!();
}

/// Step 5c: Deep recursion stack limit.
/// Uses Context::base() + Eval intrinsic only.
fn test_deep_recursion() {
    println!("--- Step 5c: Deep recursion stack limit ---");
    let rt = Runtime::new().expect("runtime");
    rt.set_max_stack_size(256 * 1024); // 256 KB

    let ctx = Context::builder()
        .with::<intrinsic::Eval>()
        .build(&rt)
        .expect("context");
    let result = ctx.with(|ctx| ctx.eval::<(), _>("function f(){ return f(); } f();"));

    match result {
        Ok(()) => println!("FAIL: deep recursion did not hit stack limit"),
        Err(e) => {
            println!("PASS: deep recursion stack error triggered");
            println!("  error: {:?}", e);
        }
    }
    println!();
}

/// Steps 6a–6c: Host callbacks — register mock rollshot.ocr(), test Err → JS exception,
/// and demonstrate interrupt while inside a host callback.
fn test_host_callbacks() {
    println!("--- Step 6: Host callbacks ---");

    // ── 6a + 6b: basic callback marshal and Err→exception ──────────────────────
    {
        let rt = Runtime::new().expect("runtime");
        let ctx = Context::builder()
            .with::<intrinsic::Eval>()
            .build(&rt)
            .expect("context");

        ctx.with(|ctx| {
            let globals = ctx.globals();

            // 6a: register rollshot.ocr() that returns a mock rect array string
            let rollshot_obj = Object::new(ctx.clone()).expect("object");

            let ocr_fn = Function::new(
                ctx.clone(),
                |arg: Opt<rquickjs::String>| -> rquickjs::Result<String> {
                    let input = arg
                        .0
                        .map(|s| s.to_string().unwrap_or_default())
                        .unwrap_or_else(|| "(none)".to_string());
                    println!("  [host] ocr() called with arg: {:?}", input);
                    Ok("[{\"x\":0,\"y\":0,\"w\":100,\"h\":20,\"text\":\"hello\"}]".to_string())
                },
            )
            .expect("ocr function");

            rollshot_obj.set("ocr", ocr_fn).expect("set ocr");
            globals.set("rollshot", rollshot_obj).expect("set rollshot");

            let result = ctx.eval::<String, _>("rollshot.ocr('screen')");
            match result {
                Ok(s) => println!("PASS 6a: host ocr() called and returned: {:?}", s),
                Err(e) => println!("FAIL 6a: unexpected error: {:?}", e),
            }

            // 6b: host fn returning Err → must surface as catchable JS exception
            let err_fn = Function::new(ctx.clone(), |ctx: rquickjs::Ctx| {
                Err::<(), _>(Exception::throw_message(&ctx, "host-side error"))
            })
            .expect("err function");
            globals.set("throwingFn", err_fn).expect("set throwingFn");

            let result2 = ctx.eval::<String, _>(
                "try { throwingFn(); 'no-throw' } catch(e) { 'caught: ' + e.message }",
            );
            match result2 {
                Ok(s) => {
                    if s.starts_with("caught") {
                        println!("PASS 6b: host Err surfaced as JS exception: {}", s);
                    } else {
                        println!("FAIL 6b: no exception caught, got: {}", s);
                    }
                }
                Err(e) => println!("FAIL 6b: unexpected Rust error: {:?}", e),
            }
        });
    }

    // ── 6c: interrupt fires at JS step boundaries; Rust host calls are NOT pre-empted ──
    //
    // QuickJS's interrupt handler is called at interpreter opcode boundaries.
    // A host function (Rust closure) executes synchronously on the Rust side —
    // no JS opcodes run during it, so the interrupt handler cannot fire mid-call.
    // We demonstrate this by:
    //   (i)  Setting an interrupt counter that fires only AFTER many steps (>1000).
    //   (ii) Having the host function complete (marking host_completed=true).
    //   (iii) Calling into a tight JS loop AFTER the host call, which triggers the interrupt.
    // Result: host_completed=true (host ran fully) AND interrupt_fired=true (fired in JS loop).
    //
    // This is MITIGATED (not PASS) — the interrupt fires at JS step boundaries only.
    // A blocking host call is NOT pre-empted. Record under remaining risks.
    {
        println!("--- Step 6c: Interrupt fires at JS step boundary, not inside host callback ---");

        let rt = Runtime::new().expect("runtime");

        let interrupt_fired = Arc::new(AtomicBool::new(false));
        let interrupt_fired_clone = interrupt_fired.clone();

        // Interrupt fires after >1000 steps — enough to let the host call complete
        // and control return to JS, but tight enough to catch the while(true) loop.
        let mut counter = 0u32;
        rt.set_interrupt_handler(Some(Box::new(move || {
            counter += 1;
            if counter > 1000 {
                interrupt_fired_clone.store(true, Ordering::SeqCst);
                true // raise uncatchable QuickJS exception
            } else {
                false
            }
        })));

        let ctx = Context::builder()
            .with::<intrinsic::Eval>()
            .build(&rt)
            .expect("context");

        let host_completed = Arc::new(AtomicBool::new(false));
        let host_completed_clone = host_completed.clone();

        ctx.with(|ctx| {
            let globals = ctx.globals();

            // Host function: runs entirely in Rust — no JS opcodes, so interrupt
            // counter cannot advance while this is running.
            let fn_ = Function::new(ctx.clone(), move || -> rquickjs::Result<()> {
                // Brief work. The interrupt handler is NOT called while we're here.
                std::thread::sleep(std::time::Duration::from_millis(2));
                host_completed_clone.store(true, Ordering::SeqCst);
                Ok(())
            })
            .expect("fn");
            globals.set("hostFn", fn_).expect("set");

            // JS: call host function (Rust executes fully), then spin — interrupt fires in JS.
            let result = ctx.eval::<(), _>("hostFn(); while(true){}");
            match result {
                Err(_) => println!("PASS 6c: interrupt fired in JS loop after host callback completed"),
                Ok(_) => println!("FAIL 6c: expected interrupt, got Ok"),
            }
        });

        let host_ran = host_completed.load(Ordering::SeqCst);
        let fired = interrupt_fired.load(Ordering::SeqCst);
        println!(
            "  host_completed: {} (host call ran fully — Rust is not pre-empted by QuickJS interrupt)",
            host_ran
        );
        println!(
            "  interrupt_fired: {} (fired at JS step boundary after host returned control to JS)",
            fired
        );
        if host_ran && fired {
            println!("  FINDING: interrupt fires only at JS opcode boundaries.");
            println!("  A blocking host call is NOT pre-empted mid-execution.");
            println!("  MITIGATED: keep host fns short (<1ms); use Rust-side timeout for longer ones.");
        } else if !host_ran {
            println!("  NOTE: host did not complete — interrupt may have fired before host call.");
            println!("  Downgraded to MITIGATED: interrupt fires at JS step boundaries (may precede host call).");
        }
    }
    println!();
}

/// Step 7a: Fresh context cost.
/// Uses Context::base() + Eval — same restricted footprint as production experiments.
fn test_fresh_context_cost() {
    println!("--- Step 7a: Fresh context cost ---");
    let n = 100u128;
    let rt = Runtime::new().expect("runtime");

    let start = Instant::now();
    for _ in 0..n {
        let ctx = Context::builder()
            .with::<intrinsic::Eval>()
            .build(&rt)
            .expect("context");
        ctx.with(|ctx| {
            let _: i32 = ctx.eval("1 + 1").expect("eval");
        });
        drop(ctx);
    }
    let elapsed = start.elapsed();
    let per_ctx_us = elapsed.as_micros() / n;
    println!(
        "RESULT: {} contexts in {:?} = {} µs/context",
        n, elapsed, per_ctx_us
    );
    if per_ctx_us < 5000 {
        println!("PASS: well under 5ms budget ({} µs)", per_ctx_us);
    } else {
        println!("CONCERN: {} µs/context exceeds 5ms target", per_ctx_us);
    }
    println!();
}

fn test_binary_footprint_note() {
    println!("--- Step 7b: Binary footprint ---");
    println!("(check `ls -lh target/release/spike-sandbox-executor` after `cargo build --release`)");
    println!();
}
