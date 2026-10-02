use crate::app::ClawbackApp;
use crate::app::special::windows::programs;
use crate::{i18n::tr, platform, theme};
use clawback_core::ROOT;
use eframe::egui::{self, Id};
use std::sync::mpsc;

impl ClawbackApp {
    pub(in crate::app) fn poll_removals(&mut self, ctx: &egui::Context) {
        let mut ready = Vec::new();
        let mut index = 0;
        while index < self.special.removal_checks.len() {
            match self.special.removal_checks[index].1.try_recv() {
                Ok(result) => {
                    let (request, _) = self.special.removal_checks.remove(index);
                    ready.push((request, result));
                }
                Err(mpsc::TryRecvError::Empty) => index += 1,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.special.removal_checks.remove(index);
                    self.error = Some(tr!("uninstall-identification-failed"));
                }
            }
        }
        for (mut request, result) in ready {
            if self.settings.disable_delete {
                continue;
            }
            let Some(doc) = &self.doc else { continue };
            if doc.id != request.doc {
                continue;
            }
            let Some(node) = doc.tree.find_path(&request.path).filter(|&n| n != ROOT || result.is_some()) else {
                continue;
            };
            request.node = node;
            if let Some(mut removal) = result {
                programs::retain_confirmed_owners(&mut removal);
                if !removal.applications.is_empty() {
                    self.special.removals.push_back(removal);
                }
            } else if !self.pending_paths().any(|pending| request.path.starts_with(pending)) {
                self.queue_delete(request, ctx);
            }
        }
    }

    pub(in crate::app) fn removal_dialog(&mut self, ctx: &egui::Context) {
        if self.settings.disable_delete {
            self.special.removal_checks.clear();
            self.special.removals.clear();
            return;
        }
        let Some(removal) = self.special.removals.front() else {
            if let Some((request, _)) = self.special.removal_checks.first() {
                let mut cancel = false;
                let modal = egui::Modal::new(Id::new("identify-application")).show(ctx, |ui| {
                    ui.set_width(460.0_f32.min((ctx.content_rect().width() - 48.0).max(160.0)));
                    cancel = super::header(ui, &tr!("uninstall-identifying"), egui_phosphor::regular::MAGNIFYING_GLASS);
                    ui.label(request.path.to_string_lossy());
                    ui.spinner();
                    cancel |= ui.button(tr!("cancel")).clicked();
                });
                if cancel || modal.should_close() {
                    self.special.removal_checks.remove(0);
                }
            }
            return;
        };
        let mut close = false;
        let mut launch = None;
        let mut open = None;
        let modal = egui::Modal::new(Id::new("uninstall-application")).show(ctx, |ui| {
            ui.set_width(500.0_f32.min((ctx.content_rect().width() - 48.0).max(160.0)));
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.spacing_mut().item_spacing.y = 10.0;
            close = super::header(ui, &tr!("uninstall-dialog-title"), egui_phosphor::regular::TRASH);
            ui.add_space(2.0);
            ui.label(egui::RichText::new(tr!("uninstall-scope-short")).color(theme::MUTED));
            egui::ScrollArea::vertical()
                .id_salt("uninstall-apps")
                .max_height((ctx.content_rect().height() - 210.0).max(80.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for app in &removal.applications {
                        egui::Frame::new().fill(theme::NOTE_BG).corner_radius(8.0).inner_margin(16.0).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(egui::Label::new(egui::RichText::new(&app.name).size(22.0).strong()).wrap());
                            let steam = matches!(app.action, Some(programs::Action::Steam(_)));
                            ui.label(
                                egui::RichText::new(if steam {
                                    tr!("uninstall-via-steam")
                                } else {
                                    tr!("uninstall-via-windows")
                                })
                                .small()
                                .color(theme::ACCENT),
                            );
                            if !app.publisher.is_empty() {
                                ui.label(egui::RichText::new(&app.publisher).color(theme::MUTED));
                            }
                            ui.add_space(4.0);
                            ui.collapsing(tr!("uninstall-details"), |ui| {
                                if !app.version.is_empty() {
                                    ui.label(tr!("uninstall-version", version = app.version.as_str()));
                                }
                                if let Some(programs::Action::Steam(id)) = app.action {
                                    ui.label(tr!("uninstall-steam-id", id = id.to_string()));
                                }
                                ui.add(egui::Label::new(app.location.to_string_lossy()).wrap().selectable(true));
                                if removal.path != app.location {
                                    ui.label(
                                        egui::RichText::new(tr!("uninstall-selected-path")).small().color(theme::MUTED),
                                    );
                                    ui.add(egui::Label::new(removal.path.to_string_lossy()).wrap().selectable(true));
                                }
                            });
                            ui.add_space(4.0);
                            let label = if steam { tr!("uninstall-steam") } else { tr!("uninstall-launch") };
                            if ui
                                .add_enabled(
                                    app.action.is_some(),
                                    egui::Button::new(label)
                                        .fill(theme::SELECTED_FILL)
                                        .min_size(egui::vec2(0.0, theme::BUTTON_HEIGHT)),
                                )
                                .on_hover_text(tr!("uninstall-refresh-help"))
                                .clicked()
                            {
                                launch.clone_from(&app.action);
                            }
                        });
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                let (label, uri) = if removal.steam {
                    (tr!("uninstall-open-steam"), "steam://open/games")
                } else {
                    (tr!("uninstall-open-apps"), "ms-settings:appsfeatures")
                };
                if ui.add(egui::Button::new(label).frame(false)).clicked() {
                    open = Some(uri);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close |= ui.button(tr!("cancel")).clicked();
                });
            });
        });
        if let Some(action) = launch {
            if let Err(error) = programs::launch(&action) {
                self.error = Some(error);
            }
            close = true;
        }
        if let Some(uri) = open {
            if let Err(error) = platform::open(std::path::Path::new(uri)) {
                self.error = Some(error);
            }
            close = true;
        }
        if close || modal.should_close() {
            self.special.removals.pop_front();
        }
    }
}
