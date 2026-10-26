//! Special-location visuals, action labels and hints. The treemap only supplies geometry.
use super::{Inventory, Kind};
use crate::{i18n::tr, maprender, theme};
use clawback_core::{NodeId, Tree, format, layout::DisplayBox};
use std::borrow::Cow;

const DISK_BADGE_HEIGHT: i32 = 96;
use eframe::egui::{self, Color32, FontId, Pos2, Rect, pos2, vec2};

#[cfg(windows)]
use super::windows::programs::steam::{self, Art};

#[derive(Default)]
pub struct Visuals {
    recycle_bin_icons: [Option<egui::TextureHandle>; 2],
    #[cfg(windows)]
    games: GameArt,
}

/// Steam's cached library images, decoded on workers and kept as textures.
#[cfg(windows)]
#[derive(Default)]
pub struct GameArt(std::collections::HashMap<(u32, Art), ArtSlot>);

#[cfg(windows)]
enum ArtSlot {
    Loading(Option<crate::background::Job<(), Option<egui::ColorImage>>>),
    Ready(Option<egui::TextureHandle>),
}

impl Visuals {
    pub fn paint(
        &mut self,
        painter: &egui::Painter,
        tree: &Tree,
        inventory: Inventory,
        boxes: &[DisplayBox],
        origin: Pos2,
    ) {
        // File providers are recognized individually, rather than collapsed like cleanup folders.
        for tile in boxes.iter().filter(|tile| !tile.folder && tile.w >= 64 && tile.h >= DISK_BADGE_HEIGHT) {
            let Some(node) = tile.node() else { continue };
            if !tree.node(node).name_lossy().to_ascii_lowercase().ends_with(".vhdx")
                || inventory.classify(tree, node) != Some(Kind::WslDisk)
            {
                continue;
            }
            let rect = maprender::screen_rect(tile, origin).shrink(3.0);
            let clipped = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
            let badge = Rect::from_min_size(rect.min, vec2(rect.width().min(114.0), 22.0));
            clipped.rect_filled(badge, 3.0, theme::NOTE_BG);
            clipped.text(
                badge.left_center() + vec2(5.0, 0.0),
                egui::Align2::LEFT_CENTER,
                if tile.w >= 120 {
                    format!("{}  VHDX · WSL", egui_phosphor::regular::HARD_DRIVES)
                } else {
                    format!("{}  VHDX", egui_phosphor::regular::HARD_DRIVES)
                },
                FontId::proportional(11.0),
                theme::ACCENT,
            );
        }
        for cell in inventory.cells.iter().flatten() {
            let Some(tile) = boxes.iter().find(|tile| tile.node() == Some(cell.node)) else { continue };
            let rect = maprender::screen_rect(tile, origin);
            let body = Rect::from_min_max(rect.min + vec2(0.0, maprender::LINE), rect.max).shrink(4.0);
            let side = body.width().min(body.height()).min(128.0);
            if side < 16.0 {
                continue;
            }
            match cell.kind {
                Kind::RecycleBin => {
                    let full = tree.get(cell.node).is_some_and(|node| node.size > 0);
                    let slot = &mut self.recycle_bin_icons[usize::from(full)];
                    #[cfg(windows)]
                    if slot.is_none()
                        && let Some(image) = super::windows::recycle_bin::icon(full, 128)
                    {
                        *slot = Some(painter.ctx().load_texture("recycle-bin", image, egui::TextureOptions::LINEAR));
                    }
                    if let Some(texture) = slot {
                        painter.image(
                            texture.id(),
                            Rect::from_center_size(body.center(), vec2(side, side)),
                            Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                }
                Kind::TempFolder | Kind::WindowsOs => {
                    painter.text(
                        body.center(),
                        egui::Align2::CENTER_CENTER,
                        if cell.kind == Kind::WindowsOs {
                            egui_phosphor::regular::WINDOWS_LOGO
                        } else {
                            egui_phosphor::regular::BROOM
                        },
                        FontId::proportional(side),
                        theme::TEXT,
                    );
                }
                Kind::WslDisk | Kind::InstalledSoftware => {}
            }
        }
        #[cfg(windows)]
        self.paint_games(painter, tree, boxes, origin);
    }
}

#[cfg(windows)]
impl Visuals {
    /// Steam's library art below each game tile's label: the header on wide tiles, the
    /// capsule on tall ones, and the icon where neither fits.
    fn paint_games(&mut self, painter: &egui::Painter, tree: &Tree, boxes: &[DisplayBox], origin: Pos2) {
        for tile in boxes.iter().filter(|tile| maprender::is_collapsed_folder(tree, tile)) {
            let Some(node) = tile.node() else { continue };
            let Some(game) = steam::game_at(tree, node) else { continue };
            let rect = maprender::screen_rect(tile, origin);
            if rect.width() < 24.0 || rect.height() < 14.0 {
                continue;
            }
            let clip = painter.with_clip_rect(rect.shrink(1.0).intersect(painter.clip_rect()));
            // The name, and the size where there is room, in a band across the top.
            let two_lines = rect.width() >= 88.0 && rect.height() >= 56.0;
            let band = Rect::from_min_size(rect.min, vec2(rect.width(), if two_lines { 34.0 } else { 18.0 }));
            let body = Rect::from_min_max(pos2(rect.left(), band.bottom()), rect.max).shrink(3.0);
            if body.width() >= 20.0 && body.height() >= 20.0 {
                let preferred: &[Art] = if body.width() < 96.0 || body.height() < 48.0 {
                    &[Art::Icon, Art::Header, Art::Capsule]
                } else if body.height() > body.width() * 1.2 {
                    &[Art::Capsule, Art::Hero, Art::Header, Art::Icon]
                } else if body.width() > 480.0 {
                    &[Art::Hero, Art::Header, Art::Capsule, Art::Icon]
                } else {
                    &[Art::Header, Art::Hero, Art::Capsule, Art::Icon]
                };
                if let Some((art, texture)) = self.games.first(painter.ctx(), game.id, preferred) {
                    let size = texture.size_vec2();
                    let (target, uv) = if art == Art::Hero {
                        // The banner has no title, so it may be cropped to fill the tile.
                        let scale = (body.width() / size.x).max(body.height() / size.y);
                        let shown = body.size() / (size * scale);
                        (body, Rect::from_center_size(pos2(0.5, 0.5), shown))
                    } else {
                        // Others keep their whole picture; a small icon is never blown up past 2x.
                        let fit = (body.width() / size.x).min(body.height() / size.y);
                        let scale = if art == Art::Icon { fit.min(2.0) } else { fit };
                        (
                            Rect::from_center_size(body.center(), size * scale),
                            Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                        )
                    };
                    clip.image(texture.id(), target, uv, Color32::WHITE);
                }
            }
            clip.rect_filled(band, 0.0, Color32::from_black_alpha(120));
            let name_font = FontId::proportional(12.0);
            let name = crate::app::widgets::fit_text(painter, &game.name, &name_font, band.width() - 8.0, false);
            clip.text(band.left_top() + vec2(4.0, 2.0), egui::Align2::LEFT_TOP, name, name_font, theme::TEXT);
            if two_lines {
                clip.text(
                    band.left_top() + vec2(4.0, 18.0),
                    egui::Align2::LEFT_TOP,
                    format::size(tree.node(node).size),
                    FontId::proportional(11.0),
                    theme::MUTED,
                );
            }
        }
    }
}

#[cfg(windows)]
impl GameArt {
    /// The first of `preferred` that Steam has cached, once decoded, and which one it is.
    pub fn first(&mut self, ctx: &egui::Context, id: u32, preferred: &[Art]) -> Option<(Art, egui::TextureHandle)> {
        preferred.iter().find_map(|&art| self.texture(ctx, id, art).map(|texture| (art, texture)))
    }

    /// A game's image once it is decoded; `None` while loading or when Steam has none cached.
    fn texture(&mut self, ctx: &egui::Context, id: u32, art: Art) -> Option<egui::TextureHandle> {
        let slot = self.0.entry((id, art)).or_insert_with(|| {
            ArtSlot::Loading(Some(crate::background::Job::spawn((), ctx, move || {
                let bytes = std::fs::read(steam::art_path(id, art)?).ok()?;
                let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
                let size = [image.width() as usize, image.height() as usize];
                Some(egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()))
            })))
        });
        if let ArtSlot::Loading(job) = slot
            && let Some(((), image)) = crate::background::Job::poll(job)
        {
            *slot = ArtSlot::Ready(
                image.map(|image| ctx.load_texture(format!("steam-{id}-{art:?}"), image, egui::TextureOptions::LINEAR)),
            );
        }
        match slot {
            ArtSlot::Ready(texture) => texture.clone(),
            ArtSlot::Loading(_) => None,
        }
    }
}

impl Kind {
    pub fn action_enabled(self, deletion_disabled: bool) -> bool {
        matches!(self, Self::WslDisk | Self::WindowsOs) || !deletion_disabled
    }

