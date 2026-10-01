//! The drive picker: drives as cards with usage bars, recent folders, and a native folder browser.
use crate::background::Job;
use crate::i18n::tr;
use crate::platform::{self, DiskInfo};
use crate::theme;
use clawback_core::format::{self, dir_display, display_name, fraction};
use eframe::egui::{self, Align, Align2, Key, Layout, RichText, Ui, vec2};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Choice {
    Drive(usize),
    Recent(usize),
}

struct PickerDrive {
    disk: DiskInfo,
    #[cfg(windows)]
    turbo: bool,
}

pub struct OpenDialog {
    drives: Vec<PickerDrive>,
    choice: Option<Choice>,
    loading: Option<Job<(), Vec<PickerDrive>>>,
}

impl OpenDialog {
    /// Open the picker; drives are listed on a worker, since probing them can stall.
    pub fn start(ctx: &egui::Context) -> Self {
        let loading = Job::spawn((), ctx, || {
            platform::drive_list()
                .into_iter()
                .map(|disk| PickerDrive {
                    #[cfg(windows)]
                    turbo: crate::turbo::eligible(Some(&disk), true),
                    disk,
                })
                .collect()
        });
        OpenDialog { drives: Vec::new(), choice: None, loading: Some(loading) }
    }
}

/// Native picker icons, loaded once; painted stand-ins replace any that are missing.
#[derive(Default)]
pub struct PickerIcons {
    fixed: Option<egui::TextureHandle>,
    removable: Option<egui::TextureHandle>,
    folder: Option<egui::TextureHandle>,
}

impl PickerIcons {
    #[cfg(windows)]
    pub fn load(ctx: &egui::Context) -> Self {
        use windows_sys::Win32::UI::Shell::{SIID_DRIVEFIXED, SIID_DRIVEREMOVE, SIID_FOLDER};
        let load = |id| {
            crate::filetype_icons::windows::stock(id, 64)
                .map(|image| ctx.load_texture("drive-icon", image, egui::TextureOptions::LINEAR))
        };
        PickerIcons { fixed: load(SIID_DRIVEFIXED), removable: load(SIID_DRIVEREMOVE), folder: load(SIID_FOLDER) }
    }

    #[cfg(not(windows))]
    pub fn load(_ctx: &egui::Context) -> Self {
        Self::default()
    }

    fn drive(&self, removable: bool) -> Option<&egui::TextureHandle> {
        if removable { self.removable.as_ref() } else { self.fixed.as_ref() }
    }
}

/// How the picker closed.
pub enum Picked {
    /// Scan `path`; `turbo` also asks for the elevated NTFS reader.
    Scan {
        path: PathBuf,
        turbo: bool,
    },
    Cancelled,
}

/// Show the picker; `browse_from` is where "Other folder" starts.
pub fn show(
    ctx: &egui::Context,
    dlg: &mut OpenDialog,
    icons: &PickerIcons,
    recent: &[PathBuf],
    browse_from: Option<&Path>,
) -> Option<Picked> {
    let _span = crate::perf::span("ui.drive_picker");
    if let Some(((), drives)) = Job::poll(&mut dlg.loading) {
        dlg.drives = drives;
    } else if dlg.loading.is_some() {
        ctx.request_repaint_after(Duration::from_millis(100));
    }
    let mut chosen: Option<PathBuf> = None;
    #[cfg(windows)]
    let mut turbo = false;
    #[cfg(not(windows))]
    let turbo = false;
    let (mut cancel, mut browse) = (false, false);
    let path_of = |c: Choice, dlg: &OpenDialog| match c {
        Choice::Drive(i) => dlg.drives[i].disk.mount.clone(),
        Choice::Recent(i) => recent[i].clone(),
    };
    let modal = theme::modal("clawback-open-drive").show(ctx, |ui| {
        ui.set_width((ctx.content_rect().width() - 64.0).clamp(300.0, 560.0));
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(RichText::new(tr!("select-drive-to-view")).size(24.0).strong().color(theme::TEXT));
        ui.add_space(4.0);
        ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
        egui::ScrollArea::vertical()
            .max_height((ctx.content_rect().height() - 240.0).clamp(160.0, 420.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if dlg.loading.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.weak(tr!("finding-drives"));
                    });
                } else if dlg.drives.is_empty() {
                    ui.weak(tr!("no-drives-found-use-other-folder-to-pick"));
                }
                for (i, drive) in dlg.drives.iter().enumerate() {
                    #[cfg(windows)]
                    let turbo = drive.turbo;
                    #[cfg(not(windows))]
                    let turbo = false;
                    let icon = icons.drive(drive.disk.removable);
                    let r = drive_card(ui, &drive.disk, icon, turbo, dlg.choice == Some(Choice::Drive(i)));
                    if r.clicked() {
                        dlg.choice = Some(Choice::Drive(i));
                    }
                    if r.double_clicked() {
                        chosen = Some(drive.disk.mount.clone());
                    }
                }
                if !recent.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(tr!("recent")).size(11.0).strong().color(theme::MUTED));
                    for (i, p) in recent.iter().enumerate() {
                        let r = folder_card(ui, p, icons.folder.as_ref(), dlg.choice == Some(Choice::Recent(i)));
                        if r.clicked() {
                            dlg.choice = Some(Choice::Recent(i));
                        }
                        if r.double_clicked() {
                            chosen = Some(p.clone());
                        }
                    }
                }
            });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(tr!("other-folder")).min_size(vec2(0.0, 34.0))).clicked() {
                browse = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let open =
                    theme::primary_button(tr!("ok").trim(), theme::ACCENT).min_size(vec2(96.0, theme::BUTTON_HEIGHT));
                if ui.add_enabled(dlg.choice.is_some(), open).clicked()
                    && let Some(c) = dlg.choice
                {
                    chosen = Some(path_of(c, dlg));
                }
                #[cfg(windows)]
                if let Some(Choice::Drive(i)) = dlg.choice
                    && dlg.drives[i].turbo
                    && ui
                        .add(turbo_button())
                        .on_hover_text(tr!("try-a-faster-ntfs-scan-with-administrator-permission"))
                        .clicked()
                {
                    chosen = Some(dlg.drives[i].disk.mount.clone());
                    turbo = true;
                }
                if ui.add(theme::secondary_button(tr!("cancel"))).clicked() {
                    cancel = true;
                }
            });
        });
    });
    if chosen.is_none()
        && ctx.input(|i| i.key_pressed(Key::Enter))
        && let Some(c) = dlg.choice
    {
        chosen = Some(path_of(c, dlg));
    }
    if modal.should_close() {
        cancel = true;
    }
    if browse {
        chosen = platform::pick_folder(browse_from).or(chosen);
    }
    if let Some(path) = chosen {
        Some(Picked::Scan { path, turbo })
    } else if cancel || ctx.input(|i| i.key_pressed(Key::Escape)) {
        Some(Picked::Cancelled)
    } else {
        None
    }
}

