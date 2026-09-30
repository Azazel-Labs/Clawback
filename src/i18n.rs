//! Embedded Fluent catalogs with checked message calls and English fallback.
use i18n_embed::{
    LanguageLoader,
    fluent::{FluentLanguageLoader, fluent_language_loader},
};
use rust_embed::RustEmbed;
use std::sync::LazyLock;
mod system;

#[derive(RustEmbed)]
#[folder = "locales/"]
struct Localizations;

include!(concat!(env!("OUT_DIR"), "/translations.rs"));

pub static LOADER: LazyLock<FluentLanguageLoader> = LazyLock::new(|| {
    let loader = fluent_language_loader!();
    loader.load_languages(&Localizations, &[loader.fallback_language().clone()]).expect("embedded English catalog");
    loader
});

pub fn languages() -> impl Iterator<Item = &'static str> {
    LANGUAGES.iter().copied()
}

pub fn set_language(code: &str) -> &'static str {
    let preferences = if code == "auto" { system::languages() } else { vec![code.to_owned()] };
    let code = preferred_catalog(&preferences, LANGUAGES).map_or("en", |index| LANGUAGES[index]);
    LOADER
        .load_languages(&Localizations, &[code.parse().expect("validated catalog language")])
        .expect("validated embedded Fluent catalogs");
    code
}

/// Match preferences in order, allowing a regional locale to use its generic
/// catalog (fr-CA -> fr), with CLDR's explicit es-419 and pt-PT parents.
/// Never guess between sibling regional or script variants.
fn preferred_catalog(preferences: &[String], available: &[&str]) -> Option<usize> {
    for preference in preferences {
        let mut code = preference.split(['.', '@']).next().unwrap_or_default().replace('_', "-");
        if code == "C" || code == "POSIX" {
            code = "en".into();
        }
        loop {
            if let Some(index) = available.iter().position(|c| c.eq_ignore_ascii_case(&code)) {
                return Some(index);
            }
            // Region-only OS locales for Chinese. Explicit scripts
            // (zh-Hans/zh-Hant) retain priority through the normal truncation.
            if matches!(code.to_ascii_lowercase().as_str(), "zh-cn" | "zh-sg")
                && let Some(index) = available.iter().position(|c| c.eq_ignore_ascii_case("zh-Hans"))
            {
                return Some(index);
            }
            if matches!(code.to_ascii_lowercase().as_str(), "zh-tw" | "zh-hk" | "zh-mo")
                && let Some(index) = available.iter().position(|c| c.eq_ignore_ascii_case("zh-Hant"))
            {
                return Some(index);
            }
            // CLDR supplementalData.xml parentLocales: Latin American Spanish.
            // Check after the exact locale and before the generic `es` fallback.
            // https://github.com/unicode-org/cldr/blob/main/common/supplemental/supplementalData.xml
            if matches!(
                code.to_ascii_lowercase().as_str(),
                "es-ar"
                    | "es-bo"
                    | "es-br"
                    | "es-bz"
                    | "es-cl"
                    | "es-co"
                    | "es-cr"
                    | "es-cu"
                    | "es-do"
                    | "es-ec"
                    | "es-gt"
                    | "es-hn"
                    | "es-jp"
                    | "es-mx"
                    | "es-ni"
                    | "es-pa"
                    | "es-pe"
                    | "es-pr"
                    | "es-py"
                    | "es-sv"
                    | "es-us"
                    | "es-uy"
                    | "es-ve"
            ) && let Some(index) = available.iter().position(|c| c.eq_ignore_ascii_case("es-419"))
            {
                return Some(index);
            }
            // CLDR's European Portuguese parents, after any exact match.
            if matches!(
                code.to_ascii_lowercase().as_str(),
                "pt-ao"
                    | "pt-ch"
                    | "pt-cv"
                    | "pt-fr"
                    | "pt-gq"
                    | "pt-gw"
                    | "pt-lu"
                    | "pt-mo"
                    | "pt-mz"
                    | "pt-st"
                    | "pt-tl"
            ) && let Some(index) = available.iter().position(|c| c.eq_ignore_ascii_case("pt-PT"))
            {
                return Some(index);
            }
            let Some(end) = code.rfind('-') else { break };
            code.truncate(end);
        }
    }
    None
}

// Forward raw numeric arguments so Fluent can apply locale-specific plural rules.
macro_rules! tr {
    ($id:literal $(, $name:ident = $value:expr)* $(,)?) => {
        i18n_embed_fl::fl!($crate::i18n::LOADER, $id $(, $name = $value)*)
    };
}
pub(crate) use tr;

