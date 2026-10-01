//! Native association icons: one background worker, bounded queues and an LRU cache.
use eframe::egui::{self, Color32, ColorImage, Context, Rect, TextureHandle, TextureOptions};
use std::{collections::HashMap, sync::mpsc, time::Duration};
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
pub(crate) mod windows;

const CAPACITY: usize = 512;

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
    /// By pixel size, then extension, so per-row lookups borrow the extension.
    entries: HashMap<u32, HashMap<String, Entry>>,
    len: usize,
    pending: usize,
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
                let entry = self.entries.get_mut(&reply.key.size).and_then(|sized| sized.get_mut(&reply.key.extension));
                if let Some(entry) = entry {
                    entry.texture =
                        reply.image.map(|image| ctx.load_texture("file-association", image, TextureOptions::LINEAR));
                    if std::mem::take(&mut entry.pending) {
                        self.pending -= 1;
                    }
                }
            }
        }
        if self.pending > 0 {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
    pub fn paint(&mut self, ui: &egui::Ui, rect: Rect, extension: &str) {
        if extension.len() > 128 {
            return;
        }
        let size = (rect.width() * ui.ctx().pixels_per_point()).ceil().clamp(16.0, 64.0) as u32;
        if let Some(entry) = self.entries.get_mut(&size).and_then(|sized| sized.get_mut(extension)) {
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
        if self.len >= CAPACITY && !self.evict() {
            return;
        }
        let worker = self.worker.get_or_insert_with(start_worker);
        let key = || Key { extension: extension.to_owned(), size };
        if worker.tx.try_send(Request { key: key(), ctx: ui.ctx().clone() }).is_ok() {
            self.insert(key(), Entry { texture: None, pending: true, used: self.tick });
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
    }
    fn insert(&mut self, key: Key, entry: Entry) {
        self.pending += usize::from(entry.pending);
        if let Some(old) = self.entries.entry(key.size).or_default().insert(key.extension, entry) {
            self.pending -= usize::from(old.pending);
        } else {
            self.len += 1;
        }
    }
    /// Drop the least recently used finished icon; false when every icon is still loading.
    fn evict(&mut self) -> bool {
        let victim = self
            .entries
            .iter()
            .flat_map(|(&size, sized)| sized.iter().map(move |(extension, entry)| (size, extension, entry)))
            .filter(|(_, _, entry)| !entry.pending)
            .min_by_key(|(_, _, entry)| entry.used)
            .map(|(size, extension, _)| (size, extension.clone()));
        let Some((size, extension)) = victim else { return false };
        if let Some(sized) = self.entries.get_mut(&size) {
            sized.remove(&extension);
            if sized.is_empty() {
                self.entries.remove(&size);
            }
        }
        self.len -= 1;
        true
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
    // Platform loaders see None, never the displayed placeholder.
    let extension = (extension != crate::filetypes::NO_EXTENSION).then_some(extension);
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
            let key = || Key { extension: format!(".{index}"), size: 32 };
            icons.insert(key(), Entry { texture: None, pending: true, used: 0 });
            replies.send(Reply { key: key(), image: None }).expect("reply");
        }
        icons.begin_frame(&Context::default());
        let entries = || icons.entries.values().flat_map(HashMap::values);
        assert_eq!(entries().filter(|entry| entry.pending).count(), 6);
        assert_eq!(icons.pending, 6);
        assert_eq!(icons.len, 10); // failed lookups are retained, not retried each frame
        assert_eq!(entries().count(), 10);
    }

    #[test]
    fn eviction_drops_the_least_recently_used_finished_icon() {
        let mut icons = Icons::default();
        for (index, (pending, used)) in [(true, 0), (false, 2), (false, 1)].into_iter().enumerate() {
            icons.insert(
                Key { extension: format!(".{index}"), size: 16 + index as u32 },
                Entry { texture: None, pending, used },
            );
        }
        assert!(icons.evict());
        assert!(!icons.entries.contains_key(&18));
        assert!(icons.evict());
        assert!(!icons.evict()); // only the loading icon remains
        assert_eq!((icons.len, icons.pending), (1, 1));
    }
}
