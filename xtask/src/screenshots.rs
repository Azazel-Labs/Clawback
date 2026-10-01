//! Capture the app's fictional dataset and render portable marketing images.
use crate::{Result, hex};
use ab_glyph::{Font, FontRef, point};
use std::{
    fmt::Write as _,
    fs,
    io::BufWriter,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

pub const USAGE: &str = "cargo xtask screenshots [--render-only]";

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    let render_only = match args {
        [] => false,
        [arg] if arg == "--render-only" => true,
        _ => return Err(crate::usage(USAGE)),
    };
    let target = root.join("target");
    if !render_only {
        println!("Building the fictional demo capture feature…");
        crate::run_checked(
            crate::cargo(root)
                .args(["build", "--package", "clawback", "--features", "screenshots", "--locked", "--target-dir"])
                .arg(&target),
            "Screenshot build failed",
        )?;
        let capture = target.join("demo-capture");
        fs::create_dir_all(&capture)?;
        let binary = format!("clawback{}", std::env::consts::EXE_SUFFIX);
        let executable = capture.join(&binary);
        fs::copy(target.join("debug").join(&binary), &executable)?;
        for (name, view) in [("desktop", ""), ("explore", "Projects")] {
            println!("Capturing {name} (fictional demo drive)…");
            let output = target.join(format!("demo-{name}.ppm"));
            capture_app(&executable, &output, Some(view))?;
        }
        capture_app(&executable, &target.join("demo-terminal.tsv"), None)?;
    }
    let out = root.join("docs/images");
    fs::create_dir_all(&out)?;
    write_palettes(&out.join("palettes.svg"))?;
    for name in ["desktop", "explore"] {
        let data = fs::read(target.join(format!("demo-{name}.ppm")))?;
        let (width, height, rgb) = ppm(&data)?;
        write_png(&out.join(format!("{name}.png")), width, height, rgb)?;
    }
    let terminal = fs::read_to_string(target.join("demo-terminal.tsv"))?;
    let (width, height, pixels, svg) = render_terminal(&terminal)?;
    fs::write(out.join("terminal.svg"), svg)?;
    write_png(&out.join("terminal.png"), width, height, &pixels)?;
    println!("Wrote desktop.png, explore.png, terminal.png and terminal.svg in docs/images.");
    Ok(())
}

fn write_palettes(path: &Path) -> Result<()> {
    use clawback_core::palette::{DEFAULT_MAP_SCHEME, MAP_PRESETS, map_color};
    let height = MAP_PRESETS.len() * 54 + 24;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"900\" height=\"{height}\" viewBox=\"0 0 900 {height}\"><title>Clawback map palettes, Electric default</title><rect width=\"900\" height=\"{height}\" fill=\"#181818\"/>\n"
    );
    for (row, (scheme, name)) in MAP_PRESETS.iter().enumerate() {
        let y = row * 54 + 12;
        let suffix = if *scheme == DEFAULT_MAP_SCHEME { " · default" } else { "" };
        writeln!(
            svg,
            "<text x=\"18\" y=\"{}\" fill=\"#eeeeee\" font-family=\"sans-serif\" font-size=\"16\">{name}{suffix}</text>",
            y + 27
        )?;
        for depth in 0..8 {
            writeln!(
                svg,
                "<rect x=\"{}\" y=\"{y}\" width=\"79\" height=\"40\" rx=\"3\" fill=\"{}\"/>",
                214 + depth * 84,
                hex(map_color(*scheme, depth))
            )?;
        }
    }
    svg.push_str("</svg>\n");
    fs::write(path, svg)?;
    Ok(())
}

