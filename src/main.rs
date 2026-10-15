// Release builds on Windows are GUI-subsystem programs (no console window); `--cli` and `--help`
// re-attach to the parent console at start-up.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod analysis;
mod cli;
mod diag;
mod export;
mod fmt;
mod i18n;
mod i18n_fr;
mod scan;
mod settings;
mod spec;
mod update;
mod shell_ext;
mod shell;
mod trash;
mod ui;

/// Re-attach stdout/stderr to the console of the parent process (needed with the GUI subsystem).
#[cfg(windows)]
fn attach_parent_console() {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::System::Console::{AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
    // SAFETY: plain Win32 calls; the handles passed to SetStdHandle stay open for the process lifetime.
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }
        for (std_handle, name) in [(STD_OUTPUT_HANDLE, "CONOUT$"), (STD_ERROR_HANDLE, "CONOUT$")] {
            let cur = GetStdHandle(std_handle);
            // Keep redirected handles (pipes, files); only fix missing ones.
            if cur.is_null() || cur as isize == -1 {
                if let Ok(f) = std::fs::OpenOptions::new().write(true).open(name) {
                    SetStdHandle(std_handle, f.into_raw_handle() as _);
                }
            }
        }
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}

const USAGE: &str = "Strata - disk space analyzer

Usage: strata [PATH]                launch the GUI (PATH is scanned at startup)
       strata --renderer auto|opengl|directx|software [PATH]
                                            how the window is drawn: auto tries OpenGL, then DirectX (which also works in virtual machines
                                            without OpenGL); software forces the DirectX software adapter
       strata --cli [PATH] [-d N] [--export FILE.csv|json|html] [--changes] [--cleanup] [--fast]
                                            headless scan: print the tree, export it, show what changed since the last scan, list reclaimable folders;
                                            --fast reads the NTFS Master File Table of a whole drive (administrator only)
       strata --version
       strata --check-update [OWNER/NAME]   ask GitHub once whether a newer release exists (a repository other than Strata's is for tests)
       strata --screenshot FILE.png [--view tree|treemap|chart|types|largest|oldest|duplicates|compare|cleanup|age|system] [--tab N] [--dialog options|about]
                      [--lang CODE] [PATH]  developer flag: scan PATH, render one frame, save a PNG, exit
       strata --simulate-start-failure      developer flag: show the message of a window that cannot be created
       strata --simulate-no-opengl          developer flag: make the OpenGL set-up fail to test the DirectX fallback";

fn main() {
    diag::install_panic_hook(diag::log_path());
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let simulate_failure = args.iter().any(|a| a == "--simulate-start-failure");
    args.retain(|a| a != "--simulate-start-failure");
    let help = args.iter().any(|a| a == "-h" || a == "--help");
    if args.iter().any(|a| a == "-V" || a == "--version") {
        attach_parent_console();
        println!("strata {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if let Some(pos) = args.iter().position(|a| a == "--check-update") {
        attach_parent_console();
        let repo = args.get(pos + 1).filter(|a| !a.starts_with('-')).map_or(update::REPO, String::as_str);
        match update::latest(repo, env!("CARGO_PKG_VERSION")) {
            Ok(outcome) => println!("{}", update::describe(&outcome, env!("CARGO_PKG_VERSION"))),
            Err(e) => {
                eprintln!("update check failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let cli_mode = args.iter().any(|a| a == "--cli");
    if help || cli_mode {
        attach_parent_console();
    }
    if help {
        println!("{USAGE}");
        return;
    }
    if let Some(pos) = args.iter().position(|a| a == "--cli") {
        cli::run(&args[pos + 1..]);
        return;
    }
    let opts = match ui::parse_args(&args) {
        Ok(o) => o,
        Err(e) => {
            attach_parent_console();
            eprintln!("{e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    let result = if simulate_failure { Err(eframe::Error::AppCreation("simulated start-up failure".into())) } else { ui::run(opts) };
    if let Err(e) = result {
        // No console with the GUI subsystem: a message box and the crash log are the only ways to tell the user.
        diag::report_start_failure(&e.to_string());
        attach_parent_console();
        eprintln!("GUI error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    /// The release workflow publishes RELEASE_NOTES.md as the text of a release: it must be about this version
    /// and must not carry a user folder.
    #[test]
    fn the_release_notes_are_about_the_current_version_and_hold_no_personal_path() {
        let notes = include_str!("../RELEASE_NOTES.md");
        let title = format!("# Strata {}", env!("CARGO_PKG_VERSION"));
        assert!(notes.starts_with(&title), "RELEASE_NOTES.md must start with '{title}'");
        for needle in ["C:\\Users", "AppData\\Roaming", "/home/"] {
            assert!(!notes.contains(needle), "RELEASE_NOTES.md mentions {needle}");
        }
    }
}
