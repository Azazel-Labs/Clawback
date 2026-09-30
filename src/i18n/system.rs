//! UI language preferences, rather than the locale used for dates or numbers.

#[cfg(windows)]
pub(super) fn languages() -> Vec<String> {
    use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
    let mut count = 0;
    let mut length = 0;
    // SAFETY: Output pointers are valid; a null buffer requests its required size.
    if unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &raw mut count, std::ptr::null_mut(), &raw mut length) }
        == 0
        || length == 0
    {
        return Vec::new();
    }
    let mut buffer = vec![0u16; length as usize];
    // SAFETY: The buffer contains length writable UTF-16 elements.
    if unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &raw mut count, buffer.as_mut_ptr(), &raw mut length) }
        == 0
    {
        return Vec::new();
    }
    buffer.split(|c| *c == 0).take_while(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
}

#[cfg(target_os = "macos")]
pub(super) fn languages() -> Vec<String> {
    objc2_foundation::NSLocale::preferredLanguages().iter().map(|s| s.to_string()).collect()
}

#[cfg(not(any(windows, target_os = "macos")))]
pub(super) fn languages() -> Vec<String> {
    environment_languages(|key| std::env::var(key).ok())
}

#[cfg(any(test, not(any(windows, target_os = "macos"))))]
fn environment_languages(get: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"].into_iter().find_map(|key| get(key).filter(|s| !s.is_empty()));
    let locale = locale.unwrap_or_else(|| "C".into());
    // gettext ignores LANGUAGE in the untranslated C/POSIX locale.
    if locale == "C" || locale == "POSIX" {
        return vec!["en".into()];
    }
    let mut result: Vec<_> =
        get("LANGUAGE").unwrap_or_default().split(':').filter(|s| !s.is_empty()).map(str::to_owned).collect();
    result.push(locale);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_preferences_respect_message_locale_and_language_order() {
        let values = [("LANGUAGE", "de:fr"), ("LC_ALL", ""), ("LC_MESSAGES", "fr_CA.UTF-8"), ("LANG", "en_US.UTF-8")];
        assert_eq!(
            environment_languages(|key| values.iter().find(|(k, _)| *k == key).map(|(_, v)| (*v).into())),
            ["de", "fr", "fr_CA.UTF-8"]
        );
        assert_eq!(
            environment_languages(|key| match key {
                "LC_ALL" => Some("C".into()),
                "LANGUAGE" => Some("de".into()),
                _ => None,
            }),
            ["en"]
        );
        assert_eq!(environment_languages(|_| None), ["en"]);
    }
}
