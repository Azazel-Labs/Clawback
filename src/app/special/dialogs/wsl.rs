use crate::app::ClawbackApp;
use crate::app::special::Compacted;
use crate::app::special::windows::wsl_disks;
use crate::{i18n::tr, platform, theme};
use clawback_core::format;
use eframe::egui::{self, Align, Id, Layout};
use std::path::PathBuf;

impl ClawbackApp {
    pub(in crate::app) fn request_wsl_disk(&mut self, path: PathBuf, ctx: &egui::Context) {
        if self.special.compacting.is_some() {
            return;
        }
        self.special.wsl_info = None;
        self.special.wsl_confirm_compact = false;
        let target = path.clone();
        self.special.wsl_info_job = Some(crate::background::Job::spawn((), ctx, move || wsl_disks::inspect(&target)));
        self.special.wsl_disk = Some(path);
    }

    pub(in crate::app) fn finish_compaction(&mut self, doc_id: u64, updated: Compacted) {
        let Some(doc) = self.doc.as_mut().filter(|doc| doc.id == doc_id) else { return };
        if let Some(live) = &self.live {
            live.invalidate(updated.path.clone());
        }
        // Update just this file and its ancestor totals, including when watching is unavailable.
        if let Some(id) = doc.tree.find_path(&updated.path)
            && !doc.tree.node(id).is_dir()
            && let Some(len) = updated.info.file_size
        {
            let old = doc.tree.node(id);
            let size = if old.has(clawback_core::tree::flags::HARDLINK_DUP) {
                0
            } else if self.settings.apparent_size {
                len
            } else {
                updated.info.size_on_disk.unwrap_or(old.size)
            };
            let entry = clawback_core::tree::NewEntry {
                name: old.name.to_os_string(),
                kind: old.kind,
                size,
                len,
                mtime: updated.info.modified.unwrap_or(old.mtime),
                flags: old.flags,
                file_id: old.file_id,
            };
            std::sync::Arc::make_mut(&mut doc.tree).update_file(id, entry);
        }
        doc.disk = updated.disk.or(doc.disk.take());
        doc.generation += 1;
    }

