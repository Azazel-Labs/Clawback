//! Fictional in-memory data for reproducible marketing captures. No disk scan or watcher.
use clawback_core::{Kind, NodeId, ROOT, Tree, tree::NewEntry};
use std::path::Path;

/// The fictional drive's name and mount point.
pub(crate) const DRIVE: &str = "Demo Drive";

/// Where the fictional games are installed, below the drive.
#[cfg_attr(not(windows), allow(dead_code))] // Steam games are shown on Windows.
pub(crate) const STEAM_LIBRARY: &str = "Steam Library/steamapps/common";

/// Fictional installed games: (Steam app ID, name and folder, last played).
#[cfg_attr(not(windows), allow(dead_code))] // Steam games are shown on Windows.
pub(crate) const GAMES: [(u32, &str, Option<i64>); 4] = [
    (4_200_010, "Starfall", Some(1_779_800_000)),
    (4_200_020, "Neon Circuit", Some(1_771_000_000)),
    (4_200_030, "Lantern Keep", None),
    (4_200_040, "Tiny Orchard", Some(1_756_000_000)),
];

/// The pattern drawn over a fictional game's art.
#[cfg_attr(not(windows), allow(dead_code))] // Steam games are shown on Windows.
#[derive(Clone, Copy)]
enum Motif {
    Stars,
    Grid,
    Lantern,
    Orchard,
}

#[cfg_attr(not(windows), allow(dead_code))]
const STYLES: [Style; 4] = [
    Style { top: [12, 18, 52], bottom: [72, 28, 104], glow: [120, 170, 255], motif: Motif::Stars },
    Style { top: [24, 6, 44], bottom: [210, 40, 140], glow: [255, 170, 60], motif: Motif::Grid },
    Style { top: [30, 18, 10], bottom: [96, 52, 20], glow: [255, 196, 96], motif: Motif::Lantern },
    Style { top: [150, 214, 240], bottom: [70, 150, 70], glow: [255, 244, 190], motif: Motif::Orchard },
];

/// A fictional game's art: a gradient from `top` to `bottom`, a `glow`, and a motif.
#[cfg_attr(not(windows), allow(dead_code))]
struct Style {
    top: [u8; 3],
    bottom: [u8; 3],
    glow: [u8; 3],
    motif: Motif,
}

/// Generated library art of `size` for a fictional game, so captures never show a real
/// game's artwork. Square images are icons.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn game_art(id: u32, size: [usize; 2]) -> Option<eframe::egui::ColorImage> {
    let index = GAMES.iter().position(|game| game.0 == id)?;
    let Style { top, bottom, glow, motif } = STYLES[index];
    let [width, height] = size;
    let mix =
        |a: [f32; 3], b: [u8; 3], t: f32| std::array::from_fn::<f32, 3, _>(|i| a[i] + (f32::from(b[i]) - a[i]) * t);
    let mut rgba = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let (u, v) = (x as f32 / width as f32, y as f32 / height as f32);
            let mut color = mix(top.map(f32::from), bottom, v);
            // A soft glow, such as a planet, sun or lantern.
            let radius = width.min(height) as f32 * if width == height { 0.42 } else { 0.3 };
            let (dx, dy) = ((u - 0.66) * width as f32, (v - 0.42) * height as f32);
            let distance = (dx * dx + dy * dy).sqrt() / radius;
            if distance < 1.0 {
                color = mix(color, glow, (1.0 - distance).powf(0.7));
            }
            let hash = (x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663) ^ id;
            let hash = hash.wrapping_mul(2_654_435_761);
            match motif {
                Motif::Stars if hash.is_multiple_of(613) && distance > 1.0 => color = [235.0, 240.0, 255.0],
                Motif::Grid if v > 0.62 => {
                    let depth = (v - 0.62) / 0.38;
                    let across = ((u - 0.5) / (depth + 0.08) * 4.0).fract().abs();
                    let along = (1.0 / (depth + 0.06) * 2.0).fract();
                    if across < 0.04 || along < 0.06 {
                        color = mix(color, [255, 90, 220], 0.85);
                    }
                }
                Motif::Lantern => color = mix(color, [8, 4, 2], ((u - 0.5).abs() + (v - 0.5).abs()).min(1.0) * 0.6),
                Motif::Orchard if v > 0.55 => {
                    let (cell_x, cell_y) = ((u * 9.0).fract() - 0.5, ((v - 0.55) * 7.0).fract() - 0.5);
                    if cell_x * cell_x + cell_y * cell_y < 0.09 {
                        color = if hash.is_multiple_of(5) { [220.0, 50.0, 40.0] } else { [40.0, 110.0, 40.0] };
                    }
                }
                _ => {}
            }
            rgba.extend(color.map(|c| c.clamp(0.0, 255.0) as u8));
            rgba.push(255);
        }
    }
    Some(eframe::egui::ColorImage::from_rgba_unmultiplied(size, &rgba))
}

