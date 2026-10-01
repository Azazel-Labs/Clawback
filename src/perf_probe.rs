//! Deterministic, opt-in developer workload. Never deletes or modifies scanned files.
use clawback_core::{Kind, ROOT, Tree, tree::NewEntry};
use eframe::egui::{self, Context};
use std::{path::Path, sync::mpsc, time::Instant};

pub enum Action {
    Load(Tree),
    ZoomIn,
    ZoomOut,
    Resize(bool),
    Close,
}
pub struct Probe {
    started: Instant,
    step: u64,
    tree: Option<mpsc::Receiver<Tree>>,
    last_frame: Option<Instant>,
    interactive: bool,
}
impl Probe {
    pub fn new() -> Option<Self> {
        let scenario = std::env::var("CLAWBACK_PERF_SCENARIO").ok()?;
        if !["empty", "large", "scan"].contains(&scenario.as_str()) {
            return None;
        }
        crate::perf::instant(&format!("scenario.{scenario}"));
        let tree = if scenario == "large" {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(fixture());
            });
            Some(rx)
        } else {
            None
        };
        Some(Self { started: Instant::now(), step: 0, tree, last_frame: None, interactive: scenario != "empty" })
    }
    pub fn pointer(&self, input: &mut egui::RawInput) {
        if !self.interactive {
            return;
        }
        let phase = self.started.elapsed().as_secs_f32();
        input.events.push(egui::Event::PointerMoved(egui::pos2(200.0 + (phase * 110.0) % 600.0, 410.0)));
    }
    pub fn next(&mut self, ctx: &Context) -> Option<Action> {
        if ctx.current_pass_index() != 0 {
            return None;
        }
        // This interval is meaningful only in the continuously repainting probe.
        let now = Instant::now();
        if let Some(last) = self.last_frame.replace(now) {
            crate::perf::counter("probe.frame_interval_ms", now.duration_since(last).as_secs_f64() * 1000.0);
        }
        ctx.request_repaint();
        if let Some(rx) = &self.tree
            && let Ok(tree) = rx.try_recv()
        {
            self.tree = None;
            crate::perf::instant("probe.tree_loaded");
            return Some(Action::Load(tree));
        }
        let elapsed = self.started.elapsed().as_secs();
        if elapsed >= 8 {
            crate::perf::instant("shutdown.close_requested");
            return Some(Action::Close);
        }
        if elapsed <= self.step {
            return None;
        }
        self.step = elapsed;
        if !self.interactive {
            return None;
        }
        match elapsed {
            2 | 5 => {
                crate::perf::instant("probe.zoom_in");
                Some(Action::ZoomIn)
            }
            3 | 6 => {
                crate::perf::instant("probe.zoom_out");
                Some(Action::ZoomOut)
            }
            4 => {
                crate::perf::instant("probe.resize_small");
                Some(Action::Resize(false))
            }
            7 => {
                crate::perf::instant("probe.resize_large");
                Some(Action::Resize(true))
            }
            _ => None,
        }
    }
}

fn fixture() -> Tree {
    let _span = crate::perf::span("probe.build_fixture");
    let mut tree = Tree::new(Path::new("Performance fixture"));
    let entry = |name: String, kind, size| NewEntry {
        name: name.into(),
        kind,
        size,
        len: size,
        mtime: 1_779_984_000,
        flags: 0,
        file_id: None,
    };
    let dirs = tree.add_children(ROOT, (0..200).map(|i| entry(format!("Folder-{i:03}"), Kind::Dir, 0)).collect());
    for parent in dirs {
        let files = (0..1000)
            .map(|i| {
                let ext = ["rs", "txt", "png", "zip", "mp4", "json"][i % 6];
                entry(format!("File-{i:04}.{ext}"), Kind::File, ((i + 1) * 4096) as u64)
            })
            .collect();
        tree.add_children(parent, files);
    }
    tree.sort_all();
    tree
}

#[cfg(test)]
mod tests {
    #[test]
    fn fixture_has_the_documented_size() {
        let tree = super::fixture();
        assert_eq!(tree.len(), 200_201);
        assert_eq!(tree.root().files, 200_000);
    }
}