    pub(in crate::app) fn wsl_disk_dialog(&mut self, ctx: &egui::Context) {
        if let Some(((), info)) = crate::background::Job::poll(&mut self.special.wsl_info_job) {
            self.special.wsl_info = Some(info);
        }
        if let Some((doc_id, result)) = crate::background::Job::poll(&mut self.special.compacting) {
            match result {
                Ok(updated) => {
                    self.special.wsl_disk = None;
                    self.finish_compaction(doc_id, updated);
                }
                Err(error) => {
                    self.error = Some(tr!("compact-wsl-error", error = error));
                    self.special.wsl_disk = None;
                }
            }
        }
        let Some(path) = self.special.wsl_disk.clone() else { return };
        let busy = self.special.compacting.is_some();
        let mut close = false;
        let mut reveal = false;
        let mut browse = None;
        let mut open_docker = false;
        let mut compact = false;
        let dialog = egui::Modal::new(Id::new("wsl-disk")).show(ctx, |ui| {
            ui.set_width(680.0_f32.min((ctx.content_rect().width() - 48.0).max(160.0)));
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.spacing_mut().item_spacing.y = 8.0;
            close = super::header(ui, &tr!("wsl-disk-title"), egui_phosphor::regular::HARD_DRIVES);
            ui.label(egui::RichText::new("VHDX · WSL").small().color(theme::MUTED));
            ui.add_space(4.0);
            let body_height = (ctx.content_rect().height() - 190.0).max(80.0);
            egui::ScrollArea::vertical().id_salt("wsl-body").max_height(body_height).auto_shrink([false, true]).show(
                ui,
                |ui| {
                    if let Some(info) = &self.special.wsl_info {
                        use wsl_disks::DiskKind;
                        let owner = match info.kind {
                            DiskKind::DockerData => tr!("wsl-owner-docker-data"),
                            DiskKind::DockerSystem => tr!("wsl-owner-docker-system"),
                            DiskKind::Linux => {
                                info.distribution.clone().unwrap_or_else(|| tr!("wsl-owner-unidentified"))
                            }
                        };
                        ui.strong(owner);
                        ui.label(match info.kind {
                            DiskKind::DockerData => tr!("wsl-docker-data-description"),
                            DiskKind::DockerSystem => tr!("wsl-docker-system-description"),
                            DiskKind::Linux => tr!("wsl-linux-description"),
                        });
                        ui.horizontal(|ui| {
                            if let Some(size) = info.size_on_disk.or(info.file_size) {
                                ui.label(egui::RichText::new(format::size(size)).size(26.0).strong());
                                let label =
                                    if info.size_on_disk.is_some() { tr!("size-on-disk") } else { tr!("size-2") };
                                ui.label(egui::RichText::new(label).color(theme::MUTED));
                            }
                        });
                        if info.kind == DiskKind::Linux && info.distribution.is_none() {
                            ui.label(tr!("wsl-owner-unidentified-detail"));
                        }
                    } else {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(tr!("reading-file-details"));
                        });
                    }
                    ui.collapsing(tr!("wsl-disk-details"), |ui| {
                        if let Some(info) = &self.special.wsl_info {
                            if let Some(size) = info.file_size {
                                ui.label(format!("{} {}", tr!("size-2"), format::size(size)));
                            }
                            if let Some(modified) = info.modified {
                                ui.label(format!("{} {}", tr!("modified"), format::date(modified)));
                            }
                            if let Some(package) = &info.package {
                                ui.label(egui::RichText::new(tr!("wsl-package")).color(theme::MUTED));
                                ui.add(egui::Label::new(package).wrap().selectable(true));
                            }
                        }
                        ui.label(egui::RichText::new(tr!("wsl-disk-path")).color(theme::MUTED));
                        ui.add(egui::Label::new(path.display().to_string()).wrap().selectable(true));
                        if ui.button(platform::file_manager_label()).clicked() {
                            reveal = true;
                        }
                    });
                    ui.separator();
                    ui.add_space(8.0);
                    if busy {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(tr!("compacting-wsl-disk"));
                        });
                    } else if self.special.wsl_confirm_compact {
                        ui.strong(tr!("wsl-compact-ready"));
                        ui.label(tr!("wsl-compact-stop-short"));
                        ui.label(egui::RichText::new(tr!("wsl-compact-admin")).small().color(theme::MUTED));
                    } else {
                        let info = self.special.wsl_info.as_ref();
                        let mut browse_action = |ui: &mut egui::Ui| {
                            if let Some(info) = info {
                                match info.kind {
                                    wsl_disks::DiskKind::DockerData | wsl_disks::DiskKind::DockerSystem => {
                                        open_docker = action(
                                            ui,
                                            egui_phosphor::regular::HARD_DRIVES,
                                            &tr!("wsl-open-docker"),
                                            tr!("wsl-open-docker-detail"),
                                            None,
                                        );
                                    }
                                    wsl_disks::DiskKind::Linux => {
                                        if let Some(name) = &info.distribution
                                            && action(
                                                ui,
                                                egui_phosphor::regular::FOLDER_OPEN,
                                                &tr!("wsl-browse-linux"),
                                                tr!("wsl-browse-linux-detail"),
                                                None,
                                            )
                                        {
                                            browse = Some(name.clone());
                                        }
                                    }
                                }
                            }
                        };
                        let mut compact_action = |ui: &mut egui::Ui| {
                            if action(
                                ui,
                                egui_phosphor::regular::ARROWS_CLOCKWISE,
                                &tr!("wsl-prepare-compaction"),
                                tr!("wsl-compact-summary"),
                                Some(tr!("wsl-compact-tooltip")),
                            ) {
                                self.special.wsl_confirm_compact = true;
                            }
                        };
                        let has_browse = info
                            .is_some_and(|info| info.kind != wsl_disks::DiskKind::Linux || info.distribution.is_some());
                        if ui.available_width() >= 560.0 && has_browse {
                            ui.columns(2, |columns| {
                                browse_action(&mut columns[0]);
                                compact_action(&mut columns[1]);
                            });
                        } else {
                            browse_action(ui);
                            compact_action(ui);
                        }
                        ui.collapsing(tr!("wsl-cleanup-help"), |ui| {
                            let docker = self
                                .special
                                .wsl_info
                                .as_ref()
                                .is_some_and(|info| info.kind != wsl_disks::DiskKind::Linux);
                            ui.label(if docker {
                                tr!("wsl-docker-cleanup-help")
                            } else {
                                tr!("wsl-linux-cleanup-help")
                            });
                        });
                    }
                },
            );
            ui.separator();
            // Keep navigation and confirmation visible even when the details need scrolling.
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if busy {
                        ui.spinner();
                    } else if self.special.wsl_confirm_compact {
                        if ui.button(tr!("wsl-stop-and-compact")).on_hover_text(tr!("wsl-compact-tooltip")).clicked() {
                            compact = true;
                        }
                        if ui.button(tr!("back")).clicked() {
                            self.special.wsl_confirm_compact = false;
                        }
                    } else if ui.button(tr!("cancel")).clicked() {
                        close = true;
                    }
                });
            });
        });
        if reveal && let Err(error) = platform::reveal(&path) {
            self.error = Some(error);
            close = true;
        }
        if let Some(name) = browse {
            if let Err(error) = platform::open(&PathBuf::from(format!("\\\\wsl$\\{name}"))) {
                self.error = Some(error);
            }
            close = true;
        }
        if open_docker {
            if let Err(error) = wsl_disks::open_docker() {
                self.error = Some(error);
            }
            close = true;
        }
        if compact && let Some(doc) = &self.doc {
            let mut disk = doc.disk.clone();
            self.special.compacting = Some(crate::background::Job::spawn(doc.id, ctx, move || {
                wsl_disks::compact(&path).map_err(|error| error.to_string())?;
                let info = wsl_disks::inspect(&path);
                if let Some(disk) = &mut disk {
                    platform::refresh_disk_space(disk);
                }
                Ok(Compacted { path, info, disk })
            }));
        }
        if close || (!busy && dialog.should_close()) {
            self.special.wsl_disk = None;
        }
    }
}

/// A full-width action with a quiet explanation directly beneath its label.
fn action(ui: &mut egui::Ui, icon: &str, title: &str, detail: String, tooltip: Option<String>) -> bool {
    let mut clicked = false;
    let card = egui::Frame::new()
        .fill(theme::NOTE_BG)
        .stroke(egui::Stroke::new(1.0, theme::DIALOG_EDGE))
        .corner_radius(6.0)
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.set_min_height(104.0);
            let button = egui::Button::new(egui::RichText::new(format!("{icon}  {title}")).color(theme::ACCENT))
                .wrap()
                .frame(false);
            let mut response = ui.add_sized([ui.available_width(), theme::BUTTON_HEIGHT], button);
            if let Some(tip) = &tooltip {
                response = response.on_hover_text(tip);
            }
            clicked = response.clicked();
            ui.add(egui::Label::new(egui::RichText::new(detail).small().color(theme::MUTED)).wrap());
        });
    let response = card.response.interact(egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    let response = if let Some(tip) = tooltip { response.on_hover_text(tip) } else { response };
    clicked || response.clicked()
}
