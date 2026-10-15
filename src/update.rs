//! Optional check for a newer release. Nothing here runs unless the user turned it on in Options or asked with
//! `strata --check-update`: one HTTPS request to the public GitHub API (the program's name and version are all
//! it says about itself), and the answer only ever leads to the release page. Nothing is downloaded or installed.

/// The repository whose releases are checked.
pub const REPO: &str = "laurentvv/strata";

/// A background check is not repeated within this many seconds.
const MIN_INTERVAL_SECS: i64 = 24 * 60 * 60;

/// A newer release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// "0.2.0" (without the leading v).
    pub version: String,
    /// Address of its page on GitHub.
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    UpToDate,
    /// Nothing has been published yet.
    NoRelease,
    Available(Release),
}

/// "v1.2.3", "1.2.3" or "1.2" give (1, 2, 3); anything else (a pre-release suffix, words) is not a version.
pub fn parse_version(tag: &str) -> Option<(u64, u64, u64)> {
    let t = tag.trim().trim_start_matches(['v', 'V']);
    let mut parts = t.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().map_or(Some(0), |p| p.parse().ok())?;
    let patch = parts.next().map_or(Some(0), |p| p.parse().ok())?;
    parts.next().is_none().then_some((major, minor, patch))
}

/// Is a background check due, `last` being the time of the previous one (0 = never)? A clock set back counts as due.
pub fn due(now: i64, last: i64) -> bool {
    last <= 0 || now < last || now - last >= MIN_INTERVAL_SECS
}

/// `owner/name` made of letters, digits, `-`, `_` and `.` only: it ends up in a web address.
fn valid_repo(repo: &str) -> bool {
    let ok = |s: &str| !s.is_empty() && s != "." && s != ".." && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    repo.split_once('/').is_some_and(|(owner, name)| ok(owner) && ok(name) && !name.contains('/'))
}

/// What GitHub's "latest release" answer means for the running `current` version.
pub fn interpret(status: u16, body: &[u8], repo: &str, current: &str) -> Result<Outcome, String> {
    match status {
        404 => return Ok(Outcome::NoRelease),
        200 => {}
        other => return Err(format!("GitHub answered {other}")),
    }
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|e| format!("unreadable answer: {e}"))?;
    if v["draft"].as_bool() == Some(true) || v["prerelease"].as_bool() == Some(true) {
        return Ok(Outcome::NoRelease);
    }
    let tag = v["tag_name"].as_str().ok_or("no release name in the answer")?;
    let url = v["html_url"].as_str().ok_or("no release page in the answer")?;
    let remote = parse_version(tag).ok_or_else(|| format!("unrecognised version '{tag}'"))?;
    let local = parse_version(current).ok_or_else(|| format!("unrecognised running version '{current}'"))?;
    if remote <= local {
        return Ok(Outcome::UpToDate);
    }
    // The page is opened in a browser: it must be a page of that repository on GitHub, whatever the answer says.
    if !url.starts_with(&format!("https://github.com/{repo}/")) {
        return Err("unexpected release address".into());
    }
    Ok(Outcome::Available(Release { version: tag.trim_start_matches(['v', 'V']).to_string(), url: url.to_string() }))
}

/// The one-line result for the command line.
pub fn describe(outcome: &Outcome, current: &str) -> String {
    match outcome {
        Outcome::UpToDate => format!("Strata {current} is up to date."),
        Outcome::NoRelease => "No release has been published yet.".to_string(),
        Outcome::Available(r) => format!("A newer version is available: {} (you have {current}).\n{}", r.version, r.url),
    }
}

/// Ask GitHub for the latest release of `repo` and compare it with `current`.
pub fn latest(repo: &str, current: &str) -> Result<Outcome, String> {
    if !valid_repo(repo) {
        return Err(format!("'{repo}' is not a repository name (owner/name)"));
    }
    let (status, body) = get("api.github.com", &format!("/repos/{repo}/releases/latest"))?;
    interpret(status, &body, repo, current)
}

