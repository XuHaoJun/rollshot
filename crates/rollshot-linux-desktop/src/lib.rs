#[cfg(target_os = "linux")]
const TARGET_SNI: &str = "rollshot::linux_desktop::sni";

#[cfg(target_os = "linux")]
pub fn sni_host_available() -> bool {
    use zbus::blocking::{Connection, Proxy};

    let Ok(connection) = Connection::session() else {
        tracing::warn!(target: TARGET_SNI, "session bus unavailable");
        return false;
    };
    for service in [
        "org.kde.StatusNotifierWatcher",
        "org.freedesktop.StatusNotifierWatcher",
    ] {
        let Ok(proxy) = Proxy::new(&connection, service, "/StatusNotifierWatcher", service) else {
            continue;
        };
        if let Ok(true) = proxy.get_property::<bool>("IsStatusNotifierHostRegistered") {
            tracing::debug!(target: TARGET_SNI, service, "SNI host registered");
            return true;
        }
    }
    tracing::warn!(target: TARGET_SNI, "registered SNI host not found");
    false
}
