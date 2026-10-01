//! Settings editor: consistent rows, a bounded scrolling body, and a fixed footer.
use crate::{i18n::tr, theme};
use clawback_core::{
    Settings,
    palette::MAP_PRESETS,
    settings::{BIAS, DENSITY, TIP_DELAY_MAX_MS},
};
use eframe::egui::{self, Align, Color32, Id, Layout, RichText, Stroke, Ui, vec2};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Appearance,
    Tooltips,
    Behavior,
    Scanning,
}

impl Page {
    const ALL: [Self; 4] = [Self::Appearance, Self::Tooltips, Self::Behavior, Self::Scanning];

    fn label(self) -> String {
        match self {
            Self::Appearance => tr!("settings-appearance"),
            Self::Tooltips => tr!("tooltips"),
            Self::Behavior => tr!("settings-behavior"),
            Self::Scanning => tr!("scanning"),
        }
    }

    fn ui(self, ui: &mut Ui, settings: &mut Settings) {
        match self {
            Self::Appearance => appearance(ui, settings),
            Self::Tooltips => tooltips(ui, settings),
            Self::Behavior => behavior(ui, settings),
            Self::Scanning => scanning(ui, settings),
        }
    }
}

pub fn show(ctx: &egui::Context, settings: &mut Settings) -> Option<bool> {
    let mut done = None;
    let compact = ctx.content_rect().height() < 520.0;
    let margin = if compact { 16 } else { 24 };
    let width = (ctx.content_rect().width() - 48.0).clamp(300.0, 680.0);
    // Stored as an index; demo captures preselect a page the same way.
    let page_id = Id::new("settings-page");
    let stored = ctx.data_mut(|data| data.get_temp::<usize>(page_id));
    let mut page = stored.and_then(|index| Page::ALL.get(index).copied()).unwrap_or(Page::Appearance);
    let response = theme::modal("clawback-settings")
        .frame(theme::modal_frame().stroke(Stroke::new(1.0, theme::DIALOG_EDGE)).inner_margin(margin))
        .show(ctx, |ui| {
            ui.set_width(width - 2.0 * f32::from(margin));
            ui.spacing_mut().item_spacing = vec2(12.0, if compact { 8.0 } else { 12.0 });
            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    if !compact {
                        ui.label(RichText::new("CLAWBACK").size(10.0).strong().color(theme::ACCENT));
                    }
                    ui.label(
                        RichText::new(tr!("settings").trim_end_matches(['…', '.']))
                            .size(if compact { 24.0 } else { 28.0 })
                            .strong()
                            .color(theme::TEXT),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("×").size(23.0))
                                .frame_when_inactive(false)
                                .min_size(vec2(32.0, 32.0)),
                        )
                        .on_hover_text(tr!("cancel"))
                        .clicked()
                    {
                        done = Some(false);
                    }
                });
            });
            if !compact {
                ui.add_space(4.0);
            }
            if ui.available_width() < 450.0 || compact {
                let mut index = page as usize;
                egui::ComboBox::from_id_salt("settings-category").width(ui.available_width()).show_index(
                    ui,
                    &mut index,
                    Page::ALL.len(),
                    |index| Page::ALL[index].label(),
                );
                page = Page::ALL[index];
            } else {
                ui.columns(Page::ALL.len(), |columns| {
                    for (&tab, column) in Page::ALL.iter().zip(columns) {
                        let active = tab == page;
                        let response = column.add_sized(
                            [column.available_width(), 36.0],
                            egui::Button::new(RichText::new(tab.label()).color(if active {
                                theme::ACCENT
                            } else {
                                theme::MUTED
                            }))
                            .fill(if active { theme::SELECTED_FILL } else { Color32::TRANSPARENT })
                            .stroke(Stroke::NONE)
                            .corner_radius(7),
                        );
                        if response.clicked() {
                            page = tab;
                        }
                    }
                });
            }
            ui.separator();
            let body_height = (ctx.content_rect().height() - if compact { 200.0 } else { 260.0 }).clamp(24.0, 388.0);
            egui::ScrollArea::vertical()
                .id_salt(("settings-body", page as usize))
                .max_height(body_height)
                .min_scrolled_height(body_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_height(body_height);
                    page.ui(ui, settings);
                });
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add_sized(
                            [112.0, 36.0],
                            theme::primary_button(tr!("settings-save"), theme::ACCENT)
                                .stroke(Stroke::NONE)
                                .corner_radius(7),
                        )
                        .clicked()
                    {
                        done = Some(true);
                    }
                    if ui
                        .add_sized([88.0, 36.0], theme::secondary_button(tr!("cancel")).frame_when_inactive(false))
                        .clicked()
                    {
                        done = Some(false);
                    }
                });
            });
        });
    ctx.data_mut(|data| data.insert_temp(page_id, page as usize));
    if response.should_close() {
        done = Some(false);
    }
    done
}