/// HTTPS GET through WinHTTP: the system's certificates, proxy settings and TLS, and no extra library in the program.
#[cfg(windows)]
fn get(host: &str, path: &str) -> Result<(u16, Vec<u8>), String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Networking::WinHttp::*;

    /// A WinHTTP handle, closed when dropped.
    struct Handle(*mut c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the handle came from a WinHttp call and is closed once.
                unsafe { WinHttpCloseHandle(self.0) };
            }
        }
    }
    let wide = |s: &str| -> Vec<u16> { std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect() };
    let fail = |what: &str| format!("{what}: {}", std::io::Error::last_os_error());
    let agent = wide(&format!("Strata/{}", env!("CARGO_PKG_VERSION")));
    let (host_w, verb, object, headers) = (wide(host), wide("GET"), wide(path), wide("Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28"));

    // SAFETY (the whole block): every pointer passed is either null where the API allows it or points to a
    // NUL-terminated buffer or a local that outlives the call; every handle is checked before use and closed by `Handle`.
    unsafe {
        let mut session = Handle(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, null(), null(), 0));
        if session.0.is_null() {
            session = Handle(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, null(), null(), 0));
        }
        if session.0.is_null() {
            return Err(fail("cannot start the web client"));
        }
        WinHttpSetTimeouts(session.0, 5000, 5000, 5000, 8000);
        let connection = Handle(WinHttpConnect(session.0, host_w.as_ptr(), INTERNET_DEFAULT_HTTPS_PORT, 0));
        if connection.0.is_null() {
            return Err(fail(&format!("cannot reach {host}")));
        }
        let request = Handle(WinHttpOpenRequest(connection.0, verb.as_ptr(), object.as_ptr(), null(), null(), null(), WINHTTP_FLAG_SECURE));
        if request.0.is_null() {
            return Err(fail("cannot prepare the request"));
        }
        WinHttpAddRequestHeaders(request.0, headers.as_ptr(), u32::MAX, WINHTTP_ADDREQ_FLAG_ADD);
        if WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0) == 0 {
            return Err(fail(&format!("cannot reach {host}")));
        }
        if WinHttpReceiveResponse(request.0, null_mut()) == 0 {
            return Err(fail(&format!("no answer from {host}")));
        }
        let mut status: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        if WinHttpQueryHeaders(request.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, null(), &mut status as *mut u32 as *mut c_void, &mut size, null_mut()) == 0 {
            return Err(fail("unreadable answer"));
        }
        let mut body = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let mut read = 0u32;
            if WinHttpReadData(request.0, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut read) == 0 {
                return Err(fail("the answer was cut off"));
            }
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read as usize]);
            if body.len() > 256 * 1024 {
                return Err("the answer is unexpectedly large".to_string());
            }
        }
        Ok((status as u16, body))
    }
}

