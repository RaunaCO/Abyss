//! Embedded UI catalogs. Keys and persisted service values never depend on language.
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    OnceLock,
};

pub const LANGUAGES: [(&str, &str); 6] =
    [("en", "English"), ("es", "Español"), ("pt", "Português"), ("fr", "Français"), ("de", "Deutsch"), ("ja", "日本語")];
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static CATALOGS: OnceLock<Vec<HashMap<String, String>>> = OnceLock::new();

fn catalogs() -> &'static [HashMap<String, String>] {
    CATALOGS.get_or_init(|| {
        [
            include_str!("en.json"),
            include_str!("es.json"),
            include_str!("pt.json"),
            include_str!("fr.json"),
            include_str!("de.json"),
            include_str!("ja.json"),
        ]
        .iter()
        .map(|s| serde_json::from_str(s).expect("valid embedded language catalog"))
        .collect()
    })
}

fn language_index(code: &str) -> Option<usize> {
    let base = code.split(['-', '_', '.']).next().unwrap_or("").to_ascii_lowercase();
    LANGUAGES.iter().position(|(id, _)| *id == base)
}

pub fn system_language() -> String {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetUserDefaultUILanguage() -> u16;
            fn LCIDToLocaleName(locale: u32, name: *mut u16, count: i32, flags: u32) -> i32;
        }
        let mut name = [0u16; 85];
        let len = unsafe { LCIDToLocaleName(GetUserDefaultUILanguage() as u32, name.as_mut_ptr(), name.len() as i32, 0) };
        if len > 1 {
            return String::from_utf16_lossy(&name[..len as usize - 1]);
        }
    }
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            if !value.is_empty() {
                return value;
            }
        }
    }
    "en".into()
}

pub fn set_language(preference: Option<&str>) {
    let system = system_language();
    CURRENT.store(language_index(preference.unwrap_or(&system)).unwrap_or(0), Ordering::Relaxed);
}

/// The only lookup function. Unsupported languages and missing keys fall back to English.
/// Catalogs live for the entire process, so static text can borrow without allocating.
/// Call during rendering, never in a const initializer.
pub fn t(key: &str) -> &'static str {
    let all = catalogs();
    all[CURRENT.load(Ordering::Relaxed)].get(key).or_else(|| all[0].get(key)).map(String::as_str).unwrap_or("[missing translation]")
}

/// Insert already formatted values in one pass, so braces in user data stay literal.
/// Numbered slots let translators reorder arguments without changing code.
pub fn text(key: &str, values: &[String]) -> String {
    let template = t(key);
    let mut result = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        if let Some(end) = tail.find('}') {
            if let Ok(index) = tail[..end].parse::<usize>() {
                if let Some(value) = values.get(index) {
                    result.push_str(value);
                    rest = &tail[end + 1..];
                    continue;
                }
            }
        }
        result.push('{');
        rest = tail;
    }
    result.push_str(rest);
    result
}