/// A capture run: fictional data, no saved state, and a screenshot written on exit.
pub fn capturing() -> bool {
    std::env::var_os("CLAWBACK_DEMO_CAPTURE").is_some()
}

pub fn tree() -> Tree {
    let mut tree = Tree::new(Path::new(DRIVE));
    // Deep, realistic paths: the map colours boxes by nesting depth, so files reach seven
    // folders down and every step of a palette (Blackbody Radiation's included) appears.
    let groups: &[(&str, &[(&str, u64)])] = &[
        // The fictional games in `GAMES`, installed where Steam puts them.
        (
            "Steam Library/steamapps/common/Starfall/Content/Paks/Windows",
            &[("worlds.pak", 24000), ("textures.pak", 16000)],
        ),
        ("Steam Library/steamapps/common/Starfall/Content/Audio", &[("audio.pak", 6000)]),
        ("Steam Library/steamapps/common/Neon Circuit/Content/Tracks/City/Night", &[("tracks.pak", 18000)]),
        ("Steam Library/steamapps/common/Neon Circuit/Content/Vehicles", &[("cars.pak", 9000)]),
        ("Steam Library/steamapps/common/Neon Circuit/Soundtrack", &[("soundtrack.ogg", 3000)]),
        ("Steam Library/steamapps/common/Lantern Keep/Data", &[("keep.pak", 7400), ("music.pak", 1100)]),
        ("Steam Library/steamapps/common/Tiny Orchard", &[("orchard.pak", 2600)]),
        ("Movies/Documentaries/Nature", &[("Ocean Expedition.mkv", 22000)]),
        ("Movies/Documentaries/Space", &[("Night Sky.mkv", 14000)]),
        ("Movies/Road Trips", &[("Desert Roads.mkv", 11000)]),
        (
            "Projects/Lunar Garden/Assets",
            &[
                ("Scenes/Environments.blend", 7000),
                ("Scenes/Characters.blend", 4500),
                ("Textures/4K/Terrain/terrain_albedo.exr", 3600),
                ("Textures/4K/Terrain/terrain_normal.exr", 2400),
                ("Textures/4K/Characters/hero_albedo.exr", 1900),
            ],
        ),
        (
            "Projects/Lunar Garden/Saved/Cooked/Windows/Content",
            &[("Maps/moon_base.umap", 5200), ("Maps/crater_rim.umap", 3100), ("Shaders/shaders.bin", 3500)],
        ),
        ("Projects/Lunar Garden/Builds/Windows/Shipping", &[("preview.zip", 6500)]),
        (
            "Projects/Studio Website",
            &[
                ("Design/design.fig", 1400),
                ("Public/Media/media.zip", 2800),
                ("node_modules/.cache/build-cache.bin", 1900),
                ("package.json", 0),
            ],
        ),
        (
            "Photos/2026/05 May/Coastal Weekend/RAW",
            &[("Sunrise.raw", 1800), ("Harbor.raw", 1400), ("Cliffs.raw", 2100)],
        ),
        ("Photos/2025/09 September/Mountain Trails/RAW", &[("Alpine Lake.raw", 2400), ("Summit.raw", 1800)]),
        ("Backups/Workstation/2026/May/Week 4", &[("workstation-may.zip", 26000)]),
        ("Backups/Workstation/2026/April", &[("workstation-april.zip", 23000)]),
        ("Backups/Phone/2026", &[("phone-backup.zip", 10000)]),
        ("Downloads", &[("texture-library.zip", 9500)]),
        ("Downloads/Audio", &[("sample-pack.zip", 6200)]),
        ("Downloads/Installers/Archive", &[("old-installer.iso", 4800)]),
        ("Music/Library/Electronic/Synthwave", &[("Synthwave Collection.flac", 3200)]),
        ("Music/Library/Classical/Piano", &[("Piano Sessions.flac", 1900)]),
        ("Music/Recordings/Field/2026", &[("Field Recordings.wav", 4200)]),
    ];
    for (folder, files) in groups {
        for (name, megabytes) in *files {
            let parts: Vec<_> = folder.split('/').chain(name.split('/')).collect();
            let mut parent = ROOT;
            for (index, name) in parts.iter().enumerate() {
                if let Some(existing) = tree.child_named(parent, std::ffi::OsStr::new(name)) {
                    parent = existing;
                    continue;
                }
                let kind = if index + 1 == parts.len() { Kind::File } else { Kind::Dir };
                let size = if kind == Kind::File { megabytes * 1024 * 1024 } else { 0 };
                parent = tree
                    .add_children(
                        parent,
                        vec![NewEntry {
                            name: (*name).into(),
                            kind,
                            size,
                            len: size,
                            mtime: 1_779_984_000,
                            flags: 0,
                            file_id: None,
                        }],
                    )
                    .start;
            }
        }
    }
    tree.sort_all();
    tree
}

