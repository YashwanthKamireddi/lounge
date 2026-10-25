/// webOS sends a launched app's stderr to /dev/null. When `/tmp/luma-debug`
/// exists (developer use only), send it to `/tmp/luma.log` instead.
pub(crate) fn debug_log() {
    #[cfg(feature = "webos")]
    if std::path::Path::new("/tmp/luma-debug").exists()
        && let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/luma.log")
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
