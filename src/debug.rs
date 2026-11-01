/// Whether this TV asked for Lounge's diagnostics: the status log, the app history and the
/// frame logs. Off on every TV unless `/var/lib/lounge/debug` (kept across power-offs) or
/// `/tmp/lounge-debug` (until the next one) exists, so by default Lounge writes no log files at
/// all. Read once, at start.
pub(crate) fn enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        cfg!(feature = "webos")
            && ["/var/lib/lounge/debug", "/tmp/lounge-debug"]
                .iter()
                .any(|flag| std::path::Path::new(flag).exists())
    })
}

/// webOS sends a launched app's stderr to /dev/null. With diagnostics on, send it to
/// `/tmp/lounge.log` instead.
pub(crate) fn debug_log() {
    #[cfg(feature = "webos")]
    if enabled()
        && let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/lounge.log")
    {
        use std::os::fd::AsRawFd;
        // SAFETY: dup2 onto the process's own stderr descriptor; `file`
        // stays valid for the call and the duplicate outlives it.
        unsafe { libc::dup2(file.as_raw_fd(), 2) };
    }
}

/// Major page faults so far: memory this process had to read back from swap.
pub(crate) fn major_faults() -> u64 {
    std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|stat| {
            // Fields after the command name (which may hold spaces); majflt
            // is the 12th field overall, the 10th after the name's ')'.
            let rest = &stat[stat.rfind(')')? + 2..];
            rest.split(' ').nth(9)?.parse().ok()
        })
        .unwrap_or(0)
}