/// A fictional earlier snapshot of [`tree`], for capturing the Changes view: May's backup
/// had not been made yet, a game was smaller and the downloads held more.
pub fn baseline(tree: &Tree) -> clawback_core::snapshot::Snapshot {
    let mut earlier = tree.clone();
    let find = |tree: &Tree, path: &str| tree.find_path(&tree.root_path().join(path)).expect("demo path");
    let resize = |tree: &mut Tree, path: &str, megabytes: u64| {
        let node = find(tree, path);
        let size = megabytes * 1024 * 1024;
        tree.update_file(node, NewEntry::file(tree.node(node).name.clone(), size));
    };
    resize(&mut earlier, "Steam Library/steamapps/common/Starfall/Content/Paks/Windows/worlds.pak", 15000);
    resize(&mut earlier, "Downloads/texture-library.zip", 14000);
    resize(&mut earlier, "Movies/Road Trips/Desert Roads.mkv", 17000);
    let may = find(&earlier, "Backups/Workstation/2026/May");
    earlier.remove(may);
    clawback_core::snapshot::Snapshot::capture(
        &earlier,
        clawback_core::snapshot::Detail::Files { min_size: 0 },
        1_777_000_000,
        "April",
    )
}

pub fn view(tree: &Tree) -> NodeId {
    std::env::var("CLAWBACK_DEMO_VIEW")
        .ok()
        .and_then(|path| tree.find_path(&tree.root_path().join(path)))
        .unwrap_or(ROOT)
}

/// Ignore the real pointer and keyboard so a capture never depends on where
/// its window opens relative to the cursor.
pub fn isolate_input(input: &mut eframe::egui::RawInput) {
    if capturing() {
        input.events.retain(|event| matches!(event, eframe::egui::Event::Screenshot { .. }));
    }
}

pub fn capture(ctx: &eframe::egui::Context) {
    use eframe::egui::{self, vec2};
    use std::io::Write;
    let Some(destination) = std::env::var_os("CLAWBACK_DEMO_CAPTURE") else { return };
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
    if ctx.cumulative_frame_nr() == 0 {
        let size = std::env::var("CLAWBACK_DEMO_SIZE")
            .ok()
            .and_then(|value| {
                let (width, height) = value.split_once('x')?;
                Some(vec2(width.parse().ok()?, height.parse().ok()?))
            })
            .unwrap_or(vec2(1280.0, 820.0));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    }
    let image = ctx.input(|i| {
        i.events
            .iter()
            .find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None })
    });
    if let Some(image) = image {
        let mut file = std::fs::File::create(destination).expect("screenshot destination");
        write!(file, "P6\n{} {}\n255\n", image.size[0], image.size[1]).expect("screenshot header");
        let bytes: Vec<_> = image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]).collect();
        file.write_all(&bytes).expect("screenshot pixels");
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    } else if ctx.cumulative_frame_nr() == 100 {
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
    }
}

/// The fictional drive: 384 GB with 64 GB free, so captures show a modest free-space box.
pub fn disk() -> crate::platform::DiskInfo {
    crate::platform::DiskInfo {
        name: DRIVE.into(),
        mount: DRIVE.into(),
        fs: "NTFS".into(),
        total: 384 << 30,
        free: 64 << 30,
        removable: false,
        kind: clawback_core::adaptive::StorageKind::Unknown,
    }
}

/// README screenshots use the Blackbody Radiation palette for files and folders.
pub const PALETTE: usize = 22;
