use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use rollshot_automation::{
    AutomationHost, AutomationInput, CapabilityError, CapabilityName, ExecutionPolicy,
};
use rquickjs::{Ctx, Function, Object, Value};

thread_local! {
    static BRIDGE_STATES: RefCell<HashMap<u64, *mut BridgeStateInner>> = RefCell::new(HashMap::new());
}

static NEXT_BRIDGE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Inner state for the bridge, stored in thread-local storage via raw pointer.
///
/// # Safety
/// `host` and `policy` are raw pointers that are valid for the duration of
/// the `execute` call. `BridgeGuard` owns the allocation (via `Box::into_raw`)
/// and frees it in `Drop` after removing the TLS entry.
pub(crate) struct BridgeStateInner {
    host: *mut dyn AutomationHost,
    policy: *const ExecutionPolicy,
    pub capability_calls: u32,
    pub calls_by_capability: BTreeMap<CapabilityName, u32>,
    pub host_allocation_bytes: usize,
    pub pending_error: Option<CapabilityError>,
}

/// RAII guard that registers/unregisters bridge state in thread-local storage.
pub(crate) struct BridgeGuard {
    id: u64,
    ptr: *mut BridgeStateInner,
}

impl BridgeGuard {
    pub fn new(host: &mut dyn AutomationHost, policy: &ExecutionPolicy) -> Self {
        let id = NEXT_BRIDGE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // SAFETY: `host` and `policy` are valid for the duration of `execute`.
        // The raw pointers are coerced with their lifetimes erased to 'static
        // because `Function::new` closures require 'static captures. The
        // `BridgeGuard` is dropped (and unregistered) before `execute` returns,
        // so the pointers never dangle.
        let host_ptr: *mut (dyn AutomationHost + 'static) = unsafe {
            std::mem::transmute::<*mut dyn AutomationHost, *mut (dyn AutomationHost + 'static)>(
                host as *mut dyn AutomationHost,
            )
        };
        let policy_ptr: *const ExecutionPolicy = policy as *const ExecutionPolicy;

        let inner = Box::new(BridgeStateInner {
            host: host_ptr,
            policy: policy_ptr,
            capability_calls: 0,
            calls_by_capability: BTreeMap::new(),
            host_allocation_bytes: 0,
            pending_error: None,
        });
        let ptr = Box::into_raw(inner);
        Self { id, ptr }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn register(&self) {
        BRIDGE_STATES.with(|states| {
            states.borrow_mut().insert(self.id, self.ptr);
        });
    }

    pub fn inner(&self) -> &BridgeStateInner {
        // SAFETY: ptr comes from Box::into_raw in `new()`, the guard owns it,
        // and it is only accessed through this guard during its lifetime.
        unsafe { &*self.ptr }
    }

    pub fn inner_mut(&mut self) -> &mut BridgeStateInner {
        // SAFETY: same as inner() — exclusive access via &mut self.
        unsafe { &mut *self.ptr }
    }
}

impl Drop for BridgeGuard {
    fn drop(&mut self) {
        BRIDGE_STATES.with(|states| {
            states.borrow_mut().remove(&self.id);
        });
        // SAFETY: ptr was created via Box::into_raw in `new()` and is only
        // freed here. The TLS entry was just removed, so no dangling refs.
        drop(unsafe { Box::from_raw(self.ptr) });
    }
}

fn with_state<F, R>(id: u64, f: F) -> R
where
    F: FnOnce(&mut BridgeStateInner) -> R,
{
    BRIDGE_STATES.with(|states| {
        let ptr = *states
            .borrow()
            .get(&id)
            .expect("bridge state not registered");
        // SAFETY: ptr was inserted by `register()` and points to a valid
        // BridgeStateInner allocated via Box::into_raw. The guard removes the
        // TLS entry in its Drop before freeing the allocation.
        let state = unsafe { &mut *ptr };
        f(state)
    })
}

pub(crate) fn install_input<'js>(
    ctx: &Ctx<'js>,
    input: &AutomationInput,
) -> rquickjs::Result<Value<'js>> {
    let json = serde_json::to_string(input)
        .map_err(|error| rquickjs::Error::new_from_js_message("rust", "json", error.to_string()))?;
    let value: Value = ctx.json_parse(json)?;
    deep_freeze(ctx, value.clone())?;
    ctx.globals().set("input", value.clone())?;
    Ok(value)
}

pub(crate) fn deep_freeze<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> rquickjs::Result<()> {
    if let Some(object) = value.as_object() {
        let keys = object
            .keys::<String>()
            .collect::<rquickjs::Result<Vec<_>>>()?;
        for key in keys {
            let child: Value = object.get(key)?;
            deep_freeze(ctx, child)?;
        }
        let object_constructor: Object = ctx.globals().get("Object")?;
        let freeze: Function = object_constructor.get("freeze")?;
        freeze.call::<_, ()>((object.clone(),))?;
    }
    Ok(())
}

fn store_error_and_throw(ctx: &Ctx<'_>, id: u64, error: CapabilityError) -> rquickjs::Error {
    with_state(id, |s| s.pending_error = Some(error));
    rquickjs::Exception::throw_message(ctx, "capability_rejected")
}

fn charge_host_allocation(id: u64, bytes: usize) -> Result<(), CapabilityError> {
    with_state(id, |s| {
        s.host_allocation_bytes += bytes;
        // SAFETY: policy pointer is valid for the duration of execute.
        let limit = unsafe { &*s.policy }.max_host_allocation_bytes;
        if s.host_allocation_bytes > limit {
            Err(CapabilityError::LimitExceeded)
        } else {
            Ok(())
        }
    })
}

