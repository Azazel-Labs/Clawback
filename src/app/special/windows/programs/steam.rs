//! Steam library and app manifests; no folder-name guessing or game-name matching.
use super::{Action, Application, Inventory};
use clawback_core::{NO_NODE, NodeId, ROOT, Tree};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

#[derive(Debug)]
enum Value {
    Text(String),
    Object(Vec<(String, Value)>),
}
impl Value {
    fn object(&self) -> Option<&[(String, Value)]> {
        if let Self::Object(items) = self { Some(items) } else { None }
    }
    fn text(&self) -> Option<&str> {
        if let Self::Text(text) = self { Some(text) } else { None }
    }
    fn get(&self, key: &str) -> Option<&Self> {
        self.object()?.iter().find(|(name, _)| name.eq_ignore_ascii_case(key)).map(|(_, v)| v)
    }
}

struct Parser<'a> {
    text: &'a str,
}
impl Parser<'_> {
    fn whitespace(&mut self) {
        loop {
            self.text = self.text.trim_start();
            if !self.text.starts_with("//") {
                break;
            }
            self.text = self.text.split_once('\n').map_or("", |(_, rest)| rest);
        }
    }
    fn token(&mut self) -> Option<String> {
        self.whitespace();
        let mut chars = self.text.chars();
        if chars.next()? != '"' {
            return None;
        }
        let mut result = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => {
                    self.text = chars.as_str();
                    return Some(result);
                }
                '\\' => match chars.next()? {
                    '\\' => result.push('\\'),
                    '"' => result.push('"'),
                    c => {
                        result.push('\\');
                        result.push(c);
                    }
                },
                c => result.push(c),
            }
        }
        None
    }
    fn object(&mut self, depth: usize) -> Option<Value> {
        if depth > 32 {
            return None;
        }
        let mut values = Vec::new();
        loop {
            self.whitespace();
            if self.text.is_empty() {
                return (depth == 0).then_some(Value::Object(values));
            }
            if let Some(rest) = self.text.strip_prefix('}') {
                if depth == 0 {
                    return None;
                }
                self.text = rest;
                return Some(Value::Object(values));
            }
            let key = self.token()?;
            self.whitespace();
            let value = if let Some(rest) = self.text.strip_prefix('{') {
                self.text = rest;
                self.object(depth + 1)?
            } else {
                Value::Text(self.token()?)
            };
            values.push((key, value));
        }
    }
}

fn read(path: &Path) -> Option<Value> {
    const LIMIT: u64 = 4 * 1024 * 1024;
    let mut text = String::new();
    std::fs::File::open(path).ok()?.take(LIMIT + 1).read_to_string(&mut text).ok()?;
    if text.len() as u64 > LIMIT {
        return None;
    }
    Parser { text: text.trim_start_matches('\u{feff}') }.object(0)
}

fn libraries(root: &Path, data: Option<&Value>) -> Vec<PathBuf> {
    let mut result = vec![root.to_owned()];
    if let Some(libraries) = data.and_then(|v| v.get("libraryfolders")).and_then(Value::object) {
        for (index, value) in libraries {
            if index.parse::<u32>().is_err() {
                continue;
            }
            // Older clients stored the path directly; newer clients use an object.
            if let Some(path) = value.text().or_else(|| value.get("path").and_then(Value::text)) {
                let path = PathBuf::from(path);
                if path.is_absolute() && !result.iter().any(|r| super::normalized(r) == super::normalized(&path)) {
                    result.push(path);
                }
            }
        }
    }
    result
}

fn game(root: &Path, id: u32, data: &Value) -> Option<Application> {
    let state = data.get("AppState")?;
    let manifest_id = state.get("appid")?.text()?.parse::<u32>().ok()?;
    if id == 0 || manifest_id != id {
        return None;
    }
    let directory = state.get("installdir")?.text()?;
    // A manifest directory is one child of steamapps/common, never an arbitrary path.
    if directory.is_empty()
        || matches!(directory, "." | "..")
        || directory.contains(['/', '\\', ':', '\0'])
        || directory.ends_with(['.', ' '])
    {
        return None;
    }
    Some(Application {
        name: state.get("name")?.text()?.to_owned(),
        publisher: String::new(),
        version: String::new(),
        location: root.join("steamapps").join("common").join(directory),
        action: Some(Action::Steam(id)),
    })
}

pub(super) fn library_for(path: &Path) -> Option<PathBuf> {
    path.ancestors().find_map(|ancestor| {
        let common = ancestor.parent()?;
        let steamapps = common.parent()?;
        (common.file_name()?.eq_ignore_ascii_case("common") && steamapps.file_name()?.eq_ignore_ascii_case("steamapps"))
            .then(|| steamapps.parent().map(Path::to_owned))
            .flatten()
    })
}