/// Prominent, consistent action shared by the picker and scan status.
#[cfg(windows)]
pub fn turbo_button() -> egui::Button<'static> {
    theme::primary_button(format!("⚡  {}", tr!("turbo")), theme::ACCENT).min_size(vec2(112.0, theme::BUTTON_HEIGHT))
}

/// One drive in the picker: a large native icon, its name, a usage bar and free space.
fn drive_card(
    ui: &mut Ui,
    disk: &DiskInfo,
    icon: Option<&egui::TextureHandle>,
    turbo: bool,
    selected: bool,
) -> egui::Response {
    let (rect, response) = picker_card(ui, 78.0, selected);
    let p = ui.painter();
    let icon_rect = egui::Rect::from_center_size(rect.left_center() + vec2(42.0, 0.0), vec2(56.0, 56.0));
    if let Some(icon) = icon {
        egui::Image::from_texture(icon).paint_at(ui, icon_rect);
    } else {
        // Portable stand-in: a drive body with an activity light.
        let body = egui::Rect::from_center_size(icon_rect.center(), vec2(48.0, 30.0));
        p.rect_filled(body, 6, egui::Color32::from_rgb(70, 78, 88));
        p.circle_filled(body.right_center() - vec2(9.0, 0.0), 3.0, theme::ACCENT);
    }
    let left = icon_rect.right() + 16.0;
    let right = rect.right() - 16.0;
    let used = disk.total.saturating_sub(disk.free);
    let fraction = fraction(used, disk.total);
    let name = p.layout_no_wrap(disk.label(), egui::FontId::proportional(16.0), theme::TEXT);
    let mut name_right = right;
    if turbo {
        let badge = p.layout_no_wrap(format!("⚡ {}", tr!("turbo")), egui::FontId::proportional(11.0), theme::ACCENT);
        let badge_rect = egui::Rect::from_min_size(
            egui::pos2(right - badge.size().x - 14.0, rect.top() + 12.0),
            badge.size() + vec2(14.0, 6.0),
        );
        p.rect_filled(badge_rect, 9, theme::ACCENT.gamma_multiply(0.14));
        p.galley(badge_rect.min + vec2(7.0, 3.0), badge, theme::ACCENT);
        name_right = badge_rect.left() - 8.0;
    }
    let name_pos = egui::pos2(left, rect.top() + 12.0);
    p.with_clip_rect(egui::Rect::from_min_max(name_pos, egui::pos2(name_right, rect.bottom()))).galley(
        name_pos,
        name,
        theme::TEXT,
    );
    let bar = egui::Rect::from_min_max(egui::pos2(left, rect.top() + 40.0), egui::pos2(right, rect.top() + 46.0));
    p.rect_filled(bar, 3, theme::BG);
    let mut filled = bar;
    filled.max.x = bar.left() + bar.width() * fraction;
    p.rect_filled(filled, 3, if fraction >= 0.9 { theme::DANGER } else { theme::ACCENT });
    p.text(
        egui::pos2(left, rect.bottom() - 14.0),
        Align2::LEFT_CENTER,
        tr!("drive-free-of-total", free = format::size(disk.free), total = format::size(disk.total)),
        egui::FontId::proportional(12.0),
        theme::MUTED,
    );
    p.text(
        egui::pos2(right, rect.bottom() - 14.0),
        Align2::RIGHT_CENTER,
        format!("{}  ·  {}", format::percent(used, disk.total), disk.fs),
        egui::FontId::proportional(12.0),
        theme::MUTED,
    );
    response.on_hover_text(disk.mount.display().to_string())
}

