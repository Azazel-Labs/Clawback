//! Reproducible native icon assets, generated entirely in Rust.
use crate::Result;
use std::{fmt::Write as _, fs, path::Path};
const SIZES: &[u32] = &[16, 20, 24, 28, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 128, 256, 512, 1024];

fn png(size: u32) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut data, size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&clawback_core::icon::pixels(size))?;
    }
    Ok(data)
}

/// An uncompressed ICO image: Explorer only reliably decodes PNG entries at 256 px, so
/// smaller sizes use a 32-bit bottom-up DIB with an empty AND mask (alpha does the work).
fn ico_image(size: u32, png: &[u8]) -> Vec<u8> {
    if size >= 256 {
        return png.to_vec();
    }
    let rgba = clawback_core::icon::pixels(size);
    let mask_row = size.div_ceil(32) as usize * 4;
    let mut dib = Vec::with_capacity(40 + rgba.len() + mask_row * size as usize);
    for value in [40, size, size * 2] {
        dib.extend_from_slice(&value.to_le_bytes());
    }
    dib.extend_from_slice(&1u16.to_le_bytes()); // planes
    dib.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    dib.extend_from_slice(&[0; 24]); // BI_RGB, image size, resolution, palette
    for row in rgba.chunks_exact(size as usize * 4).rev() {
        for pixel in row.as_chunks::<4>().0 {
            dib.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    dib.resize(dib.len() + mask_row * size as usize, 0);
    dib
}

pub fn run(root: &Path) -> Result<()> {
    let out = root.join("assets/icons");
    fs::create_dir_all(&out)?;
    let images: Vec<_> = SIZES.iter().map(|&size| Ok((size, png(size)?))).collect::<Result<_>>()?;
    for (size, image) in &images {
        fs::write(out.join(format!("clawback-{size}.png")), image)?;
    }
    let ico_images: Vec<_> =
        images.iter().filter(|(size, _)| *size <= 256).map(|(size, png)| (*size, ico_image(*size, png))).collect();
    let mut ico = vec![0, 0, 1, 0];
    ico.extend_from_slice(&(ico_images.len() as u16).to_le_bytes());
    let mut offset = 6 + ico_images.len() as u32 * 16;
    for (size, data) in &ico_images {
        ico.extend_from_slice(&[*size as u8, *size as u8, 0, 0, 1, 0, 32, 0]); // 256 wraps to 0, as ICO requires
        ico.extend_from_slice(&(data.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &ico_images {
        ico.extend_from_slice(data);
    }
    fs::write(out.join("clawback.ico"), ico)?;
    let mut chunks = Vec::new();
    for (kind, size) in [
        ("icp4", 16),
        ("icp5", 32),
        ("icp6", 64),
        ("ic07", 128),
        ("ic08", 256),
        ("ic09", 512),
        ("ic10", 1024),
        ("ic11", 32),
        ("ic12", 64),
        ("ic13", 256),
        ("ic14", 512),
    ] {
        let data = png(size)?;
        chunks.extend_from_slice(kind.as_bytes());
        chunks.extend_from_slice(&(data.len() as u32 + 8).to_be_bytes());
        chunks.extend_from_slice(&data);
    }
    let mut icns = b"icns".to_vec();
    icns.extend_from_slice(&(chunks.len() as u32 + 8).to_be_bytes());
    icns.extend_from_slice(&chunks);
    fs::write(out.join("clawback.icns"), icns)?;
    let mut svg = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 64 64\">");
    for (x, y, w, h, c) in clawback_core::icon::BOXES {
        write!(
            svg,
            "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"#{:02x}{:02x}{:02x}\"/>",
            c[0], c[1], c[2]
        )?;
    }
    svg.push_str("</svg>\n");
    fs::write(out.join("clawback.svg"), svg)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_containers_include_valid_pngs_at_required_sizes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("workspace");
        let assets = root.join("assets/icons");
        let ico = fs::read(assets.join("clawback.ico")).expect("ico");
        assert_eq!(&ico[..4], &[0, 0, 1, 0]);
        let count = u16::from_le_bytes(ico[4..6].try_into().expect("count")) as usize;
        assert_eq!(count, SIZES.iter().filter(|&&size| size <= 256).count());
        for i in 0..count {
            let entry = &ico[6 + i * 16..6 + (i + 1) * 16];
            let length = u32::from_le_bytes(entry[8..12].try_into().expect("length")) as usize;
            let offset = u32::from_le_bytes(entry[12..16].try_into().expect("offset")) as usize;
            let size = if entry[0] == 0 { 256 } else { u32::from(entry[0]) };
            let expected = png(size).expect("render");
            assert_eq!(&ico[offset..offset + length], ico_image(size, &expected));
            // Only the 256 px entry may be PNG; Explorer falls back to a generic icon otherwise.
            assert_eq!(ico[offset..].starts_with(b"\x89PNG"), size == 256);
        }
        let icns = fs::read(assets.join("clawback.icns")).expect("icns");
        assert_eq!(&icns[..4], b"icns");
        assert_eq!(u32::from_be_bytes(icns[4..8].try_into().expect("length")) as usize, icns.len());
        let mut offset = 8;
        let mut count = 0;
        while offset < icns.len() {
            let length = u32::from_be_bytes(icns[offset + 4..offset + 8].try_into().expect("chunk")) as usize;
            let decoder = png::Decoder::new(std::io::Cursor::new(&icns[offset + 8..offset + length]));
            let reader = decoder.read_info().expect("PNG");
            assert!(matches!(reader.info().width, 16 | 32 | 64 | 128 | 256 | 512 | 1024));
            offset += length;
            count += 1;
        }
        assert_eq!(count, 11);
        assert_eq!(offset, icns.len());
    }
}