fn known_libraries() -> Vec<PathBuf> {
    super::registry::steam_path().map_or_else(Vec::new, |root| {
        let folders = read(&root.join("steamapps").join("libraryfolders.vdf"));
        libraries(&root, folders.as_ref())
    })
}

/// Games installed in `library`, from its app manifests, with when each was last played.
fn installed(library: &Path) -> impl Iterator<Item = (Application, Option<i64>)> + '_ {
    let entries = std::fs::read_dir(library.join("steamapps")).into_iter().flatten().flatten();
    entries.filter_map(move |entry| {
        let name = entry.file_name();
        let id = name
            .to_str()
            .and_then(|s| s.strip_prefix("appmanifest_"))
            .and_then(|s| s.strip_suffix(".acf"))
            .and_then(|s| s.parse().ok())?;
        let manifest = read(&entry.path())?;
        Some((game(library, id, &manifest)?, last_played(&manifest)))
    })
}

/// Seconds since the Unix epoch; Steam writes 0 for a game that was never started.
fn last_played(manifest: &Value) -> Option<i64> {
    let seconds = manifest.get("AppState")?.get("LastPlayed")?.text()?.parse::<i64>().ok()?;
    (seconds > 0).then_some(seconds)
}

pub(super) fn extend(inventory: &mut Inventory, path: &Path) {
    inventory.steam_roots = known_libraries();
    if let Some(root) = library_for(path)
        && !inventory.steam_roots.iter().any(|known| super::normalized(known) == super::normalized(&root))
    {
        inventory.steam_roots.push(root);
    }
    for library in &inventory.steam_roots {
        for (app, _) in installed(library) {
            // Prefer the current manifest to a possibly stale uninstall registration.
            inventory.applications.retain(|registered| registered.action != app.action);
            inventory.applications.push(app);
        }
    }
}

/// An installed game, drawn on the map as one tile with its name and library art.
#[derive(Clone, Debug)]
pub struct Game {
    pub id: u32,
    pub name: String,
    /// Seconds since the Unix epoch; `None` if it was never played.
    pub last_played: Option<i64>,
    location: String,
}

impl Game {
    /// A runtime or tool Steam installs for games, rather than something to play. App
    /// manifests do not say which, so the common ones are known by ID and name.
    pub fn is_tool(&self) -> bool {
        const TOOLS: [u32; 4] = [
            228_980,   // Steamworks Common Redistributables
            250_820,   // SteamVR
            1_161_040, // Proton BattlEye Runtime
            1_826_330, // Proton EasyAntiCheat Runtime
        ];
        TOOLS.contains(&self.id)
            || self.name.starts_with("Proton ")
            || self.name.starts_with("Steam Linux Runtime")
            || self.name.starts_with("Steamworks ")
    }
}

struct Catalog {
    read_at: Instant,
    refreshing: bool,
    games: Arc<[Game]>,
}

fn catalog() -> Arc<[Game]> {
    known_libraries()
        .iter()
        .flat_map(|library| installed(library).collect::<Vec<_>>())
        .filter_map(|(app, last_played)| match app.action {
            Some(Action::Steam(id)) => {
                Some(Game { id, name: app.name, last_played, location: super::normalized(&app.location) })
            }
            _ => None,
        })
        .collect()
}

/// Every library's games. The first call reads the manifests; later calls return at once and
/// refresh them in the background when they are more than 30 seconds old.
pub fn games() -> Arc<[Game]> {
    static CATALOG: OnceLock<Mutex<Catalog>> = OnceLock::new();
    let cell =
        CATALOG.get_or_init(|| Mutex::new(Catalog { read_at: Instant::now(), refreshing: false, games: catalog() }));
    let mut current = clawback_core::scan::lock(cell);
    if !current.refreshing && current.read_at.elapsed() > Duration::from_secs(30) {
        current.refreshing = true;
        std::thread::spawn(move || {
            let games = catalog();
            let mut current = clawback_core::scan::lock(cell);
            *current = Catalog { read_at: Instant::now(), refreshing: false, games };
        });
    }
    Arc::clone(&current.games)
}

/// The game installed at `node`, a folder directly inside a `steamapps/common` folder.
pub fn game_at(tree: &Tree, node: NodeId) -> Option<Game> {
    let named = |node: NodeId, name: &str| node != NO_NODE && tree.node(node).name_lossy().eq_ignore_ascii_case(name);
    let entry = tree.node(node);
    if node == ROOT || !entry.is_dir() || !named(entry.parent, "common") {
        return None;
    }
    let common = tree.node(entry.parent);
    if entry.parent == ROOT || !named(common.parent, "steamapps") {
        return None;
    }
    let path = super::normalized(&tree.path(node));
    games().iter().find(|game| game.location == path).cloned()
}

