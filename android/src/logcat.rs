use std::ffi::CString;
use std::io::BufRead as _;
use std::os::fd::FromRawFd as _;

/// Android discards stdout and stderr, swallowing the tracing output and the panic message of
/// anything that fails before the log panel exists.
pub fn redirect_stdio() {
    let mut fds = [0; 2];
    // SAFETY: `pipe` fills the two-element array it is handed, and the write end is closed here
    // only after both standard streams have been duplicated onto it.
    let pipe = unsafe {
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return;
        }

        libc::dup2(fds[1], libc::STDOUT_FILENO);
        libc::dup2(fds[1], libc::STDERR_FILENO);
        libc::close(fds[1]);

        std::fs::File::from_raw_fd(fds[0])
    };

    std::mem::drop(std::thread::spawn(move || {
        for line in std::io::BufReader::new(pipe).lines() {
            let Ok(line) = line else { return };
            let Ok(text) = CString::new(line) else {
                continue;
            };

            // SAFETY: both pointers are NUL-terminated and only read for the duration of the call.
            unsafe {
                android_log_sys::__android_log_write(
                    android_log_sys::LogPriority::INFO as _,
                    c"nadir".as_ptr(),
                    text.as_ptr(),
                );
            }
        }
    }));
}
