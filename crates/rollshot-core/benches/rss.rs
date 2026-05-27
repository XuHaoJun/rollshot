//! Best-effort peak RSS measurement for the bench harness.
//!
//! - Linux: `/proc/self/status` VmRSS line.
//! - macOS: shell-out to `ps -o rss= -p <pid>` (avoids libproc bindings).
//! - Other (Windows, BSDs without procfs): returns 0 as an explicit
//!   "not measured" sentinel.
//!
//! Callers should treat 0 as "no data" rather than "0 kB resident".

#[cfg(target_os = "linux")]
pub fn read_rss_kb() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    s.lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
pub fn read_rss_kb() -> u64 {
    let pid = std::process::id();
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output();
    out.ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn read_rss_kb() -> u64 {
    0
}