/// Native name first, followed by the name in the active UI language.
pub fn language_name(code: &str) -> String {
    let (native, translated) = match code {
        "en" => ("English", tr!("english")),
        "fr" => ("Français", tr!("french")),
        "de" => ("Deutsch", tr!("german")),
        "es-419" => ("Español latinoamericano", tr!("spanish-latin-america")),
        "es-ES" => ("Español de España", tr!("spanish-spain")),
        "zh-Hans" => ("简体中文", tr!("chinese-simplified")),
        "zh-Hant" => ("繁體中文", tr!("chinese-traditional")),
        "ko" => ("한국어", tr!("korean")),
        "ja" => ("日本語", tr!("japanese")),
        "pl" => ("Polski", tr!("polish")),
        "ru" => ("Русский", tr!("russian")),
        "pt-BR" => ("Português do Brasil", tr!("portuguese-brazil")),
        "it" => ("Italiano", tr!("italian")),
        "tr" => ("Türkçe", tr!("turkish")),
        "uk" => ("Українська", tr!("ukrainian")),
        "cs" => ("Čeština", tr!("czech")),
        "pt-PT" => ("Português de Portugal", tr!("portuguese-portugal")),
        "nl" => ("Nederlands", tr!("dutch")),
        "id" => ("Bahasa Indonesia", tr!("indonesian")),
        "vi" => ("Tiếng Việt", tr!("vietnamese")),
        "th" => ("ไทย", tr!("thai")),
        "sv" => ("Svenska", tr!("swedish")),
        "ro" => ("Română", tr!("romanian")),
        "hu" => ("Magyar", tr!("hungarian")),
        _ => return code.to_owned(),
    };
    format!("{native} ({translated})")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loader(language: &str) -> FluentLanguageLoader {
        let loader = fluent_language_loader!();
        loader.load_languages(&Localizations, &[language.parse().expect("test locale")]).expect("embedded catalog");
        loader.set_use_isolating(false);
        loader
    }

    #[test]
    fn bundled_french_is_selected_for_regional_os_preferences() {
        for locale in ["fr-FR", "fr-CA", "fr-BE", "fr_CH.UTF-8"] {
            let index = preferred_catalog(&[locale.into()], LANGUAGES).expect("French catalog is embedded");
            assert_eq!(LANGUAGES[index], "fr");
            assert_eq!(loader(LANGUAGES[index]).get("cancel"), "Annuler");
        }
    }

    #[test]
    fn bundled_german_matches_regional_preferences_and_plural_rules() {
        for locale in ["de-DE", "de-AT", "de-CH", "de_DE.UTF-8"] {
            let index = preferred_catalog(&[locale.into()], LANGUAGES).expect("German catalog is embedded");
            assert_eq!(LANGUAGES[index], "de");
        }
        let de = loader("de");
        assert_eq!(de.get("cancel"), "Abbrechen");
        for (count, contents, workers) in [
            (0, "0 Dateien, 0 Ordner", "0 gleichzeitige Aufgaben"),
            (1, "1 Datei, 1 Ordner", "1 gleichzeitige Aufgabe"),
            (2, "2 Dateien, 2 Ordner", "2 gleichzeitige Aufgaben"),
        ] {
            assert_eq!(i18n_embed_fl::fl!(de, "contents-count", files = count, folders = count), contents);
            assert_eq!(i18n_embed_fl::fl!(de, "workers", count = count), workers);
        }
        assert_eq!(
            i18n_embed_fl::fl!(de, "scan-summary", size = "1 KiB", files = 1, folders = 2),
            "1 KiB | 1 Datei, 2 Ordner"
        );
    }

    #[test]
    fn latin_american_spanish_matches_parents_without_overriding_exact_locales() {
        for locale in ["es-419", "es-MX", "es_AR.UTF-8", "es-CO", "es-US", "ES-cl"] {
            let index = preferred_catalog(&[locale.into()], LANGUAGES).expect("Latin American Spanish is embedded");
            assert_eq!(LANGUAGES[index], "es-419");
        }
        let available = ["en", "es", "es-419", "es-MX"];
        let pick = |prefs: &[&str], available: &[&str]| {
            preferred_catalog(&prefs.iter().map(|s| (*s).into()).collect::<Vec<_>>(), available)
        };
        assert_eq!(pick(&["es-MX"], &available), Some(3));
        assert_eq!(pick(&["es-AR", "en"], &available), Some(2));
        assert_eq!(pick(&["en", "es-AR"], &available), Some(0));
        assert_eq!(pick(&["es-ES"], &available), Some(1));
        assert_eq!(pick(&["es"], &available), Some(1));
        assert_eq!(pick(&["es-ES", "es-GQ", "es"], &["en", "es-419"]), None);
        assert_eq!(pick(&["es-AR"], &["en", "es"]), Some(1));
    }

    #[test]
    fn latin_american_spanish_renders_counts_and_literal_paths() {
        let es = loader("es-419");
        assert_eq!(es.get("cancel"), "Cancelar");
        for (count, contents, workers) in [
            (0, "0 archivos, 0 carpetas", "0 tareas simultáneas"),
            (1, "1 archivo, 1 carpeta", "1 tarea simultánea"),
            (2, "2 archivos, 2 carpetas", "2 tareas simultáneas"),
        ] {
            assert_eq!(i18n_embed_fl::fl!(es, "contents-count", files = count, folders = count), contents);
            assert_eq!(i18n_embed_fl::fl!(es, "workers", count = count), workers);
        }
        assert_eq!(
            i18n_embed_fl::fl!(es, "scan-summary", size = "1 KiB", files = 1, folders = 2),
            "1 KiB | 1 archivo, 2 carpetas"
        );
        assert_eq!(
            i18n_embed_fl::fl!(es, "scan-path-error", path = r"C:\{error}", error = "Denied"),
            "Clawback no pudo analizar C:\\{error}.\n\nDenied"
        );
    }

    #[test]
    fn european_spanish_is_selected_independently_and_renders_plural_forms() {
        for locale in ["es-ES", "es_ES.UTF-8", "ES-es"] {
            let index = preferred_catalog(&[locale.into()], LANGUAGES).expect("European Spanish is embedded");
            assert_eq!(LANGUAGES[index], "es-ES");
        }
        for (preferences, expected) in [(["es-ES", "es-MX"], "es-ES"), (["es-MX", "es-ES"], "es-419")] {
            let index = preferred_catalog(&preferences.map(String::from), LANGUAGES).expect("Spanish catalog");
            assert_eq!(LANGUAGES[index], expected);
        }
        let es = loader("es-ES");
        assert_eq!(es.get("show-rollover-boxes"), "Resaltar elementos al pasar el ratón");
        assert_eq!(es.get("spanish-spain"), "Español de España");
        for (count, contents, workers) in [
            (0, "0 archivos, 0 carpetas", "0 tareas simultáneas"),
            (1, "1 archivo, 1 carpeta", "1 tarea simultánea"),
            (2, "2 archivos, 2 carpetas", "2 tareas simultáneas"),
        ] {
            assert_eq!(i18n_embed_fl::fl!(es, "contents-count", files = count, folders = count), contents);
            assert_eq!(i18n_embed_fl::fl!(es, "workers", count = count), workers);
        }
        assert_eq!(
            i18n_embed_fl::fl!(es, "scan-summary", size = "1 KiB", files = 1, folders = 2),
            "1 KiB | 1 archivo, 2 carpetas"
        );
        assert_eq!(
            i18n_embed_fl::fl!(es, "scan-path-error", path = r"C:\{error}", error = "Denied"),
            "Clawback no ha podido analizar C:\\{error}.\n\nDenied"
        );
    }

    #[test]
    fn simplified_chinese_matches_regions_but_preserves_explicit_scripts() {
        for locale in ["zh-Hans", "zh-Hans-CN", "zh-Hans-SG", "zh-Hans-HK", "zh-CN", "zh_SG.UTF-8"] {
            let index = preferred_catalog(&[locale.into()], LANGUAGES).expect("Simplified Chinese is embedded");
            assert_eq!(LANGUAGES[index], "zh-Hans");
        }
        for locale in ["zh-Hant", "zh-Hant-CN", "zh-Hant-TW", "zh-TW", "zh_HK.UTF-8", "zh-MO"] {
            let index = preferred_catalog(&[locale.into()], LANGUAGES).expect("Traditional Chinese is embedded");
            assert_eq!(LANGUAGES[index], "zh-Hant");
        }
        assert_eq!(preferred_catalog(&["zh".into()], LANGUAGES), None);
        assert_eq!(preferred_catalog(&["zh-CN".into()], &["zh-Hans", "zh-CN"]), Some(1));
        assert_eq!(preferred_catalog(&["zh-TW".into()], &["zh-Hant", "zh-TW"]), Some(1));
        let preferences = ["zh-TW".into(), "zh-SG".into(), "en-US".into()];
        let index = preferred_catalog(&preferences, LANGUAGES).expect("first preference matches");
        assert_eq!(LANGUAGES[index], "zh-Hant");
        assert_eq!(preferred_catalog(&preferences, &["zh-Hans", "en"]), Some(0));
    }

    #[test]
    fn simplified_chinese_counts_and_paths_render_without_plural_inflection() {
        let zh = loader("zh-Hans");
        assert_eq!(zh.get("cancel"), "取消");
        for count in [0, 1, 2] {
            assert_eq!(
                i18n_embed_fl::fl!(zh, "contents-count", files = count, folders = count),
                format!("{count} 个文件，{count} 个文件夹")
            );
            assert_eq!(i18n_embed_fl::fl!(zh, "workers", count = count), format!("{count} 个并发任务"));
        }
        assert_eq!(
            i18n_embed_fl::fl!(zh, "scan-summary", size = "1 KiB", files = 1, folders = 2),
            "1 KiB | 1 个文件，2 个文件夹"
        );
        assert_eq!(
            i18n_embed_fl::fl!(zh, "scan-path-error", path = r"C:\{error}", error = "Denied"),
            "Clawback 无法扫描 C:\\{error}。\n\nDenied"
        );
    }

    #[test]
    fn korean_matches_os_locales_and_formats_counts_and_paths() {
        for locale in ["ko", "ko-KR", "ko_KR.UTF-8", "KO-kr"] {
            let index = preferred_catalog(&[locale.into()], LANGUAGES).expect("Korean is embedded");
            assert_eq!(LANGUAGES[index], "ko");
        }
        let ko = loader("ko");
        assert_eq!(ko.get("cancel"), "취소");
        assert_eq!(ko.get("korean"), "한국어");
        for count in [0, 1, 2] {
            assert_eq!(
                i18n_embed_fl::fl!(ko, "contents-count", files = count, folders = count),
                format!("파일 {count}개, 폴더 {count}개")
            );
            assert_eq!(i18n_embed_fl::fl!(ko, "workers", count = count), format!("동시 작업 {count}개"));
        }
        assert_eq!(
            i18n_embed_fl::fl!(ko, "scan-summary", size = "1 KiB", files = 1, folders = 2),
            "1 KiB | 파일 1개, 폴더 2개"
        );
        assert_eq!(
            i18n_embed_fl::fl!(ko, "scan-path-error", path = r"C:\{error}", error = "Denied"),
            "다음 경로를 스캔하지 못했습니다: C:\\{error}\n\nDenied"
        );
    }

    #[test]
    fn japanese_matches_os_locales_and_formats_counts_and_paths() {
        for locale in ["ja", "ja-JP", "ja_JP.UTF-8", "JA-jp"] {
            let index = preferred_catalog(&[locale.into()], LANGUAGES).expect("Japanese is embedded");
            assert_eq!(LANGUAGES[index], "ja");
        }
        let ja = loader("ja");
        assert_eq!(ja.get("cancel"), "キャンセル");
        assert_eq!(ja.get("japanese"), "日本語");
        for count in [0, 1, 2] {
            assert_eq!(
                i18n_embed_fl::fl!(ja, "contents-count", files = count, folders = count),
                format!("ファイル {count} 個、フォルダー {count} 個")
            );
            assert_eq!(i18n_embed_fl::fl!(ja, "workers", count = count), format!("同時実行タスク {count} 件"));
        }
        assert_eq!(
            i18n_embed_fl::fl!(ja, "scan-summary", size = "1 KiB", files = 1, folders = 2),
            "1 KiB | ファイル 1 個、フォルダー 2 個"
        );
        assert_eq!(
            i18n_embed_fl::fl!(ja, "scan-path-error", path = r"C:\{error}", error = "Denied"),
            "次のパスをスキャンできませんでした: C:\\{error}\n\nDenied"
        );
    }

    #[test]
    fn polish_matches_os_locales_and_uses_all_plural_forms() {
        for locale in ["pl", "pl-PL", "pl_PL.UTF-8", "PL-pl"] {
            let index = preferred_catalog(&[locale.into()], LANGUAGES).expect("Polish is embedded");
            assert_eq!(LANGUAGES[index], "pl");
        }
        let pl = loader("pl");
        assert_eq!(pl.get("cancel"), "Anuluj");
        for (count, file, folder, worker) in [
            (0, "plików", "folderów", "zadań równoległych"),
            (1, "plik", "folder", "zadanie równoległe"),
            (2, "pliki", "foldery", "zadania równoległe"),
            (4, "pliki", "foldery", "zadania równoległe"),
            (5, "plików", "folderów", "zadań równoległych"),
            (12, "plików", "folderów", "zadań równoległych"),
            (14, "plików", "folderów", "zadań równoległych"),
            (21, "plików", "folderów", "zadań równoległych"),
            (22, "pliki", "foldery", "zadania równoległe"),
            (25, "plików", "folderów", "zadań równoległych"),
            (101, "plików", "folderów", "zadań równoległych"),
            (112, "plików", "folderów", "zadań równoległych"),
            (122, "pliki", "foldery", "zadania równoległe"),
        ] {
            assert_eq!(
                i18n_embed_fl::fl!(pl, "contents-count", files = count, folders = count),
                format!("{count} {file}, {count} {folder}")
            );
            assert_eq!(i18n_embed_fl::fl!(pl, "workers", count = count), format!("{count} {worker}"));
        }
        assert!(i18n_embed_fl::fl!(pl, "workers", count = 1.5).ends_with(" zadania równoległego"));
        assert_eq!(
            i18n_embed_fl::fl!(pl, "scan-summary", size = "1 KiB", files = 1, folders = 22),
            "1 KiB | 1 plik, 22 foldery"
        );
        assert_eq!(
            i18n_embed_fl::fl!(pl, "scan-path-error", path = r"C:\{error}", error = "Denied"),
            "Nie można przeskanować ścieżki: C:\\{error}\n\nDenied"
        );
    }

    #[test]
    fn russian_matches_os_locales_and_uses_all_plural_forms() {
        for locale in ["ru", "ru-RU", "ru_RU.UTF-8", "RU-ru", "ru-KZ"] {
            let index = preferred_catalog(&[locale.into()], LANGUAGES).expect("Russian is embedded");
            assert_eq!(LANGUAGES[index], "ru");
        }
        let ru = loader("ru");
        assert_eq!(ru.get("cancel"), "Отмена");
        for (count, file, folder, worker) in [
            (0, "файлов", "папок", "параллельных задач"),
            (1, "файл", "папка", "параллельная задача"),
            (2, "файла", "папки", "параллельные задачи"),
            (4, "файла", "папки", "параллельные задачи"),
            (5, "файлов", "папок", "параллельных задач"),
            (11, "файлов", "папок", "параллельных задач"),
            (12, "файлов", "папок", "параллельных задач"),
            (14, "файлов", "папок", "параллельных задач"),
            (21, "файл", "папка", "параллельная задача"),
            (22, "файла", "папки", "параллельные задачи"),
            (25, "файлов", "папок", "параллельных задач"),
            (101, "файл", "папка", "параллельная задача"),
            (111, "файлов", "папок", "параллельных задач"),
            (112, "файлов", "папок", "параллельных задач"),
            (122, "файла", "папки", "параллельные задачи"),
        ] {
            assert_eq!(
                i18n_embed_fl::fl!(ru, "contents-count", files = count, folders = count),
                format!("{count} {file}, {count} {folder}")
            );
            assert_eq!(i18n_embed_fl::fl!(ru, "workers", count = count), format!("{count} {worker}"));
        }
        assert!(i18n_embed_fl::fl!(ru, "workers", count = 1.5).ends_with(" параллельной задачи"));
        assert!(i18n_embed_fl::fl!(ru, "contents-count", files = 1.5, folders = 1.5).ends_with(" папки"));
        assert_eq!(
            i18n_embed_fl::fl!(ru, "scan-summary", size = "1 KiB", files = 21, folders = 12),
            "1 KiB | 21 файл, 12 папок"
        );
        assert_eq!(
            i18n_embed_fl::fl!(ru, "scan-path-error", path = r"C:\{error}", error = "Denied"),
            "Не удалось просканировать путь: C:\\{error}\n\nDenied"
        );
    }

    #[test]
    fn brazilian_portuguese_matches_os_locale_and_formats_counts() {
        for locale in ["pt-BR", "pt_BR.UTF-8", "PT-br"] {
            let index = preferred_catalog(&[locale.into()], LANGUAGES).expect("Brazilian Portuguese is embedded");
            assert_eq!(LANGUAGES[index], "pt-BR");
        }
        assert_eq!(preferred_catalog(&["pt".into()], LANGUAGES), None);
        let pt = loader("pt-BR");
        assert_eq!(pt.get("cancel"), "Cancelar");
        for (count, contents, workers) in [
            (0, "0 arquivo, 0 pasta", "0 tarefa simultânea"),
            (1, "1 arquivo, 1 pasta", "1 tarefa simultânea"),
            (2, "2 arquivos, 2 pastas", "2 tarefas simultâneas"),
        ] {
            assert_eq!(i18n_embed_fl::fl!(pt, "contents-count", files = count, folders = count), contents);
            assert_eq!(i18n_embed_fl::fl!(pt, "workers", count = count), workers);
        }
        assert!(i18n_embed_fl::fl!(pt, "workers", count = 1_000_000).ends_with(" tarefas simultâneas"));
        assert_eq!(
            i18n_embed_fl::fl!(pt, "scan-summary", size = "1 KiB", files = 1, folders = 2),
            "1 KiB | 1 arquivo, 2 pastas"
        );
        assert_eq!(
            i18n_embed_fl::fl!(pt, "scan-path-error", path = r"C:\{error}", error = "Denied"),
            "Não foi possível verificar o caminho: C:\\{error}\n\nDenied"
        );
    }

    #[test]
    fn new_catalogs_match_os_preferences_and_preserve_arguments() {
        for (locale, code, cancel) in [
            ("tr_TR.UTF-8", "tr", "İptal"),
            ("uk-UA", "uk", "Скасувати"),
            ("cs-CZ", "cs", "Zrušit"),
            ("pt_PT.UTF-8", "pt-PT", "Cancelar"),
            ("nl-BE", "nl", "Annuleren"),
            ("id-ID", "id", "Batal"),
            ("vi-VN", "vi", "Hủy"),
            ("th-TH", "th", "ยกเลิก"),
            ("sv-FI", "sv", "Avbryt"),
            ("ro-MD", "ro", "Anulează"),
            ("hu-HU", "hu", "Mégse"),
        ] {
            let index = preferred_catalog(&[locale.into()], LANGUAGES).expect("embedded locale");
            assert_eq!(LANGUAGES[index], code);
            let language = loader(code);
            assert_eq!(language.get("cancel"), cancel);
            let contents = i18n_embed_fl::fl!(language, "contents-count", files = 1, folders = 2);
            assert_eq!(
                i18n_embed_fl::fl!(language, "scan-summary", size = "1 KiB", files = 1, folders = 2),
                format!("1 KiB | {contents}")
            );
            let error = i18n_embed_fl::fl!(language, "scan-path-error", path = r"C:\{error}", error = "Denied");
            assert!(error.contains(r"C:\{error}"), "{code}: {error}");
            assert!(error.ends_with("\n\nDenied"), "{code}: {error}");
            for id in [
                "turkish",
                "ukrainian",
                "czech",
                "portuguese-portugal",
                "dutch",
                "indonesian",
                "vietnamese",
                "thai",
                "swedish",
                "romanian",
                "hungarian",
            ] {
                assert_ne!(language.get(id), id, "{code}: untranslated language name");
            }
        }
    }

    #[test]
    fn european_portuguese_parents_and_plural_rules() {
        for locale in
            ["pt-PT", "pt-AO", "pt-CH", "pt-CV", "pt-FR", "pt-GQ", "pt-GW", "pt-LU", "pt-MO", "pt-MZ", "pt-ST", "pt-TL"]
        {
            let index = preferred_catalog(&[locale.into()], LANGUAGES).expect("European Portuguese");
            assert_eq!(LANGUAGES[index], "pt-PT");
        }
        assert_eq!(preferred_catalog(&["pt-AO".into()], &["pt-PT", "pt-AO"]), Some(1));
        assert_eq!(preferred_catalog(&["pt-AO".into()], &["pt-BR"]), None);
        let pt = loader("pt-PT");
        for (count, files, folders, workers) in [
            (0, "ficheiros", "pastas", "tarefas simultâneas"),
            (1, "ficheiro", "pasta", "tarefa simultânea"),
            (2, "ficheiros", "pastas", "tarefas simultâneas"),
        ] {
            assert_eq!(
                i18n_embed_fl::fl!(pt, "contents-count", files = count, folders = count),
                format!("{count} {files}, {count} {folders}")
            );
            assert_eq!(i18n_embed_fl::fl!(pt, "workers", count = count), format!("{count} {workers}"));
        }
    }

    #[test]
    fn new_catalog_integer_plural_rules() {
        for (code, counts, files, folders, workers) in [
            ("uk", &[1, 21, 101][..], "файл", "тека", "паралельне завдання"),
            ("uk", &[2, 4, 22][..], "файли", "теки", "паралельні завдання"),
            ("uk", &[0, 5, 11, 12, 111][..], "файлів", "тек", "паралельних завдань"),
            ("cs", &[1][..], "soubor", "složka", "souběžná úloha"),
            ("cs", &[2, 3, 4][..], "soubory", "složky", "souběžné úlohy"),
            ("cs", &[0, 5, 11, 22][..], "souborů", "složek", "souběžných úloh"),
            ("nl", &[1][..], "bestand", "map", "gelijktijdige taak"),
            ("nl", &[0, 2][..], "bestanden", "mappen", "gelijktijdige taken"),
            ("sv", &[1][..], "fil", "mapp", "samtidig uppgift"),
            ("sv", &[0, 2][..], "filer", "mappar", "samtidiga uppgifter"),
            ("tr", &[0, 1, 2, 21][..], "dosya", "klasör", "eşzamanlı görev"),
            ("hu", &[0, 1, 2, 21][..], "fájl", "mappa", "párhuzamos feladat"),
            ("id", &[0, 1, 2][..], "berkas", "folder", "tugas bersamaan"),
            ("vi", &[0, 1, 2][..], "tệp", "thư mục", "tác vụ đồng thời"),
            ("th", &[0, 1, 2][..], "ไฟล์", "โฟลเดอร์", "งานพร้อมกัน"),
        ] {
            let language = loader(code);
            for &count in counts {
                assert_eq!(
                    i18n_embed_fl::fl!(language, "contents-count", files = count, folders = count),
                    format!("{count} {files}, {count} {folders}"),
                    "{code}"
                );
                assert_eq!(
                    i18n_embed_fl::fl!(language, "workers", count = count),
                    format!("{count} {workers}"),
                    "{code}"
                );
            }
        }
    }

    #[test]
    fn new_catalog_fractional_plural_rules() {
        for (code, files, folders, workers) in
            [("uk", "файлу", "теки", "паралельного завдання"), ("cs", "souboru", "složky", "souběžné úlohy")]
        {
            let language = loader(code);
            let contents = i18n_embed_fl::fl!(language, "contents-count", files = 1.5, folders = 1.5);
            assert!(
                contents.contains(&format!(" {files}, ")) && contents.ends_with(&format!(" {folders}")),
                "{code}: {contents}"
            );
            assert!(i18n_embed_fl::fl!(language, "workers", count = 1.5).ends_with(&format!(" {workers}")));
        }
    }

    #[test]
    fn romanian_label_first_counts_avoid_upstream_plural_rule_bug() {
        // intl_pluralrules 7.0.2 omits Romanian's modulo-100 condition.
        // Labels followed by values are grammatical regardless of category.
        let ro = loader("ro");
        for count in [0, 1, 2, 19, 20, 21, 100, 101, 102, 119, 120] {
            assert_eq!(
                i18n_embed_fl::fl!(ro, "contents-count", files = count, folders = count),
                format!("Fișiere: {count}, dosare: {count}")
            );
            assert_eq!(i18n_embed_fl::fl!(ro, "workers", count = count), format!("Sarcini simultane: {count}"));
        }
    }

    #[test]
    fn italian_matches_os_locales_and_formats_counts_and_paths() {
        for locale in ["it", "it-IT", "it-CH", "it_IT.UTF-8", "IT-it"] {
            let index = preferred_catalog(&[locale.into()], LANGUAGES).expect("Italian is embedded");
            assert_eq!(LANGUAGES[index], "it");
        }
        let it = loader("it");
        assert_eq!(it.get("cancel"), "Annulla");
        for (count, contents, workers) in [
            (0, "0 file, 0 cartelle", "0 attività simultanee"),
            (1, "1 file, 1 cartella", "1 attività simultanea"),
            (2, "2 file, 2 cartelle", "2 attività simultanee"),
        ] {
            assert_eq!(i18n_embed_fl::fl!(it, "contents-count", files = count, folders = count), contents);
            assert_eq!(i18n_embed_fl::fl!(it, "workers", count = count), workers);
        }
        assert!(i18n_embed_fl::fl!(it, "workers", count = 1_000_000).ends_with(" attività simultanee"));
        assert_eq!(
            i18n_embed_fl::fl!(it, "scan-summary", size = "1 KiB", files = 1, folders = 2),
            "1 KiB | 1 file, 2 cartelle"
        );
        assert_eq!(
            i18n_embed_fl::fl!(it, "scan-path-error", path = r"C:\{error}", error = "Denied"),
            "Impossibile analizzare il percorso: C:\\{error}\n\nDenied"
        );
    }

    #[test]
    fn traditional_chinese_counts_and_paths_render() {
        let zh = loader("zh-Hant");
        assert_eq!(zh.get("chinese-traditional"), "繁體中文");
        assert_eq!(zh.get("settings"), "設定…");
        for count in [0, 1, 2] {
            assert_eq!(
                i18n_embed_fl::fl!(zh, "contents-count", files = count, folders = count),
                format!("{count} 個檔案，{count} 個資料夾")
            );
            assert_eq!(i18n_embed_fl::fl!(zh, "workers", count = count), format!("{count} 個並行工作"));
        }
        assert_eq!(
            i18n_embed_fl::fl!(zh, "scan-summary", size = "1 KiB", files = 1, folders = 2),
            "1 KiB | 1 個檔案，2 個資料夾"
        );
        assert_eq!(
            i18n_embed_fl::fl!(zh, "scan-path-error", path = r"C:\{error}", error = "Denied"),
            "Clawback 無法掃描 C:\\{error}。\n\nDenied"
        );
    }

    #[test]
    fn os_preferences_match_in_order_with_safe_locale_fallbacks() {
        let available = ["en", "fr", "pt-BR", "zh-Hant", "zh-Hans"];
        let pick = |prefs: &[&str]| {
            preferred_catalog(&prefs.iter().map(|s| (*s).into()).collect::<Vec<_>>(), &available).map(|i| available[i])
        };
        assert_eq!(pick(&["de-DE", "fr_CA.UTF-8", "en-US"]), Some("fr"));
        assert_eq!(pick(&["PT_br", "fr"]), Some("pt-BR"));
        assert_eq!(pick(&["zh-Hant-TW"]), Some("zh-Hant"));
        assert_eq!(pick(&["pt-PT"]), None);
        assert_eq!(pick(&["zh-CN"]), Some("zh-Hans"));
        assert_eq!(pick(&["C.UTF-8"]), Some("en"));
        assert_eq!(pick(&[]), None);
    }

    #[test]
    fn counts_use_language_specific_plural_rules() {
        let en = loader("en");
        let fr = loader("fr");
        for (count, english, french) in [
            (0, "0 files, 0 folders", "0 fichier, 0 dossier"),
            (1, "1 file, 1 folder", "1 fichier, 1 dossier"),
            (2, "2 files, 2 folders", "2 fichiers, 2 dossiers"),
        ] {
            assert_eq!(i18n_embed_fl::fl!(en, "contents-count", files = count, folders = count), english);
            assert_eq!(i18n_embed_fl::fl!(fr, "contents-count", files = count, folders = count), french);
        }
        assert_eq!(
            i18n_embed_fl::fl!(fr, "scan-summary", size = "1 KiB", files = 1, folders = 2),
            "1 KiB | 1 fichier, 2 dossiers"
        );
        assert_eq!(i18n_embed_fl::fl!(fr, "workers", count = 1), "1 tâche simultanée");
        assert_eq!(i18n_embed_fl::fl!(fr, "workers", count = 2), "2 tâches simultanées");
        assert_eq!(i18n_embed_fl::fl!(en, "workers", count = 1), "1 worker");
        assert_eq!(i18n_embed_fl::fl!(en, "workers", count = 2), "2 workers");
        assert!(
            i18n_embed_fl::fl!(fr, "contents-count", files = 1_000_000, folders = 2).ends_with("fichiers, 2 dossiers")
        );
    }

    #[test]
    fn partial_catalog_falls_back_to_english() {
        struct Partial;
        impl i18n_embed::I18nAssets for Partial {
            fn get_files(&self, path: &str) -> Vec<std::borrow::Cow<'_, [u8]>> {
                if path == "fr/clawback.ftl" {
                    vec![std::borrow::Cow::Borrowed(b"cancel = Annuler")]
                } else {
                    i18n_embed::I18nAssets::get_files(&Localizations, path)
                }
            }
            fn filenames_iter(&self) -> Box<dyn Iterator<Item = String> + '_> {
                i18n_embed::I18nAssets::filenames_iter(&Localizations)
            }
        }
        let loader = fluent_language_loader!();
        loader.load_languages(&Partial, &["fr".parse().expect("French")]).expect("partial translation");
        assert_eq!(i18n_embed_fl::fl!(loader, "cancel"), "Annuler");
        assert_eq!(i18n_embed_fl::fl!(loader, "system-default"), "System default");
    }

    #[test]
    fn interpolated_paths_are_literal_and_newlines_survive() {
        let fr = loader("fr");
        assert_eq!(
            i18n_embed_fl::fl!(fr, "scan-path-error", path = r"C:\{error}", error = "Denied"),
            "Clawback n’a pas pu analyser C:\\{error}.\n\nDenied"
        );
    }
}
