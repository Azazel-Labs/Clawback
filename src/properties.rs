//! The Properties window: an item's details, read off the UI thread.
use crate::background::Job;
use crate::i18n::tr;
use crate::{platform, theme};
use clawback_core::{Kind, NodeId, Tree, format};
use eframe::egui::{self, Align, Id, Layout};
use std::path::PathBuf;
use std::sync::{Arc, mpsc};

pub struct Properties {
    name: String,
    kind: String,
    folder: bool,
    size: String,
    allocated: Option<String>,
    rows: Vec<(String, String)>,
    path: PathBuf,
    error: Option<String>,
}

impl Properties {
    /// Reads attributes from disk, so it runs on a worker.
    fn read(t: &Tree, n: NodeId) -> Self {
        let node = t.node(n);
        let path = t.path(n);
        let name = node.name_lossy().into_owned();
        let kind = match node.kind {
            Kind::Dir => tr!("folder-2"),
            Kind::File => tr!("file"),
            Kind::Symlink => tr!("symbolic-link"),
            Kind::Other => tr!("special-file"),
        };
        let mut rows = Vec::new();
        if node.is_dir() {
            let folders = t.dir_count(n).saturating_sub(1);
            rows.push((tr!("contains"), tr!("contents-count", files = node.files, folders = folders)));
        }
        rows.push((tr!("modified"), format::date(node.mtime)));
        let attrs = platform::attributes(&path);
        if !attrs.is_empty() {
            rows.push((tr!("attributes-2"), attrs.join(", ")));
        }
        Properties {
            name,
            kind,
            folder: node.is_dir(),
            size: format::size(node.display_len()),
            allocated: (format::size(node.size) != format::size(node.display_len())).then(|| format::size(node.size)),
            rows,
            path,
            error: None,
        }
    }
}

/// An open Properties window.
pub enum Props {
    Loading(Job<(), Properties>),
    Shown(Properties),
}

impl Props {
    /// Start reading `n`'s details; the window shows a spinner until they arrive.
    pub fn request(tree: Arc<Tree>, n: NodeId, ctx: &egui::Context) -> Self {
        Props::Loading(Job::spawn((), ctx, move || Properties::read(&tree, n)))
    }
}

/// Pick up finished details; a worker that died closes the window.
pub fn poll(props: &mut Option<Props>) {
    if let Some(Props::Loading(job)) = props {
        match job.try_recv() {
            Ok(p) => *props = Some(Props::Shown(p)),
            Err(mpsc::TryRecvError::Disconnected) => *props = None,
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
}

pub fn show(ctx: &egui::Context, props: &mut Option<Props>) {
    let mut open = true;
    let mut close = false;
    match props {
        None => return,
        Some(Props::Loading(_)) => {
            egui::Window::new(tr!("properties")).id(Id::new("clawback-properties")).open(&mut open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr!("reading-file-details"));
                });
            });
        }
        Some(Props::Shown(p)) => {
            egui::Window::new(tr!("properties"))
                .id(Id::new("clawback-properties"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.set_width(440.0_f32.min(ctx.content_rect().width() - 48.0));
                    ui.spacing_mut().item_spacing.y = 10.0;
                    egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 180.0).max(160.0)).show(
                        ui,
                        |ui| {
                            ui.horizontal(|ui| {
                                let icon = if p.folder {
                                    egui_phosphor::regular::FOLDER
                                } else {
                                    egui_phosphor::regular::FILE
                                };
                                ui.label(egui::RichText::new(icon).size(36.0).color(if p.folder {
                                    theme::FOLDER
                                } else {
                                    theme::ACCENT
                                }));
                                ui.vertical(|ui| {
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(&p.name).size(22.0).strong())
                                            .wrap()
                                            .selectable(true),
                                    );
                                    ui.label(egui::RichText::new(&p.kind).color(theme::MUTED));
                                });
                            });
                            ui.label(egui::RichText::new(tr!("size-2")).small().color(theme::MUTED));
                            ui.label(egui::RichText::new(&p.size).size(30.0).strong());
                            if let Some(allocated) = &p.allocated {
                                egui::Frame::new().fill(theme::NOTE_BG).corner_radius(6.0).inner_margin(12.0).show(
                                    ui,
                                    |ui| {
                                        ui.label(egui::RichText::new(tr!("size-on-disk")).small().color(theme::ACCENT));
                                        ui.label(egui::RichText::new(allocated).size(20.0).strong());
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(tr!("properties-disk-size-detail"))
                                                    .small()
                                                    .color(theme::MUTED),
                                            )
                                            .wrap(),
                                        );
                                    },
                                );
                            }
                            for (label, value) in &p.rows {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(egui::RichText::new(label).color(theme::MUTED));
                                    ui.add(egui::Label::new(value).wrap().selectable(true));
                                });
                            }
                            ui.separator();
                            ui.label(egui::RichText::new(tr!("location")).small().color(theme::MUTED));
                            ui.add(egui::Label::new(p.path.display().to_string()).wrap().selectable(true));
                            if let Some(error) = &p.error {
                                ui.colored_label(theme::DANGER, error);
                            }
                        },
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(platform::file_manager_label()).clicked() {
                            p.error = platform::reveal(&p.path).err();
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button(tr!("ok")).clicked() {
                                close = true;
                            }
                        });
                    });
                });
        }
    }
    if !open || close {
        *props = None;
    }
}
