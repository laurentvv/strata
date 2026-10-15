//! Support tools for when something goes wrong: a plain-language message when the window cannot be
//! created, a crash log, and a block of diagnostics to paste into a bug report. The reports hold the
//! version, the Windows build and the graphics driver, never a user name or a folder of the user.

use crate::i18n::tr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Set once the GUI is running: a panic of the main thread then shows a message box.
static GUI_RUNNING: AtomicBool = AtomicBool::new(false);
/// "OpenGL version | renderer", known once the window exists.
static GRAPHICS: OnceLock<String> = OnceLock::new();

/// A log older than this is started afresh.
const MAX_LOG_BYTES: u64 = 256 * 1024;

pub fn set_gui_running() {
    GUI_RUNNING.store(true, Ordering::Relaxed);
}

pub fn set_graphics(text: String) {
    let _ = GRAPHICS.set(text);
}

/// What the driver says about itself, for the diagnostics.
pub fn graphics_text(gl: &eframe::glow::Context) -> String {
    use eframe::glow::{HasContext, RENDERER, VERSION};
    // SAFETY: plain queries on the context eframe just created, on the thread that owns it.
    unsafe { format!("OpenGL {} | {}", gl.get_parameter_string(VERSION), gl.get_parameter_string(RENDERER)) }
}

/// `crash.log` in the settings folder (beside the program in portable mode).
pub fn log_path() -> Option<PathBuf> {
    crate::settings::config_dir().map(|d| d.join("crash.log"))
}

#[cfg(windows)]
fn os_version() -> String {
    use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
    // SAFETY: the structure is zeroed with its size set, as RtlGetVersion requires.
    let mut v: OSVERSIONINFOW = unsafe { std::mem::zeroed() };
    v.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;
    // SAFETY: `v` is valid for writes for the whole call.
    if unsafe { RtlGetVersion(&mut v) } == 0 {
        format!("Windows {}.{}.{}", v.dwMajorVersion, v.dwMinorVersion, v.dwBuildNumber)
    } else {
        "Windows".to_string()
    }
}

#[cfg(not(windows))]
fn os_version() -> String {
    std::env::consts::OS.to_string()
}

/// Version, system, rights and graphics of this run, ready to paste into a bug report.
pub fn diagnostics() -> String {
    let portable = std::env::current_exe().ok().and_then(|e| crate::settings::portable_dir(&e)).is_some();
    diagnostics_text(env!("CARGO_PKG_VERSION"), &os_version(), std::env::consts::ARCH, crate::scan::mft::is_elevated(), portable, GRAPHICS.get().map(String::as_str))
}

fn diagnostics_text(version: &str, os: &str, arch: &str, elevated: bool, portable: bool, graphics: Option<&str>) -> String {
    let yes_no = |b: bool| if b { "yes" } else { "no" };
    format!(
        "Strata {version}\nSystem: {os} ({arch})\nAdministrator: {}\nPortable: {}\nGraphics: {}\n",
        yes_no(elevated),
        yes_no(portable),
        graphics.unwrap_or("not initialised")
    )
}

/// One entry of the crash log.
fn crash_report(diagnostics: &str, thread: &str, message: &str, location: &str) -> String {
    format!("=== {} ===\n{diagnostics}Thread: {thread}\nWhat: {message}\nWhere: {location}\n\n", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"))
}

/// Add `text` to the log at `path` (created with its folder when missing; a log over 256 KiB starts afresh).
pub fn append_report(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        let _ = std::fs::remove_file(path);
    }
    std::fs::OpenOptions::new().create(true).append(true).open(path)?.write_all(text.as_bytes())
}

/// A source location without anything that names the user: `C:\Users\me\.cargo\registry\src\index-x\egui-0.36\src\a.rs`
/// becomes `egui-0.36/src/a.rs`, any other absolute path keeps its last part (or what follows `library/`).
fn scrub_location(file: &str) -> String {
    let norm = file.replace('\\', "/");
    if let Some(i) = norm.find("/registry/src/") {
        let rest = &norm[i + "/registry/src/".len()..];
        return rest.split_once('/').map_or(rest, |(_, r)| r).to_string();
    }
    let absolute = norm.starts_with('/') || norm.get(1..3) == Some(":/");
    if absolute {
        if let Some(i) = norm.find("/library/") {
            return norm[i + 1..].to_string();
        }
        return norm.rsplit('/').next().unwrap_or_default().to_string();
    }
    norm
}

/// Build the report of a panic and add it to the log; returns the report.
fn record_panic(log: Option<&Path>, thread: &str, message: &str, file: &str, line: u32) -> String {
    let report = crash_report(&diagnostics(), thread, message, &format!("{}:{line}", scrub_location(file)));
    if let Some(p) = log {
        let _ = append_report(p, &report);
    }
    report
}

/// Write every panic to the crash log; a panic of the GUI's main thread also says so in a message box
/// (the program has no console to print it on).
pub fn install_panic_hook(log: Option<PathBuf>) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed");
        let payload = info.payload();
        let message = payload.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_else(|| "(no message)".to_string());
        let (file, line) = info.location().map_or(("?", 0), |l| (l.file(), l.line()));
        record_panic(log.as_deref(), name, &message, file, line);
        previous(info);
        if name == "main" && GUI_RUNNING.load(Ordering::Relaxed) {
            show_error(&crash_message(log.as_deref()));
        }
    }));
}

