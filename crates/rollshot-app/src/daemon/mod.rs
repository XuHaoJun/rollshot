pub mod config;
pub mod core;
pub mod instance;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub mod process;

pub fn run() -> Result<(), String> {
    let lock_path = instance::lock_path()?;
    let acquired = instance::acquire_at(&lock_path)?;
    run_if_primary(acquired, run_primary)
}

fn run_if_primary(
    acquired: instance::AcquireResult,
    start: impl FnOnce(instance::InstanceGuard) -> Result<(), String>,
) -> Result<(), String> {
    match acquired {
        instance::AcquireResult::Acquired(guard) => start(guard),
        instance::AcquireResult::AlreadyRunning => {
            tracing::info!(
                target: "rollshot::daemon::instance",
                "Rollshot daemon is already running"
            );
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
fn run_primary(_instance: instance::InstanceGuard) -> Result<(), String> {
    let config_path = config::config_path()?;
    let loaded = config::load_from(&config_path, config::Platform::Linux);
    if let Some(warning) = loaded.warning {
        tracing::warn!(
            target: "rollshot::daemon::config",
            %warning,
            "using default daemon configuration"
        );
    }

    let executable = std::env::current_exe()
        .map_err(|error| format!("failed to resolve Rollshot executable: {error}"))?;
    let (events, receiver) = std::sync::mpsc::channel();
    let _platform = linux::LinuxPlatform::start(events.clone(), &loaded.config)?;
    let launcher = process::CurrentExeLauncher::new(executable);
    let mut core = core::DaemonCore::new(launcher, events);

    tracing::info!(
        target: "rollshot::daemon::core",
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        preferred_shortcut = %loaded.config.capture_region_hotkey,
        "Rollshot tray daemon ready; portal shortcut setup runs asynchronously"
    );
    while let Ok(event) = receiver.recv() {
        if core.handle(event) == core::LoopAction::Exit {
            return Ok(());
        }
    }
    Err("daemon event channel closed unexpectedly".into())
}

#[cfg(not(target_os = "linux"))]
fn run_primary(_instance: instance::InstanceGuard) -> Result<(), String> {
    Err("daemon mode is not implemented on this platform yet".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_instance_exits_successfully_without_starting_platform() {
        let started = std::cell::Cell::new(false);

        let result = run_if_primary(instance::AcquireResult::AlreadyRunning, |_guard| {
            started.set(true);
            Ok(())
        });

        assert!(result.is_ok());
        assert!(!started.get());
    }
}
