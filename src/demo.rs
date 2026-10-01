//! Fictional in-memory data for reproducible marketing captures. No disk scan or watcher.
use clawback_core::{Kind, NodeId, ROOT, Tree, tree::NewEntry};
use std::path::Path;

/// The fictional drive's name and mount point.
pub(crate) const DRIVE: &str = "Demo Drive";

/// A capture run: fictional data, no saved state, and a screenshot written on exit.
pub fn capturing() -> bool {
    std::env::var_os("CLAWBACK_DEMO_CAPTURE").is_some()
}

pub fn tree() -> Tree {
    let mut tree = Tree::new(Path::new(DRIVE));
    // Deep, realistic paths: the map colours boxes by nesting depth, so files reach seven
    // folders down and every step of a palette (Blackbody Radiation's included) appears.
    let groups: &[(&str, &[(&str, u64)])] = &[
        ("Games/Starfall/Content/Paks/Windows", &[("worlds.pak", 24000), ("textures.pak", 16000)]),
        ("Games/Starfall/Content/Audio", &[("audio.pak", 6000)]),
        ("Games/Neon Circuit/Content/Tracks/City/Night", &[("tracks.pak", 18000)]),
        ("Games/Neon Circuit/Content/Vehicles", &[("cars.pak", 9000)]),
        ("Games/Neon Circuit/Soundtrack", &[("soundtrack.ogg", 3000)]),
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
