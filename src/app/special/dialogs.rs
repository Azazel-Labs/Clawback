//! Feature-specific presentation and command execution.
mod cleanup;
mod programs;
mod wsl;

/// Shared modal heading with an accessible, consistently placed dismiss control.
fn header(ui: &mut eframe::egui::Ui, title: &str, icon: &str) -> bool {
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
                    header(ui, "Uninstall", egui_phosphor::regular::TRASH);
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
