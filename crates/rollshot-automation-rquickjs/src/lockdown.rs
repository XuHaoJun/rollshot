use rollshot_automation::SandboxError;
use rquickjs::context::intrinsic;
use rquickjs::{Context, Runtime, Undefined, Value};

const STRIPPED_GLOBALS: &[&str] = &[
    "eval",
    "Function",
    "queueMicrotask",
    "globalThis",
    "Reflect",
];

pub struct LockedContext {
    runtime: Runtime,
    context: Context,
}

impl LockedContext {
    pub fn new(memory_bytes: usize, stack_bytes: usize) -> Result<Self, SandboxError> {
        let runtime = Runtime::new().map_err(|_| SandboxError::Initialization {
            code: "runtime_create",
        })?;
        runtime.set_memory_limit(memory_bytes);
        runtime.set_max_stack_size(stack_bytes);
        let context = Context::builder()
            .with::<intrinsic::Eval>()
            .with::<intrinsic::Json>()
            .build(&runtime)
            .map_err(|_| SandboxError::Initialization {
                code: "context_create",
            })?;

        context.with(|ctx| {
            let globals = ctx.globals();
            for name in STRIPPED_GLOBALS {
                globals
                    .set(*name, Undefined)
                    .map_err(|_| SandboxError::Initialization {
                        code: "strip_global",
                    })?;
                let value: Value =
                    globals
                        .get(*name)
                        .map_err(|_| SandboxError::Initialization {
                            code: "verify_global",
                        })?;
                if !value.is_undefined() {
                    return Err(SandboxError::Initialization {
                        code: "global_remains",
                    });
                }
            }
            Ok(())
        })?;

        Ok(Self { runtime, context })
    }

    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    pub fn with<T>(&self, callback: impl for<'js> FnOnce(rquickjs::Ctx<'js>) -> T) -> T {
        self.context.with(callback)
    }
}
