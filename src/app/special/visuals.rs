//! Special-location visuals, action labels and hints. The treemap only supplies geometry.
use super::{Inventory, Kind};
use crate::{i18n::tr, maprender, theme};
use clawback_core::{NodeId, Tree, layout::DisplayBox};
use std::borrow::Cow;

const DISK_BADGE_HEIGHT: i32 = 96;
use eframe::egui::{self, Color32, FontId, Pos2, Rect, pos2, vec2};

#[derive(Default)]
pub struct Visuals {
    recycle_bin_icons: [Option<egui::TextureHandle>; 2],
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
                Kind::TempFolder => {
                    painter.text(
                        body.center(),
                        egui::Align2::CENTER_CENTER,
                        egui_phosphor::regular::BROOM,
                        FontId::proportional(side),
                        theme::TEXT,
                    );
                }
                Kind::WslDisk | Kind::InstalledSoftware => {}
            }
        }
    }
}

impl Kind {
    pub fn action_enabled(self, deletion_disabled: bool) -> bool {
        self == Self::WslDisk || !deletion_disabled
    }

    pub fn menu_entry(self) -> (&'static str, String) {
        use egui_phosphor::regular as icon;
        match self {
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
        if self.cells.iter().flatten().any(|cell| cell.node == node && cell.kind == Kind::TempFolder) {
            return Cow::Borrowed("Temp");
        }
        let name = tree.node(node).name_lossy();
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

    /// Preserve neighboring geometry while giving each cleanup folder one clickable cell.
    pub fn collapse_layout(self, boxes: &mut Vec<DisplayBox>) {
        for node in self.collapsed() {
            let Some(cell) = boxes.iter().find(|tile| tile.node() == Some(node)).copied() else { continue };
            let inside = |tile: &DisplayBox| {
                tile.x >= cell.x
                    && tile.y >= cell.y
                    && tile.x + tile.w <= cell.x + cell.w
                    && tile.y + tile.h <= cell.y + cell.h
            };
            boxes.retain(|tile| tile.node() == Some(node) || !inside(tile));
        }
    }
}
