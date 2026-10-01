//! Shared visual language for the shell, dialogs and disk map.
use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle, vec2};

pub const BG: Color32 = Color32::from_rgb(17, 18, 18);
pub const SURFACE: Color32 = Color32::from_rgb(24, 25, 25);
pub const NAVIGATOR: Color32 = Color32::from_rgb(27, 30, 34);
pub const ROW_ALT: Color32 = Color32::from_rgb(33, 37, 42);
pub const PANEL_EDGE: Color32 = Color32::from_rgb(62, 69, 78);
pub const BORDER: Color32 = Color32::from_rgb(43, 44, 44);
pub const TEXT: Color32 = Color32::from_rgb(216, 216, 216);
pub const MUTED: Color32 = Color32::from_rgb(153, 153, 153);
pub const ACCENT: Color32 = Color32::from_rgb(117, 169, 214);
pub const DANGER: Color32 = Color32::from_rgb(226, 104, 92);
pub const FOLDER: Color32 = Color32::from_rgb(232, 191, 92);

pub fn set_fonts(ctx: &egui::Context, language: &str) {
    // Keep CJK and Thai labels and filenames readable without installed OS fonts.
    // Install for every language so the native name in Settings also renders.
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Noto Sans SC".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/NotoSansSC-Regular.otf")).into(),
    );
    fonts.font_data.insert(
        "Noto Sans KR".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/NotoSansKR-Regular.otf")).into(),
    );
    fonts.font_data.insert(
        "Noto Sans JP".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/NotoSansJP-Regular.otf")).into(),
    );
    fonts.font_data.insert(
        "Noto Sans TC".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/NotoSansTC-Regular.otf")).into(),
    );
    fonts.font_data.insert(
        "Noto Sans Thai".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/NotoSansThai-Regular.ttf")).into(),
    );
    // Shared Han characters have different regional glyph forms.
    let order = match language {
        "ja" => ["Noto Sans JP", "Noto Sans SC", "Noto Sans KR", "Noto Sans TC"],
        "ko" => ["Noto Sans KR", "Noto Sans SC", "Noto Sans JP", "Noto Sans TC"],
        "zh-Hant" => ["Noto Sans TC", "Noto Sans SC", "Noto Sans KR", "Noto Sans JP"],
        _ => ["Noto Sans SC", "Noto Sans KR", "Noto Sans JP", "Noto Sans TC"],
    };
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .extend(order.into_iter().chain(["Noto Sans Thai"]).map(str::to_owned));
    }
    // Phosphor icons (MIT; license shipped beside the binary) live in the Private Use Area.
    // Last in the chain, so they can only supply icons and never shadow text glyphs.
    fonts
        .font_data
        .insert("Phosphor".into(), egui::FontData::from_static(egui_phosphor::Variant::Regular.font_bytes()).into());
    fonts.families.entry(egui::FontFamily::Proportional).or_default().push("Phosphor".into());
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context, language: &str) {
    set_fonts(ctx, language);
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    let v = &mut style.visuals;
    v.panel_fill = BG;
    v.window_fill = SURFACE;
    v.extreme_bg_color = BG;
    v.faint_bg_color = Color32::from_rgb(30, 31, 31);
    v.override_text_color = Some(TEXT);
    v.window_corner_radius = CornerRadius::same(6);
    v.menu_corner_radius = CornerRadius::same(4);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.selection.bg_fill = Color32::from_rgb(45, 51, 57);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    for widget in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(3);
        widget.bg_stroke = Stroke::new(1.0, BORDER);
        widget.fg_stroke = Stroke::new(1.0, TEXT);
    }
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(36, 37, 37);
    v.widgets.inactive.bg_fill = Color32::from_rgb(36, 37, 37);
    v.widgets.open.weak_bg_fill = Color32::from_rgb(43, 44, 44);
    // Hover must read clearly on dark dialog surfaces, not just a shade off the resting fill.
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(52, 59, 68);
    v.widgets.hovered.bg_fill = Color32::from_rgb(52, 59, 68);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(102, 124, 146));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(61, 70, 81);
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    style.spacing.item_spacing = vec2(8.0, 8.0);
    style.spacing.button_padding = vec2(12.0, 7.0);
    style.spacing.interact_size = vec2(32.0, 30.0);
    style.animation_time = 0.16;
    style.text_styles.insert(TextStyle::Body, FontId::proportional(13.0));
    style.text_styles.insert(TextStyle::Button, FontId::proportional(13.0));
    style.text_styles.insert(TextStyle::Heading, FontId::proportional(23.0));
    ctx.set_style_of(egui::Theme::Dark, style);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_icons_resolve_from_the_bundled_icon_font() {
        let ctx = egui::Context::default();
        set_fonts(&ctx, "en");
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let definitions = ui.fonts_mut(|fonts| fonts.definitions().clone());
            assert_eq!(
                definitions.families[&egui::FontFamily::Proportional].last().map(String::as_str),
                Some("Phosphor")
            );
            let mut fonts = egui::epaint::text::Fonts::new(egui::epaint::text::TextOptions::default(), definitions);
            let characters = fonts.fonts.font(&egui::FontFamily::Proportional).characters().clone();
            let icon = egui_phosphor::regular::TRASH.chars().next().expect("icon codepoint");
            assert!(characters.contains_key(&icon));
        });
        output.textures_delta.clear();
    }

    #[test]
    fn catalogs_have_font_coverage_and_render() {
        let ctx = egui::Context::default();
        apply(&ctx, "en");
        for language in ["ja", "ko", "zh-Hans", "zh-Hant", "th"] {
            check_cjk_fonts(&ctx, language);
        }
    }

    fn check_cjk_fonts(ctx: &egui::Context, language: &str) {
        set_fonts(ctx, language);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let definitions = ui.fonts_mut(|fonts| fonts.definitions().clone());
            let expected = match language {
                "ja" => "Noto Sans JP",
                "ko" => "Noto Sans KR",
                "zh-Hant" => "Noto Sans TC",
                _ => "Noto Sans SC",
            };
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                let first_cjk = definitions.families[&family].iter().find(|name| name.starts_with("Noto Sans"));
                assert_eq!(first_cjk.map(String::as_str), Some(expected));
            }
            let mut coverage = egui::epaint::text::Fonts::new(egui::epaint::text::TextOptions::default(), definitions);
            for font in [FontId::proportional(13.0), FontId::monospace(13.0)] {
                // Inspect the charmaps directly: egui 0.36's has_glyph reports
                // false for valid characters in the replacement glyph's face.
                let mut family = coverage.fonts.font(&font.family);
                let characters = family.characters();
                for catalog in [
                    include_str!("../locales/zh-Hans/clawback.ftl"),
                    include_str!("../locales/zh-Hant/clawback.ftl"),
                    include_str!("../locales/ko/clawback.ftl"),
                    include_str!("../locales/ja/clawback.ftl"),
                    include_str!("../locales/pl/clawback.ftl"),
                    include_str!("../locales/ru/clawback.ftl"),
                    include_str!("../locales/pt-BR/clawback.ftl"),
                    include_str!("../locales/it/clawback.ftl"),
                    include_str!("../locales/tr/clawback.ftl"),
                    include_str!("../locales/uk/clawback.ftl"),
                    include_str!("../locales/cs/clawback.ftl"),
                    include_str!("../locales/pt-PT/clawback.ftl"),
                    include_str!("../locales/nl/clawback.ftl"),
                    include_str!("../locales/id/clawback.ftl"),
                    include_str!("../locales/vi/clawback.ftl"),
                    include_str!("../locales/th/clawback.ftl"),
                    include_str!("../locales/sv/clawback.ftl"),
                    include_str!("../locales/ro/clawback.ftl"),
                    include_str!("../locales/hu/clawback.ftl"),
                    include_str!("../locales/af/clawback.ftl"),
                    include_str!("../locales/ca/clawback.ftl"),
                    include_str!("../locales/sr-Cyrl/clawback.ftl"),
                    include_str!("../locales/da/clawback.ftl"),
                    include_str!("../locales/fi/clawback.ftl"),
                    include_str!("../locales/el/clawback.ftl"),
                    include_str!("../locales/nb/clawback.ftl"),
                ] {
                    for ch in catalog.chars().filter(|c| !c.is_whitespace()) {
                        assert!(characters.contains_key(&ch), "Missing glyph: {ch}");
                    }
                }
                ui.fonts_mut(|fonts| {
                    for sample in
                        ["简体中文：正在扫描文件夹…", "한국어: 폴더 스캔 중…", "日本語：フォルダーをスキャン中…"]
                    {
                        let galley = fonts.layout_no_wrap(sample.into(), font.clone(), TEXT);
                        assert!(galley.size().x > 0.0 && galley.size().y > 0.0);
                    }
                });
            }
            ui.label("简体中文：正在扫描文件夹…");
            ui.label("繁體中文：正在掃描資料夾…");
            ui.label("한국어: 폴더 스캔 중…");
            ui.label("日本語：フォルダーをスキャン中…");
            ui.label("Polski: Zażółć gęślą jaźń. ŁĄĆĘŃÓŚŹŻ");
            ui.label("Русский: идёт сканирование файлов…");
            ui.label("Italiano: proprietà, attività, perché, più");
            ui.label("ไทย: กำลังค้นหาโฟลเดอร์…");
            ui.label("Tiếng Việt: Đang đọc chi tiết tệp…");
            ui.label("Українська: файли, теки, Україна, об’єкт");
            ui.label("Português do Brasil: verificando arquivos… ação, frequência, ícone");
        });
        assert!(!output.textures_delta.set.is_empty(), "Font glyphs must be rasterized");
        assert!(!ctx.tessellate(output.shapes, output.pixels_per_point).is_empty());
        output.textures_delta.clear();
    }
}
