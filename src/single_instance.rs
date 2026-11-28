//! One copy of Abyss per profile. Two copies on the same data each keep their
//! own picture of the ratings, the queue and the settings, and the one that
//! saves last wins: a second launch, or an installer starting the new version
//! while the old one is still closing, could undo what the other just saved.
//!
//! The first copy holds a named mutex (one per data folder, so test runs with
//! their own `ABYSS_CONFIG_DIR` are not affected). A later launch waits a few
//! seconds for it (an old copy that is closing), then brings the running
//! window to the front and exits.

/// Held for the whole life of the process; dropping it lets the next copy start.
pub struct Instance {
    #[cfg(windows)]
    _handle: isize,
}

/// How long a new launch waits for a copy that is closing.
#[cfg(windows)]
const PATIENCE_MS: u32 = 5000;

/// `Some` when this copy may run, `None` when another one is running (its
/// window has been brought to the front).
pub fn acquire(data_dir: &std::path::Path) -> Option<Instance> {
    #[cfg(windows)]
    {
        native::acquire(data_dir)
    }
    #[cfg(not(windows))]
    {
        let _ = data_dir;
        Some(Instance {})
    }
}

/// The mutex name for a data folder: stable, case-insensitive, no backslashes.
pub fn mutex_name(data_dir: &std::path::Path) -> String {
    // FNV-1a over the lower-cased path.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data_dir.to_string_lossy().to_lowercase().bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("Local\\Abyss-{hash:016x}")
}

#[cfg(windows)]
mod native {
    use super::{mutex_name, Instance, PATIENCE_MS};

    const ERROR_ALREADY_EXISTS: u32 = 183;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_ABANDONED: u32 = 0x80;
    const SW_RESTORE: i32 = 9;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateMutexW(attributes: *const core::ffi::c_void, initial_owner: i32, name: *const u16) -> isize;
        fn GetLastError() -> u32;
        fn WaitForSingleObject(handle: isize, millis: u32) -> u32;
        fn CloseHandle(handle: isize) -> i32;
        fn GetCurrentProcessId() -> u32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn QueryFullProcessImageNameW(process: isize, flags: u32, name: *mut u16, size: *mut u32) -> i32;
    }
    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(callback: unsafe extern "system" fn(isize, isize) -> i32, param: isize) -> i32;
        fn GetWindowTextW(hwnd: isize, text: *mut u16, max: i32) -> i32;
        fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
        fn IsWindowVisible(hwnd: isize) -> i32;
        fn IsIconic(hwnd: isize) -> i32;
        fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
        fn SetForegroundWindow(hwnd: isize) -> i32;
    }

    pub fn acquire(data_dir: &std::path::Path) -> Option<Instance> {
        let name: Vec<u16> = mutex_name(data_dir).encode_utf16().chain(Some(0)).collect();
        // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
        if handle == 0 {
            // No mutex (very unusual): better to run than to refuse.
            log::warn!("single instance: could not create the mutex");
            return Some(Instance { _handle: 0 });
        }
        // SAFETY: plain thread-local error code of the call above.
        if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
            return Some(Instance { _handle: handle });
        }
        // Another copy holds it: wait for it if it is closing.
        // SAFETY: `handle` is our open handle to the named mutex.
        match unsafe { WaitForSingleObject(handle, PATIENCE_MS) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => {
                log::info!("single instance: the previous copy closed, starting");
                Some(Instance { _handle: handle })
            }
            _ => {
                log::info!("single instance: another copy is running, bringing it to the front");
                focus_other();
                // SAFETY: closing our own handle once.
                unsafe { CloseHandle(handle) };
                None
            }
        }
    }

    /// Restore and bring forward the other Abyss window.
    fn focus_other() {
        if cfg!(test) {
            // Tests must not raise the user's own Abyss window.
            return;
        }
        unsafe extern "system" fn visit(hwnd: isize, found: isize) -> i32 {
            // SAFETY: `found` is the `&mut isize` passed to EnumWindows below.
            let found = unsafe { &mut *(found as *mut isize) };
            let mut pid = 0u32;
            let mut title = [0u16; 64];
            // SAFETY: `hwnd` comes from EnumWindows; the buffers outlive the calls.
            let (visible, len, own) = unsafe {
                GetWindowThreadProcessId(hwnd, &mut pid);
                (IsWindowVisible(hwnd) != 0, GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32), GetCurrentProcessId())
            };
            if !visible || pid == own || String::from_utf16_lossy(&title[..len.max(0) as usize]) != crate::i18n::t("app.name") {
                return 1;
            }
            if !is_abyss(pid) {
                return 1;
            }
            *found = hwnd;
            0
        }
        let mut found: isize = 0;
        // SAFETY: the callback only reads window data and writes `found`, which outlives the call.
        unsafe { EnumWindows(visit, &mut found as *mut isize as isize) };
        if found != 0 {
            // SAFETY: `found` is a live top-level window of the other copy.
            unsafe {
                if IsIconic(found) != 0 {
                    ShowWindow(found, SW_RESTORE);
                }
                SetForegroundWindow(found);
            }
        }
    }

    /// Whether process `pid` runs an executable called Abyss.exe.
    fn is_abyss(pid: u32) -> bool {
        // SAFETY: querying another process's image name with a limited right; the handle is closed.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process == 0 {
                return false;
            }
            let mut buf = [0u16; 1024];
            let mut size = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut size) != 0;
            CloseHandle(process);
            ok && String::from_utf16_lossy(&buf[..size as usize]).to_lowercase().ends_with("\\abyss.exe")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mutex_name_depends_on_the_folder_only() {
        let a = mutex_name(std::path::Path::new(r"C:\Users\x\AppData\Roaming\abyss"));
        assert_eq!(a, mutex_name(std::path::Path::new(r"c:\users\X\appdata\roaming\ABYSS")));
        assert_ne!(a, mutex_name(std::path::Path::new(r"C:\Temp\profile")));
        assert!(a.starts_with("Local\\Abyss-") && !a["Local\\".len()..].contains('\\'));
    }

    #[cfg(windows)]
    #[test]
    fn a_second_copy_on_the_same_folder_is_refused_and_another_folder_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let first = acquire(dir.path());
        assert!(first.is_some());
        // The same process asking again stands for a second launch: the mutex is
        // owned by this thread, so it is re-entrant here; ask from another thread.
        let path = dir.path().to_path_buf();
        let second = std::thread::spawn(move || {
            // Shorter than PATIENCE_MS would be nicer, but the refusal path is what matters.
            acquire(&path).is_some()
        })
        .join()
        .unwrap();
        assert!(!second, "a second copy on the same data must not start");
        let other = tempfile::tempdir().unwrap();
        assert!(acquire(other.path()).is_some());
    }
}