fn section(ui: &mut Ui, title: String) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).size(11.0).strong().color(theme::MUTED));
}

/// Both columns have explicit widths, independent of label and widget contents.
fn row(ui: &mut Ui, label: &str, control: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    let label_width = (width * 0.48).floor();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        ui.allocate_ui_with_layout(vec2(label_width, 32.0), Layout::left_to_right(Align::Center), |ui| {
            ui.set_width(label_width);
            ui.add(egui::Label::new(label.trim_end_matches([':', '：'])).wrap());
        });
        ui.allocate_ui_with_layout(
            vec2((width - label_width - 16.0).max(0.0), 32.0),
            Layout::right_to_left(Align::Center),
            |ui| {
                ui.set_min_width((width - label_width - 16.0).max(0.0));
                control(ui);
            },
        );
    });
}

fn toggle(ui: &mut Ui, value: &mut bool, label: &str) {
    row(ui, label, |ui| {
        let (rect, mut response) = ui.allocate_exact_size(vec2(36.0, 22.0), egui::Sense::click());
        if response.clicked() {
            *value = !*value;
            response.mark_changed();
        }
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, label));
        let t = ui.ctx().animate_bool(response.id, *value);
        let fill = if *value { theme::ACCENT } else { theme::TOGGLE_OFF };
        ui.painter().rect_filled(rect.shrink(1.0), 10, if ui.is_enabled() { fill } else { fill.gamma_multiply(0.4) });
        if response.has_focus() || response.hovered() {
            ui.painter().rect_stroke(rect, 11, Stroke::new(1.0, theme::ACCENT), egui::StrokeKind::Outside);
        }
        ui.painter().circle_filled(
            egui::pos2(rect.left() + 11.0 + t * 14.0, rect.center().y),
            7.0,
            if *value { theme::BG } else { theme::TEXT },
        );
    });
}

fn appearance(ui: &mut Ui, d: &mut Settings) {
    row(ui, &tr!("language"), |ui| {
        let label = if d.language == "auto" { tr!("system-default") } else { crate::i18n::language_name(&d.language) };
        egui::ComboBox::from_id_salt("language").width(ui.available_width()).selected_text(label).show_ui(ui, |ui| {
            ui.selectable_value(&mut d.language, "auto".to_owned(), tr!("system-default"));
            for code in crate::i18n::languages() {
                ui.selectable_value(&mut d.language, code.to_owned(), crate::i18n::language_name(code));
            }
        });
    });
    section(ui, tr!("display-colors"));
    for (index, (label, value)) in
        [(tr!("files-2"), &mut d.file_color), (tr!("folders"), &mut d.folder_color)].into_iter().enumerate()
    {
        let muted = d.mute_palette;
        row(ui, &label, |ui| {
            // Right-to-left: the current palette's swatches sit beside the closed dropdown.
            swatches(ui, *value, muted);
            egui::ComboBox::from_id_salt(("color-scheme", index))
                .width(ui.available_width())
                .selected_text(MAP_PRESETS.iter().find(|(id, _)| id == value).map_or("", |(_, name)| *name))
                .show_ui(ui, |ui| {
                    ui.set_min_width(SWATCHES_WIDTH + 140.0);
                    for &(scheme, name) in &MAP_PRESETS {
                        let response = ui.add(
                            egui::Button::selectable(*value == scheme, name)
                                .min_size(vec2(ui.available_width(), 24.0))
                                .right_text(()),
                        );
                        let strip = egui::Rect::from_min_size(
                            egui::pos2(response.rect.right() - SWATCHES_WIDTH - 6.0, response.rect.center().y - 7.0),
                            vec2(SWATCHES_WIDTH, 14.0),
                        );
                        theme::paint_swatches(ui.painter(), strip, scheme, muted, SWATCH_GAP, 2.0);
                        if response.clicked() {
                            *value = scheme;
                        }
                    }
                });
        });
    }
    toggle(ui, &mut d.mute_palette, &tr!("mute-colors"));
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), egui::Sense::hover());
    // The preview row leaves its trailing gap empty.
    theme::paint_swatches(ui.painter(), rect.with_max_x(rect.right() - 4.0), d.file_color, d.mute_palette, 4.0, 4.0);
    section(ui, tr!("file-layout"));
    row(ui, &tr!("density"), |ui| {
        let mut index = (d.density - DENSITY.0).clamp(0, 5) as usize;
        if egui::ComboBox::from_id_salt("density")
            .width(ui.available_width())
            .show_index(ui, &mut index, 6, |i| match i {
                0 => tr!("too-few-files"),
                1 => tr!("very-few-files"),
                2 => tr!("normal"),
                3 => tr!("lots-of-files"),
                4 => tr!("very-many-files"),
                _ => tr!("too-many-files"),
            })
            .changed()
        {
            d.density = index as i32 + DENSITY.0;
        }
    });
    row(ui, &tr!("bias"), |ui| {
        ui.vertical(|ui| {
            let width = ui.available_width();
            ui.spacing_mut().slider_width = width;
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add(egui::Slider::new(&mut d.bias, BIAS.0..=BIAS.1).show_value(false))
                .on_hover_text(tr!("settings-bias-help"));
            let (rect, _) = ui.allocate_exact_size(vec2(width, 14.0), egui::Sense::hover());
            for (position, anchor, label) in [
                (rect.left_center(), egui::Align2::LEFT_CENTER, tr!("horz")),
                (rect.center(), egui::Align2::CENTER_CENTER, tr!("equal")),
                (rect.right_center(), egui::Align2::RIGHT_CENTER, tr!("vert")),
            ] {
                ui.painter().text(position, anchor, label, egui::FontId::proportional(10.0), theme::MUTED);
            }
        });
    });
}

