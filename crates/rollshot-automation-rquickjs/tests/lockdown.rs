use rollshot_automation_rquickjs::LockedContext;
use rquickjs::Value;

const STRIPPED: &[&str] = &[
    "eval",
    "Function",
    "queueMicrotask",
    "globalThis",
    "Reflect",
];

#[test]
fn dangerous_base_globals_are_stripped_and_verified() {
    let locked = LockedContext::new(8 * 1024 * 1024, 256 * 1024).unwrap();
    locked.with(|ctx| {
        let globals = ctx.globals();
        for name in STRIPPED {
            let value: Value = globals.get(*name).unwrap();
            assert!(value.is_undefined(), "{name} is still present");
        }
    });
}

#[test]
fn ambient_platform_globals_are_absent() {
    let locked = LockedContext::new(8 * 1024 * 1024, 256 * 1024).unwrap();
    locked.with(|ctx| {
        let globals = ctx.globals();
        for name in [
            "fetch",
            "XMLHttpRequest",
            "WebSocket",
            "setTimeout",
            "setInterval",
            "Promise",
            "Proxy",
            "require",
            "process",
            "Deno",
            "Bun",
            "Worker",
            "document",
            "window",
        ] {
            let value: Value = globals.get(name).unwrap();
            assert!(value.is_undefined(), "{name} is unexpectedly present");
        }
    });
}

#[test]
fn fresh_runtime_marker_does_not_leak_across_instances() {
    {
        let locked_a = LockedContext::new(8 * 1024 * 1024, 256 * 1024).unwrap();
        locked_a.with(|ctx| {
            ctx.globals().set("__test_marker__", 42_i32).unwrap();
            let v: i32 = ctx.globals().get("__test_marker__").unwrap();
            assert_eq!(v, 42);
        });
    }

    let locked_b = LockedContext::new(8 * 1024 * 1024, 256 * 1024).unwrap();
    locked_b.with(|ctx| {
        let value: Value = ctx.globals().get("__test_marker__").unwrap();
        assert!(
            value.is_undefined(),
            "marker leaked from one LockedContext to another"
        );
    });
}

#[test]
fn prototype_mutation_does_not_leak_across_instances() {
    {
        let locked_a = LockedContext::new(8 * 1024 * 1024, 256 * 1024).unwrap();
        locked_a.with(|ctx| {
            ctx.globals().set("eval", rquickjs::Undefined).unwrap();
        });
    }

    let locked_b = LockedContext::new(8 * 1024 * 1024, 256 * 1024).unwrap();
    locked_b.with(|ctx| {
        let value: Value = ctx.globals().get("eval").unwrap();
        assert!(
            value.is_undefined(),
            "eval should be stripped in fresh LockedContext regardless of prior state"
        );
    });
}
