//! Translation: English texts are the source; French comes from `i18n_fr` (a plain table).
//!
//! `tr("Some English label")` returns the label in the active language. Labels may contain an
//! `&` accelerator marker, which is stripped (the ribbon and tooltips have no accelerators).

use std::sync::RwLock;

/// A language the interface is available in besides English.
pub struct Locale {
    pub code: &'static str,
    pub name: &'static str,
}

pub static LOCALES: &[Locale] = &[Locale { code: "fr", name: "Français" }];

static CURRENT_CODE: RwLock<String> = RwLock::new(String::new());

/// Remove `&` accelerators (`&&` is a literal ampersand) and East-Asian style `(&X)` groups.
pub fn strip_accel(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '(' && chars.get(i + 1) == Some(&'&') && chars.get(i + 2).is_some_and(|k| *k != '&' && !k.is_whitespace()) && chars.get(i + 3) == Some(&')') {
            i += 4;
            continue;
        }
        if c == '&' {
            if chars.get(i + 1) == Some(&'&') {
                out.push('&');
                i += 1;
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

pub fn find_locale(code: &str) -> Option<&'static Locale> {
    LOCALES.iter().find(|l| l.code.eq_ignore_ascii_case(code))
}

/// Make `code` the active language (unknown codes mean English) and return the one in effect.
pub fn set_language(code: &str) -> String {
    let eff = find_locale(code).map_or("en", |l| l.code).to_string();
    if let Ok(mut g) = CURRENT_CODE.write() {
        *g = eff.clone();
    }
    eff
}

pub fn current_language() -> String {
    CURRENT_CODE.read().map(|g| if g.is_empty() { "en".into() } else { g.clone() }).unwrap_or_else(|_| "en".into())
}

/// Translate an English label; English (and unknown labels) come back without accelerators.
pub fn tr(s: &str) -> String {
    if current_language() == "fr" {
        if let Some(t) = crate::i18n_fr::lookup(s) {
            return t.to_string();
        }
    }
    strip_accel(s)
}

/// Serialises the tests that depend on the process-wide language.
#[cfg(test)]
pub static LANG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Does the table of `code` know this English label? (translation audit)
#[cfg(test)]
pub fn is_translated(code: &str, s: &str) -> bool {
    match code {
        "fr" => crate::i18n_fr::lookup(s).is_some(),
        _ => true,
    }
}

/// Map a raw locale string (`fr-FR`, `fr_CA.UTF-8`, `en_US`) to a language code or "en".
pub fn match_locale(raw: &str) -> String {
    let lower = raw.trim().to_ascii_lowercase();
    let lang = lower.split(['.', '@', '-', '_']).next().unwrap_or("");
    find_locale(lang).map_or_else(|| "en".to_string(), |l| l.code.to_string())
}

/// System UI language as a language code or "en".
pub fn detect_system_language() -> String {
    #[cfg(windows)]
    {
        let mut buf = [0u16; 85];
        // SAFETY: the buffer is valid for 85 u16 and we pass its exact length.
        let n = unsafe { windows_sys::Win32::Globalization::GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
        if n > 1 {
            return match_locale(&String::from_utf16_lossy(&buf[..(n as usize - 1)]));
        }
        "en".into()
    }
    #[cfg(not(windows))]
    {
        for k in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(v) = std::env::var(k) {
                if !v.is_empty() {
                    return match_locale(&v);
                }
            }
        }
        "en".into()
    }
}

/// Settings value ("auto", "", "fr", unknown...) to the code that will be applied.
pub fn resolve_language(setting: &str) -> String {
    let s = setting.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("auto") {
        detect_system_language()
    } else {
        match_locale(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_accel_handles_ampersands() {
        assert_eq!(strip_accel("&File"), "File");
        assert_eq!(strip_accel("A&nd && more"), "And & more");
        assert_eq!(strip_accel("plain"), "plain");
        assert_eq!(strip_accel("ファイル(&F)"), "ファイル");
        assert_eq!(strip_accel("Save (&& more)"), "Save (& more)");
    }

    #[test]
    fn locales_are_matched_from_system_strings() {
        assert_eq!(match_locale("fr-FR"), "fr");
        assert_eq!(match_locale("fr_CA.UTF-8"), "fr");
        assert_eq!(match_locale("en-US"), "en");
        assert_eq!(match_locale("de-DE"), "en", "unsupported languages fall back to English");
        assert_eq!(match_locale(""), "en");
        assert_eq!(resolve_language("fr"), "fr");
        assert_eq!(resolve_language("zz"), "en");
    }

    #[test]
    fn tr_uses_the_active_language() {
        // The language is global: tests that read or change it hold `LANG_LOCK`.
        let _guard = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_language("fr");
        assert_eq!(tr("&Refresh"), "Actualiser");
        assert_eq!(tr("not in the table"), "not in the table");
        set_language("en");
        assert_eq!(tr("&Refresh"), "Refresh");
        assert_eq!(current_language(), "en");
    }
}
