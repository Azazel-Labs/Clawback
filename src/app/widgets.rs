//! Small widgets shared by the app's dialogs and panels.
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