const SWATCH: f32 = 10.0;
const SWATCH_GAP: f32 = 2.0;
const SWATCHES_WIDTH: f32 = 8.0 * (SWATCH + SWATCH_GAP) - SWATCH_GAP;

/// A palette's eight depth colours as a compact strip.
fn swatches(ui: &mut Ui, scheme: usize, muted: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(SWATCHES_WIDTH, 14.0), egui::Sense::hover());
    theme::paint_swatches(ui.painter(), rect, scheme, muted, SWATCH_GAP, 2.0);
}

fn delay(ui: &mut Ui, ms: &mut u32) {
    row(ui, &tr!("delay"), |ui| {
        ui.add(egui::DragValue::new(ms).range(0..=TIP_DELAY_MAX_MS).speed(5).suffix(format!(" {}", tr!("msec"))));
    });
}

fn tooltips(ui: &mut Ui, d: &mut Settings) {
    section(ui, tr!("settings-name-tips"));
    toggle(ui, &mut d.show_name_tips, &tr!("show-file-name-tips"));
    ui.add_enabled_ui(d.show_name_tips, |ui| delay(ui, &mut d.nametip_delay_ms));
    ui.separator();
    section(ui, tr!("settings-info-tips"));
    toggle(ui, &mut d.show_info_tips, &tr!("show-file-info-tips"));
    ui.add_enabled_ui(d.show_info_tips, |ui| {
        delay(ui, &mut d.infotip_delay_ms);
        ui.columns(2, |columns| {
            columns[0].checkbox(&mut d.tip_path, tr!("full-path"));
            columns[0].checkbox(&mut d.tip_icon, tr!("icon"));
            columns[1].checkbox(&mut d.tip_date, tr!("date-time"));
            columns[1].checkbox(&mut d.tip_size, tr!("file-size"));
            columns[1].checkbox(&mut d.tip_attrib, tr!("attributes"));
        });
    });
}

fn behavior(ui: &mut Ui, d: &mut Settings) {
    section(ui, tr!("settings-window"));
    toggle(ui, &mut d.save_pos, &tr!("remember-window-position"));
    ui.label(RichText::new(tr!("takes-effect-the-next-time-clawback-starts")).small().color(theme::MUTED));
    toggle(ui, &mut d.rollover_box, &tr!("show-rollover-boxes"));
    ui.add_space(8.0);
    ui.separator();
    section(ui, tr!("settings-deletion"));
    toggle(ui, &mut d.auto_rescan, &tr!("auto-rescan-on-delete"));
    toggle(ui, &mut d.disable_delete, &tr!("disable-delete-command"));
}

fn scanning(ui: &mut Ui, d: &mut Settings) {
    section(ui, tr!("settings-scan-scope"));
    toggle(ui, &mut d.one_filesystem, &tr!("stay-on-one-filesystem"));
    ui.label(RichText::new(tr!("don-t-descend-into-other-drives-or-network")).small().color(theme::MUTED));
    ui.add_space(8.0);
    ui.separator();
    section(ui, tr!("settings-file-sizes"));
    toggle(ui, &mut d.apparent_size, &tr!("use-file-lengths-not-size-on-disk"));
    if cfg!(unix) {
        toggle(ui, &mut d.dedupe_hardlinks, &tr!("count-hard-linked-files-once"));
    }
    ui.add_space(12.0);
    egui::Frame::new().fill(theme::NOTE_BG).corner_radius(7).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(tr!("scanning-options-apply-to-the-next-scan")).color(theme::ACCENT));
    });
}