fn charge_capability_calls(id: u64, capability: CapabilityName) -> Result<(), CapabilityError> {
    with_state(id, |s| {
        s.capability_calls += 1;
        // SAFETY: policy pointer is valid for the duration of execute.
        let policy = unsafe { &*s.policy };
        if s.capability_calls > policy.max_capability_calls {
            return Err(CapabilityError::LimitExceeded);
        }
        let count = s.calls_by_capability.entry(capability).or_insert(0);
        *count += 1;
        let current = *count;
        let cap = policy.max_calls_by_capability.get(&capability).copied();
        if let Some(cap) = cap {
            if current > cap {
                return Err(CapabilityError::LimitExceeded);
            }
        }
        Ok(())
    })
}

fn js_object_to_json(obj: &Object<'_>) -> rquickjs::Result<String> {
    let ctx = obj.ctx().clone();
    let value: Value<'_> = obj.clone().into_value();
    let json_value = ctx
        .json_stringify(value)?
        .ok_or(rquickjs::Error::Exception)?;
    json_value.to_string()
}

/// Cast a Value's lifetime. This is needed because `Function::new` closures
/// have a `Ctx<'_>` whose lifetime differs from the outer `'js` lifetime,
/// but in practice they are the same QuickJS context.
///
/// # Safety
/// The value must have been created from the same QuickJS context that
/// the target lifetime refers to.
unsafe fn cast_value_lifetime<'a, 'b>(value: Value<'a>) -> Value<'b> {
    std::mem::transmute(value)
}

macro_rules! capability_fn {
    ($ctx:expr, $bridge_id:expr, $capability:expr, $query_type:ty, $host_method:ident, $validate:expr) => {{
        let bridge_id = $bridge_id;
        Function::new(
            $ctx.clone(),
            move |ctx: Ctx<'_>, query: Object<'_>| -> rquickjs::Result<Value<'_>> {
                let json_str = js_object_to_json(&query)?;
                let q: $query_type =
                    serde_json::from_str(&json_str).map_err(|_| rquickjs::Error::Exception)?;
                charge_capability_calls(bridge_id, $capability)
                    .map_err(|e| store_error_and_throw(&ctx, bridge_id, e))?;
                // SAFETY: host pointer is valid for the duration of execute.
                let result = with_state(bridge_id, |s| unsafe { &mut *s.host }.$host_method(q));
                let items = result.map_err(|e| store_error_and_throw(&ctx, bridge_id, e))?;
                for item in &items {
                    if !$validate(item) {
                        return Err(store_error_and_throw(
                            &ctx,
                            bridge_id,
                            CapabilityError::Failed {
                                code: "invalid_value",
                            },
                        ));
                    }
                }
                let json = serde_json::to_string(&items).map_err(|_| {
                    rquickjs::Error::new_from_js_message("rust", "json", "serialize")
                })?;
                charge_host_allocation(bridge_id, json.len())
                    .map_err(|e| store_error_and_throw(&ctx, bridge_id, e))?;
                // Use eval to parse JSON — this creates a Value tied to the context.
                let code = format!("JSON.parse({:?})", json);
                let parsed: Value<'_> = ctx.eval(code.as_str())?;
                deep_freeze(&ctx, parsed.clone())?;
                // SAFETY: parsed was created from the same QuickJS context as the outer 'js.
                Ok(unsafe { cast_value_lifetime(parsed) })
            },
        )
    }};
}

pub(crate) fn install_rollshot<'js>(
    ctx: &Ctx<'js>,
    bridge_id: u64,
) -> rquickjs::Result<Object<'js>> {
    let rollshot = Object::new(ctx.clone())?;

    let ocr_fn = capability_fn!(
        ctx,
        bridge_id,
        CapabilityName::Ocr,
        rollshot_automation::OcrQuery,
        ocr,
        |m: &rollshot_automation::OcrMatch| m.confidence.is_finite()
    );
    rollshot.set("ocr", ocr_fn)?;

    let layout_fn = capability_fn!(
        ctx,
        bridge_id,
        CapabilityName::Layout,
        rollshot_automation::LayoutQuery,
        layout,
        |r: &rollshot_automation::LayoutRegion| r.confidence.is_finite()
    );
    rollshot.set("layout", layout_fn)?;

    let rf_fn = capability_fn!(
        ctx,
        bridge_id,
        CapabilityName::RegionFeatures,
        rollshot_automation::RegionFeaturesQuery,
        region_features,
        |f: &rollshot_automation::RegionFeatures| f.edge_density.is_finite()
    );
    rollshot.set("regionFeatures", rf_fn)?;

    let tm_fn = capability_fn!(
        ctx,
        bridge_id,
        CapabilityName::TemplateMatch,
        rollshot_automation::TemplateMatchQuery,
        template_match,
        |t: &rollshot_automation::TemplateMatch| t.score.is_finite()
    );
    rollshot.set("templateMatch", tm_fn)?;

    let object_constructor: Object = ctx.globals().get("Object")?;
    let freeze: Function = object_constructor.get("freeze")?;
    freeze.call::<_, ()>((rollshot.clone(),))?;
    ctx.globals().set("rollshot", rollshot.clone())?;

    Ok(rollshot)
}
