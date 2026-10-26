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
    game_art: std::collections::HashMap<(u32, Art), GameArt>,
}

#[cfg(windows)]
enum GameArt {
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
        for tile in boxes.iter().filter(|tile| !tile.folder && tile.w >= 40 && tile.h >= 40) {
            let Some(node) = tile.node() else { continue };
            if !tree.node(node).is_dir() {
                continue;
            }
            let Some(game) = steam::game_at(tree, node) else { continue };
            let rect = maprender::screen_rect(tile, origin);
            let lines = if maprender::shows_details(tile) { 3.0 } else { 1.0 };
            let body = Rect::from_min_max(rect.min + vec2(0.0, lines * maprender::LINE + 2.0), rect.max).shrink(4.0);
            if body.width() < 20.0 || body.height() < 20.0 {
                continue;
            }
            let preferred = if body.width() < 96.0 || body.height() < 48.0 {
                [Art::Icon, Art::Header, Art::Capsule]
            } else if body.height() > body.width() * 1.2 {
                [Art::Capsule, Art::Header, Art::Icon]
            } else {
                [Art::Header, Art::Capsule, Art::Icon]
            };
            let Some(texture) = preferred.into_iter().find_map(|art| self.game_texture(painter.ctx(), game.id, art))
            else {
                continue;
            };
            let size = texture.size_vec2();
            let mut scale = (body.width() / size.x).min(body.height() / size.y);
            if preferred[0] == Art::Icon {
                scale = scale.min(64.0 / size.x.max(size.y));
            }
            painter.with_clip_rect(rect.intersect(painter.clip_rect())).image(
                texture.id(),
                Rect::from_center_size(body.center(), size * scale),
                Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    /// A game's image once it is decoded; `None` while loading or when Steam has none cached.
    fn game_texture(&mut self, ctx: &egui::Context, id: u32, art: Art) -> Option<egui::TextureHandle> {
        let slot = self.game_art.entry((id, art)).or_insert_with(|| {
            GameArt::Loading(Some(crate::background::Job::spawn((), ctx, move || {
                let bytes = std::fs::read(steam::art_path(id, art)?).ok()?;
                let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
                let size = [image.width() as usize, image.height() as usize];
                Some(egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()))
            })))
        });
        if let GameArt::Loading(job) = slot
            && let Some(((), image)) = crate::background::Job::poll(job)
        {
            *slot = GameArt::Ready(
                image.map(|image| ctx.load_texture(format!("steam-{id}-{art:?}"), image, egui::TextureOptions::LINEAR)),
            );
        }
        match slot {
            GameArt::Ready(texture) => texture.clone(),
            GameArt::Loading(_) => None,
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
