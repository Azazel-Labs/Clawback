//! Native association icons: one background worker, bounded queues and an LRU cache.
use eframe::egui::{self, Color32, ColorImage, Context, Rect, TextureHandle, TextureOptions};
use std::{collections::HashMap, sync::mpsc, time::Duration};
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
pub(crate) mod windows;

#[derive(Clone, Hash, PartialEq, Eq)]
struct Key {
    extension: String,
    size: u32,
}
struct Entry {
    texture: Option<TextureHandle>,
    pending: bool,
    used: u64,
}
struct Request {
    key: Key,
    ctx: Context,
}
struct Reply {
    key: Key,
    image: Option<ColorImage>,
}
struct Worker {
    tx: mpsc::SyncSender<Request>,
    rx: mpsc::Receiver<Reply>,
}
#[derive(Default)]
pub struct Icons {
    entries: HashMap<Key, Entry>,
    worker: Option<Worker>,
    tick: u64,
}
impl Icons {
    pub fn begin_frame(&mut self, ctx: &Context) {
        let _span = crate::perf::span("ui.icon_uploads");
        self.tick = self.tick.wrapping_add(1);
        if let Some(worker) = &self.worker {
            // Limit texture uploads per frame, even after many results arrive together.
            for _ in 0..4 {
                let Ok(reply) = worker.rx.try_recv() else { break };
                if let Some(entry) = self.entries.get_mut(&reply.key) {
                    entry.texture =
                        reply.image.map(|image| ctx.load_texture("file-association", image, TextureOptions::LINEAR));
                    entry.pending = false;
                }
            }
        }
        if self.entries.values().any(|entry| entry.pending) {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
    pub fn paint(&mut self, ui: &egui::Ui, rect: Rect, extension: &str) {
        if extension.len() > 128 {
            return;
        }
        let size = (rect.width() * ui.ctx().pixels_per_point()).ceil().clamp(16.0, 64.0) as u32;
        let key = Key { extension: extension.to_owned(), size };
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.used = self.tick;
            if let Some(texture) = &entry.texture {
                ui.painter().image(
                    texture.id(),
                    rect,
                    Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    Color32::from_white_alpha(210),
                );
            }
            return;
        }
        if self.entries.len() >= 512 {
            let victim = self
                .entries
                .iter()
                .filter(|(_, entry)| !entry.pending)
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone());
            if let Some(victim) = victim {
                self.entries.remove(&victim);
            } else {
                return;
            }
        }
        let worker = self.worker.get_or_insert_with(start_worker);
        if worker.tx.try_send(Request { key: key.clone(), ctx: ui.ctx().clone() }).is_ok() {
            self.entries.insert(key, Entry { texture: None, pending: true, used: self.tick });
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
    }
}
fn start_worker() -> Worker {
    let (tx, requests) = mpsc::sync_channel::<Request>(32);
    let (replies, rx) = mpsc::sync_channel(32);
    std::thread::spawn(move || {
        for request in requests {
            let _span = crate::perf::span("worker.icon");
            let image = load(&request.key.extension, request.key.size).map(|mut image| {
                for pixel in &mut image.pixels {
                    let [r, g, b, a] = pixel.to_srgba_unmultiplied();
                    let gray = (u32::from(r) * 54 + u32::from(g) * 183 + u32::from(b) * 19) / 256;
                    let dim = |c| ((u32::from(c) * 2 + gray * 3) * 85 / 500) as u8;
                    *pixel = Color32::from_rgba_unmultiplied(dim(r), dim(g), dim(b), a);
                }
                image
            });
            if replies.send(Reply { key: request.key, image }).is_err() {
                break;
            }
            request.ctx.request_repaint();
        }
    });
    Worker { tx, rx }
}
fn load(extension: &str, size: u32) -> Option<ColorImage> {
    #[cfg(windows)]
    {
        windows::load(extension, size)
    }
    #[cfg(target_os = "macos")]
    {
        macos::load(extension, size)
    }
    #[cfg(target_os = "linux")]
    {
        linux::load(extension, size)
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = (extension, size);
        None
    }
}
#[cfg(not(windows))]
fn decode(bytes: &[u8], size: u32) -> Option<ColorImage> {
    let image = image::load_from_memory(bytes)
        .ok()?
        .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
        .into_rgba8();
    Some(ColorImage::from_rgba_unmultiplied([size as usize, size as usize], image.as_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uploads_are_bounded_and_completed_misses_are_cached() {
        let (request_tx, _requests) = mpsc::sync_channel(32);
        let (replies, reply_rx) = mpsc::sync_channel(32);
        let mut icons = Icons { worker: Some(Worker { tx: request_tx, rx: reply_rx }), ..Icons::default() };
        for index in 0..10 {
            let key = Key { extension: format!(".{index}"), size: 32 };
            icons.entries.insert(key.clone(), Entry { texture: None, pending: true, used: 0 });
            replies.send(Reply { key, image: None }).expect("reply");
        }
        icons.begin_frame(&Context::default());
        assert_eq!(icons.entries.values().filter(|entry| entry.pending).count(), 6);
        assert_eq!(icons.entries.len(), 10); // failed lookups are retained, not retried each frame
    }
}
