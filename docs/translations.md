# Translations

Clawback uses Fluent through `i18n-embed`, `rust-embed`, and
`i18n-embed-fl`. Catalogs live in `locales/<language>/clawback.ftl`.
English (`en`) is the fallback language; French (`fr`), German (`de`),
Latin American Spanish (`es-419`), European Spanish (`es-ES`),
Simplified Chinese (`zh-Hans`), Traditional Chinese (`zh-Hant`), Korean (`ko`), Japanese (`ja`), Polish (`pl`),
Russian (`ru`), Brazilian Portuguese (`pt-BR`), Italian (`it`), Turkish (`tr`),
Ukrainian (`uk`), Czech (`cs`), European Portuguese (`pt-PT`), Dutch (`nl`),
Indonesian (`id`), Vietnamese (`vi`), Thai (`th`), Swedish (`sv`), Romanian (`ro`),
and Hungarian (`hu`) are included: 24 catalogs including English.
Catalogs are embedded in debug and release builds. No network connection,
Crowdin credentials, or separately installed files are needed at runtime.

Language defaults to **System default**, selecting a supported language from
Windows UI preferences, macOS preferred languages, or Linux locale settings.
Regional locales can use their generic catalog (`fr-CA` → `fr`).
Spanish regional locales listed under CLDR's `es_419` parent (including
`es-MX`, `es-AR`, and `es-CO`) use `es-419` when no exact catalog exists.
This follows [CLDR parent locales](https://github.com/unicode-org/cldr/blob/main/common/supplemental/supplementalData.xml);
Spain's `es-ES` locale selects European Spanish. Unspecified `es` does not
automatically select either regional catalog.
Simplified Chinese uses `zh-Hans`, including explicit script locales such as
`zh-Hans-CN`. Region-only `zh-CN` and `zh-SG` also select it. Traditional
Chinese uses `zh-Hant`, including `zh-Hant-TW`; region-only `zh-TW`, `zh-HK`,
and `zh-MO` select the same catalog, which uses Taiwan-style terminology and
glyphs. Explicit script tags take precedence over region. Unspecified `zh`
does not automatically select either script.
Korean OS locales such as `ko-KR` use the generic `ko` catalog.
Japanese OS locales such as `ja-JP` use the generic `ja` catalog.
Polish OS locales such as `pl-PL` use the generic `pl` catalog.
Russian OS locales such as `ru-RU` use the generic `ru` catalog.
Italian OS locales such as `it-IT` and `it-CH` use the generic `it` catalog.
Brazilian Portuguese matches `pt-BR` (including `pt_BR.UTF-8`). Unspecified
`pt` does not automatically select either Portuguese variant. European
Portuguese matches `pt-PT` and its CLDR parent locales, including `pt-AO` and
`pt-MZ`; an exact catalog always takes precedence. The new generic catalogs
also match regional OS preferences, such as `tr-TR`, `uk-UA`, `cs-CZ`, `nl-BE`,
`id-ID`, `vi-VN`, `th-TH`, `sv-FI`, `ro-MD`, and `hu-HU`.
Saved explicit language choices override the OS. Settings entries show the
native name followed by its name in the active UI language, such as
**Français (French)**, **English (Anglais)**, or **Deutsch (German)**.

## Develop

Use stable message IDs instead of English sentences in Rust:

```rust
use crate::i18n::tr;

ui.button(tr!("cancel"));
ui.label(tr!("contents-count", files = file_count, folders = folder_count));
```

The short `tr!` wrapper delegates to `i18n_embed_fl::fl!`, which checks IDs
and named arguments against the English catalog at compile time.
Pass numeric counts as numbers, not formatted strings, so Fluent can choose
plural variants and format them. Paths and error messages remain literal data.
Byte sizes can still use the app's existing size formatter.

Edit Fluent messages directly. For example:

```ftl
cancel = Cancel

contents-count =
    { $files ->
        [one] { $files } file
       *[other] { $files } files
    }, { $folders ->
        [one] { $folders } folder
       *[other] { $folders } folders
    }
```

Translations can choose their own plural categories, selectors, terms, and
grammatical variants. Keep IDs and externally supplied variable names stable;
do not translate paths, shortcuts, Clawback, or byte units.
Missing messages fall back to English. Omit untranslated messages rather than
adding empty Fluent values, which are invalid.

Plural category names are language-specific: for integer counts, French
`[one]` includes both 0 and 1, while English, German, and Spanish `[one]` include only 1.
File/folder and worker messages use singular and plural variants;
the default `*[other]` also covers categories whose wording is identical
to the ordinary plural. Fixed category headings such as “Files” do not need
selectors. See [Unicode plural rules](https://cldr.unicode.org/index/cldr-spec/plural-rules).
Chinese, Korean, and Japanese count messages use one pattern with counters for all counts;
they do not need singular/plural branches.
Polish count messages use `[one]`, `[few]`, and `[many]` for integer counts
(1 plik, 2 pliki, 5 plików, 12 plików, 22 pliki). The default `[other]`
handles fractional counts. Tests cover the teen exceptions and larger numbers.
Russian also uses `[one]`, `[few]`, `[many]`, and `[other]`, with its own rules:
21 файл, 22 файла, 25 файлов, but 11 файлов. Tests cover these forms, fractional
counts, and mixed file/folder counts. Existing fonts cover Russian Cyrillic.
Brazilian Portuguese uses `[one]` for integer counts 0 and 1, and the default
plural branch for other integer counts, including the `many` category.
Italian uses the singular only for 1: `1 cartella`, `2 cartelle`;
`file` is unchanged in the plural. Worker labels also inflect their adjective:
`1 attività simultanea`, `2 attività simultanee`.
European Portuguese differs from Brazilian Portuguese at zero: `0 ficheiros`,
`1 ficheiro`, `2 ficheiros`. Ukrainian distinguishes `21 файл`, `22 файли`,
and `11 файлів`; Czech uses `1 soubor`, `2 soubory`, but `22 souborů`.
Both have fractional variants. Romanian uses label-first counts such as
`Fișiere: 101, dosare: 2` and `Sarcini simultane: 4`. This avoids incorrect
numeral agreement caused by `intl_pluralrules` 7.0.2's Romanian rule, which
omits the modulo-100 condition for counts such as 101 and 102.
Dutch and Swedish distinguish singular and plural. Turkish and Hungarian keep
the noun singular after a numeral; Indonesian, Vietnamese, and Thai also use
one count pattern. Tests cover file, folder, and worker labels.

Run:

```text
cargo xtask translations check
cargo xtask translations fmt
cargo xtask translations fmt --check
cargo test --workspace --locked
```

The check parses every catalog, rejects malformed or duplicate entries,
unresolved/cyclic references, unknown target messages, and unexpected target
variables. It also checks Rust call arguments against the English catalog.
`check` also enforces canonical Fluent formatting in CI. `fmt` applies the
official `fluent-syntax` serializer; `fmt --check` checks formatting without
writing files. Comments, grammar, message order, and literal text are preserved.
Malformed files are rejected before any catalogs are written. CRLF and LF
checkouts are treated equivalently.

Builds validate catalogs too. Fluent source files are authored, not regenerated:
the old JSON `translations update` command has been retired to avoid
overwriting translator-authored grammar.

To add a language, add `locales/<language>/clawback.ftl` and register its native
name and translatable display name in `src/i18n.rs`. Add that display-name
message to the existing catalogs. Rebuild to embed the new language.

This covers GUI menus, settings, dialogs, navigation, file-type labels, and
scan messages. CLI/TUI text, watcher diagnostics, palette names, and some core
formatting remain English. Noto Sans SC, TC, KR, and JP are embedded as fallback fonts for
Chinese, Korean, and Japanese UI text and filenames, including native language names when
another UI language is active. Their shared license and sources are in
`assets/fonts`; release packages include the license. Tests check coverage
and rendering of the CJK catalogs. The active UI language determines the
CJK fallback order so shared characters use the appropriate regional glyphs.
Noto Sans Thai is also embedded for Thai text and language names; its separate
license is included in release archives and macOS app resources. Coverage
tests include the new catalogs, including Vietnamese diacritics and Ukrainian
Cyrillic. Other scripts and bidirectional layout still need verification before shipping.

## Connect Crowdin

1. Create a Crowdin project with English as its source language and French
   (`fr`), German (`de`), Spanish, Latin America (`es-419`), Spanish (`es-ES`),
   Chinese Simplified (`zh-CN`), Chinese Traditional (`zh-TW`), Korean (`ko`), Japanese (`ja`), Polish (`pl`), Russian (`ru`),
   Portuguese, Brazilian (`pt-BR`), Italian (`it`), Turkish (`tr`), Ukrainian
   (`uk`), Czech (`cs`), Portuguese (`pt-PT`), Dutch (`nl`), Indonesian (`id`),
   Vietnamese (`vi`), Thai (`th`), Swedish (`sv-SE`), Romanian (`ro`), and
   Hungarian (`hu`) as target languages.
2. Set the GitHub repository variable `CROWDIN_PROJECT_ID` and secret
   `CROWDIN_PERSONAL_TOKEN` to the project's ID and an authorized token.
3. Upload `locales/en/clawback.ftl` as the source file and import
   `locales/fr/clawback.ftl`, `locales/de/clawback.ftl`,
   `locales/es-419/clawback.ftl`, `locales/es-ES/clawback.ftl`,
   `locales/zh-Hans/clawback.ftl`, `locales/zh-Hant/clawback.ftl`, `locales/ko/clawback.ftl`,
   `locales/ja/clawback.ftl`, `locales/pl/clawback.ftl`, `locales/ru/clawback.ftl`,
   `locales/pt-BR/clawback.ftl`, `locales/it/clawback.ftl`, and the catalogs for
   the eleven additional targets listed above as their respective translations.
   Review and approve the translations before the
   first approved-only download.
4. Allow GitHub Actions to create pull requests, then run **Crowdin translations**.

For an existing JSON-based project, import the new Fluent source and French
catalog before retiring the old JSON source. Message IDs have changed;
do not download old JSON exports over the Fluent catalogs.

The configuration maps French and German exports to the generic `fr` and `de`
directories so regional OS preferences share their language's translation.
Crowdin's Simplified Chinese (`zh-CN`) exports map to `zh-Hans`, and Traditional
Chinese (`zh-TW`) exports map to `zh-Hant`. Swedish (`sv-SE`) maps to `sv`;
the other new generic languages map to their two-letter catalog directories.
Portuguese (`pt-PT`) keeps its regional code, separate from `pt-BR`.
The workflow uploads sources, downloads approved
translations, and opens a PR; it never merges automatically.
It remains disabled until a project ID is configured. Normal local builds and
translation checks do not upload anything.

PRs created with GitHub's built-in token do not automatically trigger other
workflows: manually run **CI** against `l10n/crowdin` before merging.

References: [Fluent syntax](https://projectfluent.org/fluent/guide/),
[checked macro](https://docs.rs/i18n-embed-fl/latest/i18n_embed_fl/macro.fl.html),
[Crowdin configuration](https://crowdin.github.io/crowdin-cli/configuration).
