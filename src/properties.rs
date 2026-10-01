//! The Properties window: an item's details, read off the UI thread.
use crate::i18n::tr;
use crate::platform;
use clawback_core::{Kind, NodeId, Tree, format};
use eframe::egui::{self, Align, Id, Layout};
use std::path::PathBuf;
use std::sync::{Arc, mpsc};

pub struct Properties {
    title: String,
    rows: Vec<(String, String)>,
    path: PathBuf,
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
        let mut rows = vec![
            (tr!("name"), name.clone()),
            (tr!("type-2"), kind),
            (tr!("location"), path.parent().map(|p| p.display().to_string()).unwrap_or_default()),
            (tr!("size-2"), format::size(node.display_len())),
            (tr!("size-on-disk"), format::size(node.size)),
        ];
        if node.is_dir() {
            let folders = t.dir_count(n).saturating_sub(1);
            rows.push((tr!("contains"), tr!("contents-count", files = node.files, folders = folders)));
        }
        rows.push((tr!("modified"), format::date(node.mtime)));
        let attrs = platform::attributes(&path);
        if !attrs.is_empty() {
            rows.push((tr!("attributes-2"), attrs.join(" ")));
        }
        Properties { title: tr!("properties-title", name = name), rows, path }
    }
}

/// An open Properties window.
pub enum Props {
    Loading(mpsc::Receiver<Properties>),
    Shown(Properties),
}

impl Props {
    /// Start reading `n`'s details; the window shows a spinner until they arrive.
    pub fn request(tree: Arc<Tree>, n: NodeId, ctx: &egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Properties::read(&tree, n));
            repaint.request_repaint();
        });
        Props::Loading(rx)
    }
}

/// Pick up finished details; a worker that died closes the window.
pub fn poll(props: &mut Option<Props>) {
    if let Some(Props::Loading(rx)) = props {
        match rx.try_recv() {
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
            egui::Window::new(p.title.as_str())
                .id(Id::new("clawback-properties"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    egui::Grid::new("props").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                        for (k, v) in &p.rows {
                            ui.strong(k);
                            ui.label(v);
                            ui.end_row();
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(tr!("show-in-file-manager")).clicked() {
                            let _ = platform::reveal(&p.path);
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
