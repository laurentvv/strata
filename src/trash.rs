//! Remove files and folders: to the Recycle Bin (Windows; other platforms report "unsupported") or
//! permanently. Both refuse the folders a machine cannot do without.

use std::path::Path;

/// Folders that must never be deleted by Strata, even on request: drive roots, the Windows and program
/// folders and the user's profile folders themselves (their contents are another matter).
pub fn is_protected(path: &Path) -> bool {
    if path.parent().is_none() || path.components().count() <= 2 {
        return true;
    }
    let norm = |p: &Path| p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase();
    let target = norm(path);
    ["SystemRoot", "windir", "ProgramFiles", "ProgramFiles(x86)", "ProgramData", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "PUBLIC", "HOME"]
        .iter()
        .filter_map(std::env::var_os)
        .any(|v| norm(Path::new(&v)) == target)
}

/// Delete a file or a folder with everything in it, without the Recycle Bin. Links are removed
/// themselves, never followed.
pub fn delete_permanently(path: &Path) -> Result<(), String> {
    if is_protected(path) {
        return Err("protected folder".into());
    }
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    let res = if meta.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
    res.map_err(|e| e.to_string())
}

#[cfg(windows)]
pub fn recycle(path: &std::path::Path) -> Result<(), String> {
    if is_protected(path) {
        return Err("protected folder".into());
    }
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW};
    // The source list is double-NUL terminated.
    let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
    from.extend([0, 0]);
    // SAFETY: SHFILEOPSTRUCTW is plain data (zero is a valid empty value); `from` outlives the call.
    let mut op: SHFILEOPSTRUCTW = unsafe { std::mem::zeroed() };
    op.wFunc = FO_DELETE;
    op.pFrom = from.as_ptr();
    op.fFlags = (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16;
    // SAFETY: `op` is fully initialised and `pFrom` points to a valid double-NUL terminated list.
    let rc = unsafe { SHFileOperationW(&mut op) };
    if rc != 0 {
        return Err(format!("code 0x{rc:X}"));
    }
    if op.fAnyOperationsAborted != 0 {
        return Err("aborted".into());
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn recycle(path: &std::path::Path) -> Result<(), String> {
    if is_protected(path) {
        return Err("protected folder".into());
    }
    Err("Recycle Bin is only supported on Windows".into())
}

/// (bytes, items) held by the Recycle Bin of every drive; `None` when Windows cannot say.
#[cfg(windows)]
pub fn recycle_bin_info() -> Option<(u64, u64)> {
    use windows_sys::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};
    let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, i64Size: 0, i64NumItems: 0 };
    // SAFETY: no root path means every drive; `info` is initialised with its size, as the call requires.
    let hr = unsafe { SHQueryRecycleBinW(std::ptr::null(), &mut info) };
    // The structure is packed: copy the fields out instead of borrowing them.
    let (size, items) = (info.i64Size, info.i64NumItems);
    (hr >= 0).then(|| (size.max(0) as u64, items.max(0) as u64))
}

#[cfg(not(windows))]
pub fn recycle_bin_info() -> Option<(u64, u64)> {
    None
}

/// Empty the Recycle Bin of every drive, without Windows' own confirmation (the caller has asked the user).
#[cfg(windows)]
pub fn empty_recycle_bin() -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::{SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND};
    // SAFETY: no owner window and no root path (every drive); the flags only silence Windows' own dialogs.
    let hr = unsafe { SHEmptyRecycleBinW(std::ptr::null_mut(), std::ptr::null(), SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND) };
    if hr >= 0 {
        Ok(())
    } else {
        Err(format!("code 0x{:08X}", hr as u32))
    }
}

#[cfg(not(windows))]
pub fn empty_recycle_bin() -> Result<(), String> {
    Err("Recycle Bin is only supported on Windows".into())
}

#[cfg(all(test, windows))]
mod tests {
    /// Read-only: the size query answers on a real Windows (the Recycle Bin itself is never emptied by a test).
    #[test]
    fn the_recycle_bin_size_can_be_read() {
        let info = super::recycle_bin_info();
        assert!(info.is_some(), "SHQueryRecycleBin failed");
    }

    #[test]
    fn recycles_a_folder() {
        let d = tempfile::tempdir().unwrap();
        let victim = d.path().join("victim");
        std::fs::create_dir_all(victim.join("sub")).unwrap();
        std::fs::write(victim.join("sub").join("a.txt"), b"x").unwrap();
        super::recycle(&victim).unwrap();
        assert!(!victim.exists());
    }
}

#[cfg(test)]
mod permanent_tests {
    use super::*;

    #[test]
    fn deletes_a_tree_including_read_only_files_and_leaves_siblings() {
        let d = tempfile::tempdir().unwrap();
        let victim = d.path().join("victim");
        let keep = d.path().join("keep");
        std::fs::create_dir_all(victim.join("a/b")).unwrap();
        std::fs::create_dir_all(&keep).unwrap();
        std::fs::write(victim.join("a/b/ro.txt"), b"x").unwrap();
        std::fs::write(keep.join("k.txt"), b"k").unwrap();
        let mut perm = std::fs::metadata(victim.join("a/b/ro.txt")).unwrap().permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(victim.join("a/b/ro.txt"), perm).unwrap();
        delete_permanently(&victim).unwrap();
        assert!(!victim.exists());
        assert!(keep.join("k.txt").exists(), "a sibling is untouched");
        // a single file works too, and a missing path is an error
        delete_permanently(&keep.join("k.txt")).unwrap();
        assert!(delete_permanently(&keep.join("k.txt")).is_err());
    }

    #[test]
    fn protected_folders_are_refused() {
        let root = if cfg!(windows) { Path::new("C:\\") } else { Path::new("/") };
        assert!(is_protected(root));
        assert!(delete_permanently(root).is_err());
        let home = if cfg!(windows) { std::env::var_os("USERPROFILE") } else { std::env::var_os("HOME") };
        if let Some(h) = home {
            let h = std::path::PathBuf::from(h);
            assert!(is_protected(&h), "the profile folder itself");
            assert!(!is_protected(&h.join("some").join("project").join("target")), "a folder inside it is fine");
            assert!(delete_permanently(&h).is_err());
        }
        let d = tempfile::tempdir().unwrap();
        assert!(!is_protected(&d.path().join("x")));
    }
}