fn capture_app(executable: &Path, output: &Path, view: Option<&str>) -> Result<()> {
    if output.exists() {
        fs::remove_file(output)?;
    }
    let mut command = Command::new(executable);
    for key in ["CLAWBACK_DEMO_CAPTURE", "CLAWBACK_DEMO_VIEW", "CLAWBACK_TERMINAL_CAPTURE"] {
        command.env_remove(key);
    }
    if let Some(view) = view {
        command.arg("--gui").env("CLAWBACK_DEMO_CAPTURE", output).env("CLAWBACK_DEMO_VIEW", view);
    } else {
        command.env("CLAWBACK_TERMINAL_CAPTURE", output);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no extra console.
    }
    let mut child = command.spawn()?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() || !output.is_file() {
                return Err(format!("Capture failed ({status}): {}", output.display()).into());
            }
            return Ok(());
        }
        if started.elapsed() > Duration::from_secs(120) {
            child.kill()?;
            child.wait()?;
            return Err("Capture timed out; desktop captures require a graphical session and working GPU driver".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn dimensions(line: &str) -> Result<(u32, u32)> {
    let parts: Vec<_> = line.split_whitespace().collect();
    if parts.len() != 2 {
        return Err("Expected width and height".into());
    }
    let width: u32 = parts[0].parse()?;
    let height: u32 = parts[1].parse()?;
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err("Capture dimensions out of range".into());
    }
    Ok((width, height))
}

fn ppm(data: &[u8]) -> Result<(u32, u32, &[u8])> {
    let mut parts = data.splitn(4, |b| *b == b'\n');
    if parts.next() != Some(b"P6") {
        return Err("Expected P6 capture".into());
    }
    let (width, height) = dimensions(std::str::from_utf8(parts.next().ok_or("Missing PPM dimensions")?)?)?;
    if parts.next() != Some(b"255") {
        return Err("Expected 8-bit PPM capture".into());
    }
    let rgb = parts.next().ok_or("Missing PPM pixels")?;
    if rgb.len() != width as usize * height as usize * 3 {
        return Err("Truncated PPM capture".into());
    }
    Ok((width, height, rgb))
}

fn write_png(path: &Path, width: u32, height: u32, rgb: &[u8]) -> Result<()> {
    let mut encoder = png::Encoder::new(BufWriter::new(fs::File::create(path)?), width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgb)?;
    writer.finish()?;
    Ok(())
}

fn color(input: &str, reset: [u8; 3]) -> Result<[u8; 3]> {
    if input == "Reset" {
        return Ok(reset);
    }
    if let Some(rgb) = input.strip_prefix("Rgb(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<u8> = rgb.split(',').map(|v| v.trim().parse()).collect::<std::result::Result<_, _>>()?;
        return parts.try_into().map_err(|_| "Expected RGB triplet".into());
    }
    let colors = [
        ("Black", [0, 0, 0]),
        ("White", [255, 255, 255]),
        ("Gray", [170, 170, 170]),
        ("DarkGray", [85, 85, 85]),
        ("Red", [170, 0, 0]),
        ("Green", [0, 170, 0]),
        ("Yellow", [170, 170, 0]),
        ("Blue", [0, 0, 170]),
        ("Magenta", [170, 0, 170]),
        ("Cyan", [0, 170, 170]),
        ("LightRed", [255, 85, 85]),
        ("LightGreen", [85, 255, 85]),
        ("LightYellow", [255, 255, 85]),
        ("LightBlue", [85, 85, 255]),
        ("LightMagenta", [255, 85, 255]),
        ("LightCyan", [85, 255, 255]),
    ];
    colors
        .iter()
        .find(|(name, _)| *name == input)
        .map(|(_, rgb)| *rgb)
        .ok_or_else(|| format!("Unsupported terminal color: {input}").into())
}

fn escape_xml(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

fn render_terminal(tsv: &str) -> Result<(u32, u32, Vec<u8>, String)> {
    let mut lines = tsv.lines();
    let (cols, rows) = dimensions(lines.next().ok_or("Empty terminal capture")?)?;
    let (width, height) = (cols * 10, rows * 20);
    if width > 8192 || height > 8192 {
        return Err("Terminal capture too large".into());
    }
    let cells: Vec<_> = lines.collect();
    if cells.len() != (cols * rows) as usize {
        return Err("Terminal cell count mismatch".into());
    }
    let font = FontRef::try_from_slice(epaint_default_fonts::HACK_REGULAR)?;
    let mut pixels = vec![0; (width * height * 3) as usize];
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n<title>Clawback terminal interface — fictional demo drive</title>\n"
    );
    let mut text = String::new();
    // Paint every background before glyphs so adjacent cells never erase glyph edges.
    let mut glyphs = Vec::new();
    for (index, line) in cells.iter().enumerate() {
        let parts: Vec<_> = line.split('\t').collect();
        if parts.len() != 3 {
            return Err("Malformed terminal cell".into());
        }
        let fg = color(parts[1], [216, 216, 216])?;
        let bg = color(parts[2], [15, 20, 30])?;
        let x = index as u32 % cols * 10;
        let y = index as u32 / cols * 20;
        for dy in 0..20 {
            for dx in 0..10 {
                let offset = (((y + dy) * width + x + dx) * 3) as usize;
                pixels[offset..offset + 3].copy_from_slice(&bg);
            }
        }
        writeln!(svg, "<rect x=\"{x}\" y=\"{y}\" width=\"10\" height=\"20\" fill=\"{}\"/>", hex(bg))?;
        if !parts[0].trim().is_empty() {
            writeln!(text, "<text x=\"{x}\" y=\"{}\" fill=\"{}\">{}</text>", y + 15, hex(fg), escape_xml(parts[0]))?;
            for c in parts[0].chars() {
                let id = font.glyph_id(c);
                if id.0 == 0 {
                    return Err(format!("Embedded font is missing character {c:?}").into());
                }
                glyphs.push((id.with_scale_and_position(20.0, point(x as f32, (y + 15) as f32)), fg));
            }
        }
    }
    for (glyph, fg) in glyphs {
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|dx, dy, coverage| {
                let x = bounds.min.x as i32 + dx as i32;
                let y = bounds.min.y as i32 + dy as i32;
                if x >= 0 && y >= 0 && x < width as i32 && y < height as i32 {
                    let offset = (y as usize * width as usize + x as usize) * 3;
                    for channel in 0..3 {
                        pixels[offset + channel] = (f32::from(fg[channel]) * coverage
                            + f32::from(pixels[offset + channel]) * (1.0 - coverage))
                            .round() as u8;
                    }
                }
            });
        }
    }
    write!(
        svg,
        "<g font-family=\"Hack, Consolas, DejaVu Sans Mono, monospace\" font-size=\"16\" xml:space=\"preserve\">\n{text}</g>\n</svg>\n"
    )?;
    Ok((width, height, pixels, svg))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_ppm_preserves_whitespace_pixels_and_rejects_truncation() -> Result<()> {
        let (_, _, pixels) = ppm(b"P6\n1 1\n255\n\n\r\t")?;
        assert_eq!(pixels, b"\n\r\t");
        assert!(ppm(b"P6\n1 1\n255\n\0").is_err());
        Ok(())
    }
    #[test]
    fn terminal_renders_backgrounds_glyphs_and_escaped_svg() -> Result<()> {
        let (w, h, pixels, svg) = render_terminal("2 1\n&\tRgb(255, 255, 255)\tRgb(1, 2, 3)\n \tReset\tReset\n")?;
        assert_eq!((w, h), (20, 20));
        assert!(svg.contains("&amp;"));
        assert!(pixels.as_chunks::<3>().0.contains(&[1, 2, 3]));
        assert!(pixels.as_chunks::<3>().0.iter().any(|p| p[0] > 200));
        assert!(render_terminal("2 1\n \tReset\tReset\n").is_err());
        Ok(())
    }
}