/// A clickable picker card with hover and selection chrome; contents are painted by the caller.
fn picker_card(ui: &mut Ui, height: f32, selected: bool) -> (egui::Rect, egui::Response) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), egui::Sense::click());
    let hover = ui.ctx().animate_bool(response.id, response.hovered());
    let p = ui.painter();
    let fill = if selected { theme::SELECTED_FILL } else { theme::NAVIGATOR };
    p.rect_filled(rect, 10, fill.lerp_to_gamma(theme::ROW_ALT, if selected { 0.0 } else { hover }));
    p.rect_stroke(
        rect,
        10,
        egui::Stroke::new(if selected { 1.5 } else { 1.0 }, if selected { theme::ACCENT } else { theme::BORDER }),
        egui::StrokeKind::Inside,
    );
    (rect, response.on_hover_cursor(egui::CursorIcon::PointingHand))
}

/// A recently opened folder: its icon and name, with the containing folder beneath.
fn folder_card(ui: &mut Ui, path: &Path, icon: Option<&egui::TextureHandle>, selected: bool) -> egui::Response {
    let (rect, response) = picker_card(ui, 52.0, selected);
    let p = ui.painter();
    let icon_rect = egui::Rect::from_center_size(rect.left_center() + vec2(30.0, 0.0), vec2(32.0, 32.0));
    if let Some(icon) = icon {
        egui::Image::from_texture(icon).paint_at(ui, icon_rect);
    } else {
        // Portable stand-in: a folder tab and body.
        let body = egui::Rect::from_center_size(icon_rect.center() + vec2(0.0, 2.0), vec2(28.0, 20.0));
        p.rect_filled(egui::Rect::from_min_size(body.min - vec2(0.0, 4.0), vec2(12.0, 6.0)), 2, theme::FOLDER);
        p.rect_filled(body, 3, theme::FOLDER);
    }
    let left = icon_rect.right() + 14.0;
    let text = egui::Rect::from_min_max(egui::pos2(left, rect.top()), egui::pos2(rect.right() - 14.0, rect.bottom()));
    let parent = path.parent().map(dir_display).unwrap_or_default();
    let clipped = p.with_clip_rect(text);
    clipped.text(
        egui::pos2(left, rect.top() + 17.0),
        Align2::LEFT_CENTER,
        display_name(path),
        egui::FontId::proportional(14.0),
        theme::TEXT,
    );
    clipped.text(
        egui::pos2(left, rect.bottom() - 15.0),
        Align2::LEFT_CENTER,
        parent,
        egui::FontId::proportional(11.0),
        theme::MUTED,
    );
    response.on_hover_text(path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_rows_keep_their_geometry_on_hover_and_selection() {
        let ctx = egui::Context::default();
        theme::apply(&ctx, "en");
        let mut row = egui::Rect::NOTHING;
        let mut footer = egui::Rect::NOTHING;
        let mut draw = |pointer, selected| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(640.0, 600.0))),
                    events: vec![egui::Event::PointerMoved(pointer)],
                    ..Default::default()
                },
                |ui| {
                    ui.set_width(440.0);
                    ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
                    egui::ScrollArea::vertical()
                        .max_height(340.0)
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for i in 0..20 {
                                let disk = DiskInfo {
                                    name: format!("Drive {i}: {}", "long drive name ".repeat(12)),
                                    mount: PathBuf::from(format!("/mnt/{i}")),
                                    fs: "NTFS".into(),
                                    total: 1 << 40,
                                    free: 100 << 30,
                                    removable: false,
                                    kind: clawback_core::adaptive::StorageKind::Unknown,
                                };
                                let response = drive_card(ui, &disk, None, true, selected);
                                if i == 0 {
                                    row = response.rect;
                                }
                            }
                        });
                    footer = ui.button("Other folder").rect;
                },
            );
            output.textures_delta.clear();
            (row, footer)
        };
        let outside = egui::pos2(620.0, 580.0);
        for _ in 0..4 {
            draw(outside, false);
        }
        let expected = draw(outside, false);
        for selected in [false, true, false] {
            for pointer in [expected.0.center(), egui::pos2(expected.0.right() + 6.0, expected.0.center().y), outside] {
                for _ in 0..4 {
                    assert_eq!(draw(pointer, selected), expected);
                }
            }
        }
    }
}
