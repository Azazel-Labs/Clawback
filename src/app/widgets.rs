//! Small widgets shared by the app's dialogs and panels.
/// Shared modal heading with an accessible, consistently placed dismiss control.
pub(super) fn dialog_header(ui: &mut eframe::egui::Ui, title: &str, icon: &str) -> bool {
    use eframe::egui::{self, Align, Layout, RichText};
    let mut close = false;
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close = ui
                .add(
                    egui::Button::new(RichText::new(egui_phosphor::regular::X).size(18.0))
                        .frame(false)
                        .min_size(egui::vec2(30.0, 30.0)),
                )
                .on_hover_text(crate::i18n::tr!("close-dialog"))
                .clicked();
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.label(RichText::new(icon).size(24.0).color(crate::theme::ACCENT));
                ui.add(egui::Label::new(RichText::new(title).size(22.0).strong()).wrap());
            });
        });
    });
    close
}

/// A link that opens another application or page: accent text with an external-link
/// icon, underlined and brightened on hover.
pub(super) fn external_link(ui: &mut eframe::egui::Ui, label: &str) -> eframe::egui::Response {
    use eframe::egui::{self, Sense, Stroke, TextStyle, WidgetText};
    let text = WidgetText::from(format!("{label}  {}", egui_phosphor::regular::ARROW_SQUARE_OUT));
    let galley = text.into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, TextStyle::Button);
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::click());
    let lit = response.hovered() || response.has_focus();
    let color = if lit { crate::theme::ACCENT.lerp_to_gamma(egui::Color32::WHITE, 0.35) } else { crate::theme::ACCENT };
    let painter = ui.painter();
    painter.galley(rect.min, galley, color);
    if lit {
        painter.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, color));
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// `text` shortened with "…" to fit `width` points: at the end, or at the start where the
/// end matters most, as in paths.
pub(super) fn fit_text(
    painter: &eframe::egui::Painter,
    text: &str,
    font: &eframe::egui::FontId,
    width: f32,
    keep_end: bool,
) -> String {
    let fits =
        |s: &str| painter.layout_no_wrap(s.to_owned(), font.clone(), eframe::egui::Color32::WHITE).size().x <= width;
    if fits(text) {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let kept = |n: usize| -> String {
        if keep_end {
            std::iter::once('…').chain(chars[chars.len() - n..].iter().copied()).collect()
        } else {
            chars[..n].iter().copied().chain(std::iter::once('…')).collect()
        }
    };
    // The most characters that still fit.
    let (mut low, mut high) = (0, chars.len());
    while low < high {
        let middle = (low + high).div_ceil(2);
        if fits(&kept(middle)) { low = middle } else { high = middle - 1 }
    }
    kept(low)
}

/// A strip of tabs on a shared baseline, the active one underlined in the accent color.
pub(super) fn tabs<T: Copy + PartialEq>(ui: &mut eframe::egui::Ui, current: &mut T, tabs: &[(T, String)]) {
    use crate::theme;
    use eframe::egui::{FontId, Rect, Sense, Stroke, pos2, vec2};
    let height = 30.0;
    let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter().clone();
    painter.hline(strip.x_range(), strip.bottom() - 0.5, Stroke::new(1.0, theme::PANEL_EDGE));
    let mut left = strip.left();
    for (index, (value, label)) in tabs.iter().enumerate() {
        let galley = painter.layout_no_wrap(label.clone(), FontId::proportional(13.0), theme::TEXT);
        let tab = Rect::from_min_size(pos2(left, strip.top()), vec2(galley.size().x + 24.0, height));
        let response = ui
            .interact(tab, ui.id().with(("tab", index)), Sense::click())
            .on_hover_cursor(eframe::egui::CursorIcon::PointingHand);
        if response.clicked() {
            *current = *value;
        }
        let active = *current == *value;
        if response.hovered() && !active {
            painter.rect_filled(tab.shrink2(vec2(2.0, 3.0)), 4.0, theme::ROW_ALT);
        }
        let color = if active || response.hovered() { theme::TEXT } else { theme::MUTED };
        painter.galley_with_override_text_color(tab.center() - galley.size() / 2.0, galley, color);
        if active {
            let underline = Rect::from_min_max(
                pos2(tab.left() + 6.0, strip.bottom() - 2.5),
                pos2(tab.right() - 6.0, strip.bottom()),
            );
            painter.rect_filled(underline, 1.0, theme::ACCENT);
        }
        left = tab.right() + 2.0;
    }
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, pos2, vec2};

    #[test]
    fn modal_header_leaves_room_for_the_content() {
        let ctx = egui::Context::default();
        for height in [600.0, 1280.0] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(900.0, height))),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |root| {
                egui::Modal::new(egui::Id::new("header-size-test")).show(root.ctx(), |ui| {
                    ui.set_width(500.0);
                    let top = ui.cursor().top();
                    dialog_header(ui, "Uninstall", egui_phosphor::regular::TRASH);
                    let header_height = ui.cursor().top() - top;
                    assert!(header_height < 64.0, "header consumed {header_height} points");
                    ui.label("Application name");
                    ui.button("Uninstall through Steam").clicked();
                    assert!(ui.min_rect().height() < 150.0, "popup should fit its contents");
                });
            });
            output.textures_delta.clear();
        }
    }
}