/// Steam's cached library images for a game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Art {
    /// The wide library banner (up to 3840x1240), cropped to fill large tiles.
    Hero,
    /// The store header (460x215, or 920x430 in newer caches), for wide tiles.
    Header,
    /// The 600x900 library capsule, for tall tiles.
    Capsule,
    /// The small square icon, for tiles too small for either.
    Icon,
}

/// Whether `path` is named by a 40-digit content hash, as newer Steam caches name things.
fn hashed(path: &Path) -> bool {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.len() == 40 && stem.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub fn art_path(id: u32, art: Art) -> Option<PathBuf> {
    let cache = super::registry::steam_path()?.join("appcache").join("librarycache");
    let folder = cache.join(id.to_string());
    let entries = || std::fs::read_dir(&folder).into_iter().flatten().flatten().map(|entry| entry.path());
    // Newer clients keep each image in a subfolder named by its hash, under a new name.
    let in_subfolder = |name: &str| -> Vec<PathBuf> {
        entries().filter(|path| path.is_dir() && hashed(path)).map(|dir| dir.join(name)).collect()
    };
    let candidates: Vec<PathBuf> = match art {
        Art::Hero => vec![folder.join("library_hero.jpg"), cache.join(format!("{id}_library_hero.jpg"))],
        Art::Header => std::iter::once(folder.join("header.jpg"))
            .chain(in_subfolder("library_header.jpg"))
            .chain([cache.join(format!("{id}_header.jpg"))])
            .collect(),
        Art::Capsule => std::iter::once(folder.join("library_600x900.jpg"))
            .chain(in_subfolder("library_capsule.jpg"))
            .chain([cache.join(format!("{id}_library_600x900.jpg"))])
            .collect(),
        Art::Icon => entries()
            .filter(|path| {
                path.is_file() && hashed(path) && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg"))
            })
            .chain([cache.join(format!("{id}_icon.jpg"))])
            .collect(),
    };
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(text: &str) -> Value {
        Parser { text }.object(0).unwrap()
    }
    #[test]
    fn relocated_libraries_and_manifest_ids_identify_games() {
        let data = parse(
            r#"// comment
            "libraryfolders" { "0" { "path" "C:\\Steam" "apps" { "42" "1000" } } "1" { "path" "E:\\Games" } "2" "F:\\Old Library" }
        "#,
        );
        assert_eq!(
            libraries(Path::new(r"C:\Steam"), Some(&data)),
            [PathBuf::from(r"C:\Steam"), r"E:\Games".into(), r"F:\Old Library".into()]
        );
        let manifest = parse(
            r#""AppState" { "appid" "42" "name" "A game" "installdir" "Game Folder" "LastPlayed" "1700000000" }"#,
        );
        assert_eq!(last_played(&manifest), Some(1_700_000_000));
        assert_eq!(last_played(&parse(r#""AppState" { "LastPlayed" "0" }"#)), None);
        let app = game(Path::new(r"E:\Games"), 42, &manifest).unwrap();
        assert_eq!(app.location, Path::new(r"E:\Games\steamapps\common\Game Folder"));
        assert_eq!(app.action, Some(Action::Steam(42)));
        assert!(game(Path::new(r"E:\Games"), 43, &manifest).is_none());
    }
    #[test]
    fn steam_runtimes_and_tools_are_not_games() {
        let game = |id, name: &str| Game { id, name: name.into(), last_played: None, location: String::new() };
        assert!(game(228_980, "Steamworks Common Redistributables").is_tool());
        assert!(game(2_805_730, "Proton 9.0").is_tool());
        assert!(game(1_628_350, "Steam Linux Runtime 3.0 (sniper)").is_tool());
        assert!(!game(1_794_680, "Vampire Survivors").is_tool());
    }

    #[test]
    fn malformed_manifests_cannot_choose_paths_outside_the_game_library() {
        for directory in ["..", "../Other", r"C:\Windows", "", "Game.", "Game "] {
            let data = Value::Object(vec![(
                "AppState".into(),
                Value::Object(vec![
                    ("appid".into(), Value::Text("42".into())),
                    ("name".into(), Value::Text("Game".into())),
                    ("installdir".into(), Value::Text(directory.into())),
                ]),
            )]);
            assert!(game(Path::new(r"E:\Games"), 42, &data).is_none());
        }
        assert!(Parser { text: r#""AppState" { "appid" "42""# }.object(0).is_none());
    }
}
