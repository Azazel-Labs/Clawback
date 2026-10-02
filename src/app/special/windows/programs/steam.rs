//! Steam library and app manifests; no folder-name guessing or game-name matching.
use super::{Action, Application, Inventory};
use std::{
    io::Read,
    path::{Path, PathBuf},
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

pub(super) fn extend(inventory: &mut Inventory, path: &Path) {
    if let Some(root) = super::registry::steam_path() {
        let folders = read(&root.join("steamapps").join("libraryfolders.vdf"));
        inventory.steam_roots = libraries(&root, folders.as_ref());
    }
    if let Some(root) = library_for(path)
        && !inventory.steam_roots.iter().any(|known| super::normalized(known) == super::normalized(&root))
    {
        inventory.steam_roots.push(root);
    }
    for library in &inventory.steam_roots {
        let Ok(entries) = std::fs::read_dir(library.join("steamapps")) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(id) = name
                .to_str()
                .and_then(|s| s.strip_prefix("appmanifest_"))
                .and_then(|s| s.strip_suffix(".acf"))
                .and_then(|s| s.parse().ok())
            else {
                continue;
            };
            if let Some(app) = read(&entry.path()).as_ref().and_then(|v| game(library, id, v)) {
                // Prefer the current manifest to a possibly stale uninstall registration.
                inventory.applications.retain(|registered| registered.action != app.action);
                inventory.applications.push(app);
            }
        }
    }
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
        let manifest = parse(r#""AppState" { "appid" "42" "name" "A game" "installdir" "Game Folder" }"#);
        let app = game(Path::new(r"E:\Games"), 42, &manifest).unwrap();
        assert_eq!(app.location, Path::new(r"E:\Games\steamapps\common\Game Folder"));
        assert_eq!(app.action, Some(Action::Steam(42)));
        assert!(game(Path::new(r"E:\Games"), 43, &manifest).is_none());
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