#[cfg(not(windows))]
fn get(_host: &str, _path: &str) -> Result<(u16, Vec<u8>), String> {
    Err("the update check is only available on Windows".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(tag: &str, url: &str, extra: &str) -> Vec<u8> {
        format!(r#"{{"tag_name":"{tag}","html_url":"{url}","name":"Strata {tag}"{extra}}}"#).into_bytes()
    }
    const PAGE: &str = "https://github.com/laurentvv/strata/releases/tag/v0.2.0";

    #[test]
    fn versions_are_read_with_or_without_v_and_anything_else_is_refused() {
        assert_eq!(parse_version("v0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("V2"), Some((2, 0, 0)));
        assert_eq!(parse_version("1.10"), Some((1, 10, 0)));
        for bad in ["", "v", "1.2.3.4", "1.2.3-beta", "latest", "1..2"] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
        assert!(parse_version("0.10.0") > parse_version("0.9.9"), "compared as numbers, not as text");
    }

    #[test]
    fn a_newer_release_is_offered_and_an_equal_or_older_one_is_not() {
        let body = answer("v0.2.0", PAGE, "");
        assert_eq!(interpret(200, &body, REPO, "0.1.0"), Ok(Outcome::Available(Release { version: "0.2.0".into(), url: PAGE.into() })));
        assert_eq!(interpret(200, &body, REPO, "0.2.0"), Ok(Outcome::UpToDate));
        assert_eq!(interpret(200, &body, REPO, "0.10.0"), Ok(Outcome::UpToDate));
        assert_eq!(interpret(200, &answer("v0.10.0", PAGE.replace("0.2.0", "0.10.0").as_str(), ""), REPO, "0.9.0"), Ok(Outcome::Available(Release { version: "0.10.0".into(), url: PAGE.replace("0.2.0", "0.10.0") })));
    }

    #[test]
    fn no_release_drafts_prereleases_and_errors_are_told_apart() {
        assert_eq!(interpret(404, b"{}", REPO, "0.1.0"), Ok(Outcome::NoRelease), "nothing published yet");
        assert_eq!(interpret(200, &answer("v9.0.0", PAGE, r#","prerelease":true"#), REPO, "0.1.0"), Ok(Outcome::NoRelease));
        assert_eq!(interpret(200, &answer("v9.0.0", PAGE, r#","draft":true"#), REPO, "0.1.0"), Ok(Outcome::NoRelease));
        assert_eq!(interpret(403, b"{}", REPO, "0.1.0"), Err("GitHub answered 403".into()), "rate limit and the like");
        assert!(interpret(200, b"not json", REPO, "0.1.0").is_err());
        assert!(interpret(200, br#"{"html_url":"x"}"#, REPO, "0.1.0").is_err());
        assert!(interpret(200, &answer("nightly", PAGE, ""), REPO, "0.1.0").is_err(), "a tag that is not a version");
        assert!(interpret(200, &answer("v0.2.0", PAGE, ""), REPO, "dev").is_err(), "an unreadable running version");
    }

    #[test]
    fn the_release_page_must_belong_to_the_repository() {
        for url in ["https://evil.example/laurentvv/strata/", "http://github.com/laurentvv/strata/releases", "https://github.com/someone/else/releases/tag/v0.2.0", "https://github.com/laurentvv/strata-fake/releases"] {
            assert_eq!(interpret(200, &answer("v0.2.0", url, ""), REPO, "0.1.0"), Err("unexpected release address".into()), "{url}");
        }
    }

    #[test]
    fn a_check_is_due_once_a_day_and_when_the_clock_went_back() {
        let day = 24 * 60 * 60;
        assert!(due(1_000, 0), "never checked");
        assert!(!due(1_000 + day - 1, 1_000));
        assert!(due(1_000 + day, 1_000));
        assert!(due(500, 1_000), "the last check is in the future");
    }

    #[test]
    fn only_owner_slash_name_is_accepted_as_a_repository() {
        assert!(valid_repo("laurentvv/strata"));
        assert!(valid_repo("rust-lang/rust"));
        for bad in ["", "strata", "a/b/c", "a/", "/b", "a b/c", "a/b?x=1", "a/b#c", "../etc", "a/../b"] {
            assert!(!valid_repo(bad), "{bad}");
        }
        assert!(latest("not a repo", "0.1.0").is_err());
    }

    #[test]
    fn the_command_line_message_says_what_to_do() {
        assert_eq!(describe(&Outcome::UpToDate, "0.1.0"), "Strata 0.1.0 is up to date.");
        assert!(describe(&Outcome::NoRelease, "0.1.0").contains("No release"));
        let m = describe(&Outcome::Available(Release { version: "0.2.0".into(), url: PAGE.into() }), "0.1.0");
        assert!(m.contains("0.2.0") && m.contains("0.1.0") && m.contains(PAGE), "{m}");
    }
}