fn crash_message(log: Option<&Path>) -> String {
    let mut m = tr("Strata stopped unexpectedly.");
    if let Some(p) = log {
        m.push_str(&format!("\n\n{} {}\n{}", tr("A report was saved in"), p.display(), tr("Please check that it holds nothing private before attaching it to a bug report.")));
    }
    m
}

fn start_failure_message(error: &str, log: Option<&Path>) -> String {
    let mut m = format!("{}\n\n{error}\n\n{}\n\n{}", tr("Strata could not open its window."), tr("This usually means the graphics driver does not provide OpenGL, as in a remote desktop session, some virtual machines or with a very old driver. Update the graphics driver, or start Strata on the computer's own screen."), tr("The command-line mode needs no window: strata --cli PATH"));
    if let Some(p) = log {
        m.push_str(&format!("\n\n{} {}", tr("Details were saved in"), p.display()));
    }
    m
}

/// The window could not be created: say so (the program has no console) and keep the details in the log.
pub fn report_start_failure(error: &str) {
    let st = crate::settings::default_path().as_deref().map(crate::settings::load_from).unwrap_or_default();
    crate::i18n::set_language(&crate::i18n::resolve_language(&st.language));
    let log = log_path();
    if let Some(p) = &log {
        let _ = append_report(p, &crash_report(&diagnostics(), "main", error, "window creation"));
    }
    show_error(&start_failure_message(error, log.as_deref()));
}

#[cfg(windows)]
pub fn show_error(message: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let wide = |s: &str| std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (text, title) = (wide(message), wide("Strata"));
    // SAFETY: both buffers are NUL-terminated and outlive the call; there is no owner window.
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

#[cfg(not(windows))]
pub fn show_error(message: &str) {
    eprintln!("{message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_name_the_version_system_rights_and_graphics() {
        let t = diagnostics_text("0.1.0", "Windows 10.0.26200", "x86_64", false, true, Some("OpenGL 4.6 | Some GPU"));
        assert_eq!(t, "Strata 0.1.0\nSystem: Windows 10.0.26200 (x86_64)\nAdministrator: no\nPortable: yes\nGraphics: OpenGL 4.6 | Some GPU\n");
        assert!(diagnostics_text("0.1.0", "x", "y", true, false, None).contains("Graphics: not initialised"));
        let live = diagnostics();
        assert!(live.starts_with(&format!("Strata {}", env!("CARGO_PKG_VERSION"))), "{live}");
        for var in ["USERNAME", "USER"] {
            if let Some(name) = std::env::var_os(var).map(|v| v.to_string_lossy().into_owned()).filter(|n| n.len() > 3) {
                assert!(!live.to_lowercase().contains(&name.to_lowercase()), "the diagnostics must not hold the user name");
            }
        }
    }

    #[test]
    fn locations_never_carry_a_user_folder() {
        assert_eq!(scrub_location(r"C:\Users\me\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\egui-0.36.2\src\a.rs"), "egui-0.36.2/src/a.rs");
        assert_eq!(scrub_location("/rustc/abc123/library/core/src/option.rs"), "library/core/src/option.rs");
        assert_eq!(scrub_location(r"C:\Users\me\work\strata\src\main.rs"), "main.rs");
        assert_eq!(scrub_location(r"src\ui\app.rs"), "src/ui/app.rs");
    }

    #[test]
    fn the_log_is_created_appended_to_and_restarted_when_too_big() {
        let d = tempfile::tempdir().unwrap();
        let log = d.path().join("sub/crash.log");
        append_report(&log, "first\n").unwrap();
        append_report(&log, "second\n").unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "first\nsecond\n");
        std::fs::write(&log, vec![b'x'; MAX_LOG_BYTES as usize + 1]).unwrap();
        append_report(&log, "fresh\n").unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "fresh\n");
    }

    #[test]
    fn a_panic_report_has_the_thread_message_and_a_clean_location() {
        let d = tempfile::tempdir().unwrap();
        let log = d.path().join("crash.log");
        let r = record_panic(Some(&log), "scan-worker", "index out of bounds", r"C:\Users\me\.cargo\registry\src\i-1\rayon-1.12.0\src\x.rs", 42);
        for part in ["Strata ", "Thread: scan-worker", "What: index out of bounds", "Where: rayon-1.12.0/src/x.rs:42"] {
            assert!(r.contains(part), "{part} in {r}");
        }
        assert!(!r.contains("Users"));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), r, "the same text goes to the log");
    }

    #[test]
    fn start_failure_message_says_what_happened_what_to_do_and_where_the_details_are() {
        let m = start_failure_message("NoGlutinConfigs", Some(Path::new("crash.log")));
        for part in ["could not open its window", "NoGlutinConfigs", "graphics driver", "--cli", "crash.log"] {
            assert!(m.contains(part), "{part} in {m}");
        }
        assert!(crash_message(None).contains("stopped unexpectedly"));
        assert!(crash_message(Some(Path::new("crash.log"))).contains("crash.log"));
    }

    #[test]
    fn the_support_texts_are_translated() {
        for t in [
            "Strata could not open its window.",
            "This usually means the graphics driver does not provide OpenGL, as in a remote desktop session, some virtual machines or with a very old driver. Update the graphics driver, or start Strata on the computer's own screen.",
            "The command-line mode needs no window: strata --cli PATH",
            "Details were saved in",
            "Strata stopped unexpectedly.",
            "A report was saved in",
            "Please check that it holds nothing private before attaching it to a bug report.",
            "Copy diagnostics",
            "Copy the version, Windows build and graphics driver to paste into a bug report.",
            "Copied",
        ] {
            assert!(crate::i18n::is_translated("fr", t), "no French for: {t}");
        }
    }
}
