//! Fictional in-memory data for reproducible marketing captures. No disk scan or watcher.
use clawback_core::{Kind, NodeId, ROOT, Tree, tree::NewEntry};
use std::path::Path;

pub fn tree() -> Tree {
    let mut tree = Tree::new(Path::new("Demo Drive"));
    let groups: &[(&str, &[(&str, u64)])] = &[
        ("Games/Starfall", &[("worlds.pak", 24000), ("textures.pak", 16000), ("audio.pak", 6000)]),
        ("Games/Neon Circuit", &[("tracks.pak", 18000), ("cars.pak", 9000), ("soundtrack.ogg", 3000)]),
        ("Movies", &[("Ocean Expedition.mkv", 22000), ("Night Sky.mkv", 14000), ("Desert Roads.mkv", 11000)]),
        (
            "Projects/Lunar Garden",
            &[
                ("Assets/Environments.blend", 12000),
                ("Assets/Characters.blend", 8000),
                ("Builds/preview.zip", 6500),
                ("Cache/shaders.bin", 3500),
            ],
        ),
        ("Projects/Studio Website", &[("design.fig", 1400), ("media.zip", 2800), ("build-cache.bin", 1900)]),
        ("Photos/Coastal Weekend", &[("Sunrise.raw", 1800), ("Harbor.raw", 1400), ("Cliffs.raw", 2100)]),
        ("Photos/Mountain Trails", &[("Alpine Lake.raw", 2400), ("Summit.raw", 1800)]),
        ("Backups", &[("workstation-may.zip", 26000), ("workstation-april.zip", 23000), ("phone-backup.zip", 10000)]),
        ("Downloads", &[("texture-library.zip", 9500), ("sample-pack.zip", 6200), ("old-installer.iso", 4800)]),
        (
            "Music",
            &[("Synthwave Collection.flac", 3200), ("Piano Sessions.flac", 1900), ("Field Recordings.wav", 4200)],
        ),
    ];
    for (folder, files) in groups {
        for (name, megabytes) in *files {
            let path = format!("{folder}/{name}");
            let parts: Vec<_> = path.split('/').collect();
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

pub fn capture(ctx: &eframe::egui::Context) {
    use eframe::egui::{self, vec2};
    use std::io::Write;
    let Some(destination) = std::env::var_os("CLAWBACK_DEMO_CAPTURE") else { return };
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
    if ctx.cumulative_frame_nr() == 0 {
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(1280.0, 820.0)));
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