    pub fn menu_entry(self) -> (&'static str, String) {
        use egui_phosphor::regular as icon;
        match self {
            Self::WindowsOs => (icon::GEAR, tr!("open-storage-settings")),
            Self::RecycleBin => (icon::TRASH, tr!("empty-recycle-bin-button")),
            Self::TempFolder => (icon::BROOM, tr!("empty-temp-button")),
            Self::WslDisk => (icon::ARROWS_CLOCKWISE, tr!("reclaim-wsl-space")),
            Self::InstalledSoftware => (icon::TRASH, tr!("uninstall-software")),
        }
    }

    pub fn hint(self, deletion_disabled: bool) -> Option<String> {
        if !self.action_enabled(deletion_disabled) {
            return None;
        }
        match self {
            Self::WindowsOs => Some(tr!("click-to-open-storage-settings")),
            Self::RecycleBin => Some(tr!("click-to-empty-recycle-bin")),
            Self::TempFolder => Some(tr!("click-to-clean-temp-folder")),
            Self::WslDisk => Some(tr!("click-to-reclaim-wsl-space")),
            Self::InstalledSoftware => Some(tr!("click-to-uninstall-software")),
        }
    }
}

impl Inventory {
    /// Presentation owns special names, format recognition, and compact tile icons.
    pub fn label<'a>(self, tree: &'a Tree, node: NodeId, tile: &DisplayBox) -> Cow<'a, str> {
        match self.cells.iter().flatten().find(|cell| cell.node == node).map(|cell| cell.kind) {
            Some(Kind::TempFolder) => return Cow::Borrowed("Temp"),
            Some(Kind::WindowsOs) => {
                return Cow::Owned(
                    if tree.node(node).is_dir() && tree.node(node).name_lossy() != "System Volume Information" {
                        tr!("windows-os")
                    } else {
                        tr!("windows-system-files")
                    },
                );
            }
            _ => {}
        }
        let entry = tree.node(node);
        // Discovery needs the scan's mount state; the name alone is enough to drop the `$`.
        if cfg!(windows)
            && entry.parent == clawback_core::ROOT
            && entry.is_dir()
            && entry.name_lossy().eq_ignore_ascii_case("$Recycle.Bin")
        {
            return Cow::Owned(tr!("recycle-bin"));
        }
        #[cfg(windows)]
        if !tile.folder
            && let Some(game) = steam::game_at(tree, node)
        {
            return Cow::Owned(game.name);
        }
        let name = entry.name_lossy();
        if !tile.folder
            && tile.h < DISK_BADGE_HEIGHT
            && name.to_ascii_lowercase().ends_with(".vhdx")
            && self.classify(tree, node) == Some(Kind::WslDisk)
        {
            Cow::Owned(format!("{} {name}", egui_phosphor::regular::HARD_DRIVES))
        } else {
            name
        }
    }

    /// Tip lines for the Windows OS cell: each entry drawn inside it, and why none is offered
    /// for deletion. Empty for other cells.
    pub fn group_tip(self, tree: &Tree, node: NodeId) -> Vec<String> {
        let grouped = self.grouped();
        if grouped.first() != Some(&node) || grouped.len() < 2 {
            return Vec::new();
        }
        let mut lines: Vec<_> = grouped
            .iter()
            .map(|&member| {
                let member = tree.node(member);
                format!("{}  ·  {}", member.name_lossy(), format::size(member.len))
            })
            .collect();
        lines.push(tr!("managed-by-windows"));
        lines
    }

    /// Preserve neighboring geometry while giving each cleanup folder one clickable cell.
    pub fn collapse_layout(self, boxes: &mut Vec<DisplayBox>) {
        for node in self.collapsed() {
            empty(boxes, node);
        }
    }
}

/// Drop the boxes drawn inside `node`'s box, keeping the box itself.
fn empty(boxes: &mut Vec<DisplayBox>, node: NodeId) {
    let Some(cell) = boxes.iter().find(|tile| tile.node() == Some(node)).copied() else { return };
    let inside = |tile: &DisplayBox| {
        tile.x >= cell.x && tile.y >= cell.y && tile.x + tile.w <= cell.x + cell.w && tile.y + tile.h <= cell.y + cell.h
    };
    boxes.retain(|tile| tile.node() == Some(node) || !inside(tile));
}

/// Draw each installed Steam game as one tile, like a file: no title band and no contents.
pub fn collapse_games(tree: &Tree, boxes: &mut Vec<DisplayBox>) {
    #[cfg(windows)]
    {
        let games: Vec<NodeId> = boxes
            .iter()
            .filter(|tile| tile.folder)
            .filter_map(DisplayBox::node)
            .filter(|&node| steam::game_at(tree, node).is_some())
            .collect();
        for node in games {
            empty(boxes, node);
            if let Some(tile) = boxes.iter_mut().find(|tile| tile.node() == Some(node)) {
                tile.folder = false;
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (tree, boxes);
}
