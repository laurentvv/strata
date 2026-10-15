//! Windows shell helpers behind the context menu: open, terminal, native Properties dialog.

use std::path::{Path, PathBuf};

/// Open a file or folder with its default handler.
pub fn open(path: &Path) {
    #[cfg(windows)]
    shell_execute("open", path.as_os_str(), None, None);
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

/// Open a terminal in `dir` (Windows Terminal when installed, else a console window).
pub fn open_terminal(dir: &Path) {
    #[cfg(windows)]
    {
        if std::process::Command::new("wt").arg("-d").arg(dir).spawn().is_err() {
            let _ = std::process::Command::new("cmd").args(["/c", "start", "cmd", "/k"]).current_dir(dir).spawn();
        }
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("x-terminal-emulator").current_dir(dir).spawn();
    }
}

/// The Windows Properties dialog of the shell (security, versions, ...).
pub fn properties(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_INVOKEIDLIST, SHELLEXECUTEINFOW};
        let wide = |s: &std::ffi::OsStr| -> Vec<u16> { s.encode_wide().chain(std::iter::once(0)).collect() };
        let verb = wide(std::ffi::OsStr::new("properties"));
        let file = wide(path.as_os_str());
        // SAFETY: zeroed SHELLEXECUTEINFOW is a valid empty value; strings outlive the call.
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_INVOKEIDLIST;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.nShow = 1;
        // SAFETY: `info` is fully initialised.
        unsafe { ShellExecuteExW(&mut info) };
    }
    #[cfg(not(windows))]
    let _ = path;
}

/// Folders worth scanning first, as (English label, path): the user's own folders, the profiles of every user and the
/// temporary files. Only folders that exist; the shell knows where Documents or Desktop really are (OneDrive moves
/// them), and two entries that lead to the same place are listed once.
pub fn quick_folders() -> Vec<(&'static str, PathBuf)> {
    let mut found: Vec<(&'static str, PathBuf)> = Vec::new();
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_LocalAppData, FOLDERID_Profile, FOLDERID_UserProfiles};
        for (label, id) in [
            ("Your user folder", &FOLDERID_Profile),
            ("Downloads", &FOLDERID_Downloads),
            ("Desktop", &FOLDERID_Desktop),
            ("Documents", &FOLDERID_Documents),
            ("App data", &FOLDERID_LocalAppData),
            ("All users", &FOLDERID_UserProfiles),
        ] {
            if let Some(p) = known_folder(id) {
                found.push((label, p));
            }
        }
    }
    #[cfg(not(windows))]
    if let Some(home) = std::env::var_os("HOME") {
        found.push(("Your user folder", PathBuf::from(home)));
    }
    found.push(("Temporary files", std::env::temp_dir()));
    let mut seen = std::collections::HashSet::new();
    found.retain(|(_, p)| p.is_dir() && seen.insert(p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase()));
    found
}

#[cfg(windows)]
fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;
    let mut raw: *mut u16 = std::ptr::null_mut();
    // SAFETY: on success `raw` is a NUL-terminated string allocated by the shell; it is copied, then freed below.
    let hr = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut raw) };
    let path = if hr >= 0 && !raw.is_null() {
        // SAFETY: `raw` points to a NUL-terminated UTF-16 string, so every index up to the terminator is readable.
        let len = (0..).take_while(|&i| unsafe { *raw.add(i) } != 0).count();
        // SAFETY: `len` code units were just read from that same string.
        Some(PathBuf::from(std::ffi::OsString::from_wide(unsafe { std::slice::from_raw_parts(raw, len) })))
    } else {
        None
    };
    // SAFETY: the shell allocates with CoTaskMemAlloc even on failure; freeing a null pointer is allowed.
    unsafe { CoTaskMemFree(raw as *const _) };
    path
}

#[cfg(windows)]
fn shell_execute(verb: &str, file: &std::ffi::OsStr, params: Option<&str>, dir: Option<&Path>) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let wide = |s: &std::ffi::OsStr| -> Vec<u16> { s.encode_wide().chain(std::iter::once(0)).collect() };
    let v = wide(std::ffi::OsStr::new(verb));
    let f = wide(file);
    let p = params.map(|p| wide(std::ffi::OsStr::new(p)));
    let d = dir.map(|d| wide(d.as_os_str()));
    // SAFETY: all strings are NUL-terminated and outlive the call; null pointers are allowed.
    let rc = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            v.as_ptr(),
            f.as_ptr(),
            p.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
            d.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()),
            1,
        )
    } as usize;
    rc > 32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_folders_exist_are_listed_once_and_start_with_the_profile() {
        let q = quick_folders();
        assert!(!q.is_empty());
        assert!(q.iter().all(|(_, p)| p.is_dir()), "{q:?}");
        let mut paths: Vec<String> = q.iter().map(|(_, p)| p.to_string_lossy().to_lowercase()).collect();
        let n = paths.len();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), n, "no folder twice: {q:?}");
        assert_eq!(q[0].0, "Your user folder");
        assert!(q.iter().any(|(l, _)| *l == "Temporary files"));
    }
}
