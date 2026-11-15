//! Resolve removal requests through installed applications and Steam's own metadata.
//! Discovery is read-only and runs on a worker; launching requires a separate UI action.
mod registry;
pub(in crate::app) mod steam;

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Uninstall { executable: PathBuf, arguments: String },
    Steam(u32),
}

#[derive(Clone, Debug)]
pub struct Application {
    pub name: String,
    pub publisher: String,
    pub version: String,
    pub location: PathBuf,
    pub action: Option<Action>,
}

pub struct Removal {
    pub path: PathBuf,
    pub steam: bool,
    pub applications: Vec<Application>,
}

#[derive(Clone, Default)]
struct Inventory {
    program_roots: Vec<PathBuf>,
    steam_roots: Vec<PathBuf>,
    applications: Vec<Application>,
}

/// Compare Windows paths by components, including case, separator and verbatim-prefix differences.
fn normalized(path: &Path) -> String {
    let value = path.to_string_lossy().replace('/', "\\");
    let value = if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        value.strip_prefix(r"\\?\").unwrap_or(&value).to_owned()
    };
    let mut normalized = PathBuf::new();
    for component in Path::new(&value).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component),
        }
    }
    normalized.to_string_lossy().trim_end_matches('\\').to_lowercase()
}

pub(super) fn within(path: &Path, root: &Path) -> bool {
    let (path, root) = (normalized(path), normalized(root));
    !root.is_empty() && (path == root || path.strip_prefix(&root).is_some_and(|tail| tail.starts_with('\\')))
}

fn overlaps(a: &Path, b: &Path) -> bool {
    within(a, b) || within(b, a)
}

impl Inventory {
    fn resolve(self, path: &Path) -> Option<Removal> {
        let steam = self.steam_roots.iter().any(|root| overlaps(path, &root.join("steamapps")));
        let managed = steam || self.program_roots.iter().any(|root| overlaps(path, root));
        let mut applications: Vec<_> =
            self.applications.into_iter().filter(|app| overlaps(path, &app.location)).collect();
        // Prefer a game's precise manifest over the enclosing Steam client installation.
        if applications.iter().any(|app| matches!(app.action, Some(Action::Steam(_))) && within(path, &app.location)) {
            applications.retain(|app| matches!(app.action, Some(Action::Steam(_))));
        }
        applications.sort_by_key(|a| a.name.to_lowercase());
        applications.dedup_by(|a, b| {
            a.name == b.name && a.action == b.action && normalized(&a.location) == normalized(&b.location)
        });
        (managed || !applications.is_empty()).then(|| Removal { path: path.to_owned(), steam, applications })
    }
}

/// The applications whose uninstallers may be offered for `path`, or `None` when no
/// installed software manages it. A managed path with no confirmed owner keeps `Some`
/// with no applications, so it is neither uninstalled nor deleted.
pub fn inspect(path: &Path) -> Option<Removal> {
    let mut inventory = registry::inventory();
    steam::extend(&mut inventory, path);
    let mut removal = inventory.resolve(path)?;
    // An unrecognized game must not accidentally offer to uninstall its Steam client.
    if steam::library_for(path).is_some() {
        removal.applications.retain(|app| matches!(app.action, Some(Action::Steam(_))));
    }
    retain_confirmed_owners(&mut removal, registry::program_roots(), &subfolders);
    Some(removal)
}

fn subfolders(path: &Path) -> Option<Vec<PathBuf>> {
    let mut folders = Vec::new();
    for entry in std::fs::read_dir(path).ok()? {
        let entry = entry.ok()?;
        if entry.file_type().ok()?.is_dir() {
            folders.push(entry.path());
        }
    }
    Some(folders)
}

/// Read the slower installation records in the background, ahead of the first menu.
pub fn prepare() {
    registry::prepare();
}

/// Resolve menu ownership off-thread. Never advertise an unverified uninstall action.
pub fn recognized(path: &Path) -> bool {
    if !protected(path) {
        return false;
    }
    use std::{
        collections::HashMap,
        sync::{Mutex, OnceLock},
        time::{Duration, Instant},
    };
    type Cache = Mutex<HashMap<PathBuf, (Instant, Option<bool>)>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(Mutex::default);
    let mut entries = clawback_core::scan::lock(cache);
    if let Some((at, result)) = entries.get(path)
        && (result.is_none() || at.elapsed() < Duration::from_secs(30))
    {
        return result.unwrap_or(false);
    }
    let path = path.to_owned();
    entries.retain(|_, (at, result)| result.is_none() || at.elapsed() < Duration::from_secs(30));
    entries.insert(path.clone(), (Instant::now(), None));
    std::thread::spawn(move || {
        let confirmed = inspect(&path).is_some_and(|removal| !removal.applications.is_empty());
        clawback_core::scan::lock(cache).insert(path, (Instant::now(), Some(confirmed)));
    });
    false
}

type Subfolders<'a> = &'a dyn Fn(&Path) -> Option<Vec<PathBuf>>;

/// Keep the applications that can be uninstalled and own `removal.path`: those installed
/// at or above it, or, for a vendor folder in Program Files such as `Vendor` holding
/// `Vendor\App`, every application inside it, but only when they account for every
/// folder in it. A folder that also holds something unrecognized offers nothing.
fn retain_confirmed_owners(removal: &mut Removal, roots: &[PathBuf], subfolders: Subfolders<'_>) {
    let path = removal.path.clone();
    removal.applications.retain(|app| app.action.is_some());
    if removal.applications.iter().any(|app| within(&path, &app.location)) {
        removal.applications.retain(|app| within(&path, &app.location));
        return;
    }
    removal.applications.retain(|app| within(&app.location, &path));
    let vendor = !removal.steam
        && roots.iter().any(|root| within(&path, root) && !within(root, &path))
        && covered(&path, &removal.applications, subfolders, 4);
    if !vendor {
        removal.applications.clear();
    }
}

/// Whether every folder below `folder` belongs to one of `applications`.
fn covered(folder: &Path, applications: &[Application], subfolders: Subfolders<'_>, depth: usize) -> bool {
    if applications.iter().any(|app| within(folder, &app.location)) {
        return true;
    }
    if depth == 0 || !applications.iter().any(|app| within(&app.location, folder)) {
        return false;
    }
    subfolders(folder)
        .is_some_and(|children| children.iter().all(|child| covered(child, applications, subfolders, depth - 1)))
}

pub fn protected(path: &Path) -> bool {
    recognized_with_roots(path, registry::program_roots())
}

fn recognized_with_roots(path: &Path, roots: &[PathBuf]) -> bool {
    steam::library_for(path).is_some() || roots.iter().any(|root| within(path, root))
}

pub fn launch(action: &Action) -> Result<(), String> {
    match action {
        Action::Steam(id) => crate::platform::open(Path::new(&format!("steam://uninstall/{id}"))),
        Action::Uninstall { executable, arguments } => registry::launch(executable, arguments),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, location: &str, action: Option<Action>) -> Application {
        Application {
            name: name.into(),
            location: location.into(),
            publisher: String::new(),
            version: String::new(),
            action,
        }
    }

    #[test]
    fn steam_menu_recognition_uses_complete_path_components() {
        let recognized = |path| recognized_with_roots(Path::new(path), &[]);
        assert!(recognized(r"C:\Program Files (x86)\Steam\steamapps\common\Voyage"));
        assert!(recognized(r"E:\Library\SteamApps\Common\Voyage\data.bin"));
        assert!(!recognized(r"E:\Library\steamapps\common-backup\Voyage"));
        assert!(!recognized(r"E:\Library\steamapps\common"));
    }

    type Listing<'a> = &'a [(&'a str, &'a [&'a str])];

    fn confirmed(path: &str, applications: &[Application], folders: Listing<'_>) -> Vec<String> {
        let roots = [PathBuf::from(r"C:\Program Files")];
        let listing = |path: &Path| {
            let (_, children) = folders.iter().find(|(folder, _)| Path::new(folder) == path)?;
            Some(children.iter().map(|child| path.join(child)).collect())
        };
        let mut removal = Removal { path: path.into(), steam: false, applications: applications.to_vec() };
        retain_confirmed_owners(&mut removal, &roots, &listing);
        removal.applications.into_iter().map(|app| app.name).collect()
    }

    /// No applications confirmed.
    const NONE: [String; 0] = [];

    #[test]
    fn uninstall_requires_an_action_and_an_owning_application() {
        let owner = app("App", r"C:\Program Files\Vendor\App", Some(Action::Steam(10)));
        assert_eq!(confirmed(r"C:\Program Files\WindowsApps", &[], &[]), NONE);
        assert_eq!(
            confirmed(r"C:\Program Files\Vendor\App\assets\large.pak", std::slice::from_ref(&owner), &[]),
            ["App"]
        );
        let inert = app("App", r"C:\Program Files\Vendor\App", None);
        assert_eq!(confirmed(r"C:\Program Files\Vendor\App", &[inert], &[]), NONE);
        // Program Files itself is never a vendor folder.
        let everything: Listing<'_> = &[(r"C:\Program Files", &["Vendor"]), (r"C:\Program Files\Vendor", &["App"])];
        assert_eq!(confirmed(r"C:\Program Files", &[owner], everything), NONE);
    }

    #[test]
    fn vendor_folders_offer_their_applications_only_when_they_account_for_everything() {
        let suite = app("Suite", r"C:\Program Files\Vendor\Suite", Some(Action::Steam(1)));
        let tools = app("Tools", r"C:\Program Files\Vendor\Group\Tools", Some(Action::Steam(2)));
        let apps = [suite, tools];
        let tidy: Listing<'_> =
            &[(r"C:\Program Files\Vendor", &["Suite", "Group"]), (r"C:\Program Files\Vendor\Group", &["Tools"])];
        assert_eq!(confirmed(r"C:\Program Files\Vendor", &apps, tidy), ["Suite", "Tools"]);
        let stray: Listing<'_> = &[
            (r"C:\Program Files\Vendor", &["Suite", "Group", "Unknown"]),
            (r"C:\Program Files\Vendor\Group", &["Tools"]),
        ];
        assert_eq!(confirmed(r"C:\Program Files\Vendor", &apps, stray), NONE);
        // An unreadable folder cannot be accounted for.
        assert_eq!(confirmed(r"C:\Program Files\Vendor", &apps, &[]), NONE);
        // Outside Program Files, only an application's own folder counts.
        let elsewhere = app("Suite", r"D:\Vendor\Suite", Some(Action::Steam(1)));
        assert_eq!(confirmed(r"D:\Vendor", &[elsewhere], &[(r"D:\Vendor", &["Suite"])]), NONE);
    }

    #[test]
    fn program_descendants_offer_uninstall_and_resolve_to_the_same_owner() {
        let root = PathBuf::from(r"D:\Applications");
        let owner = app(
            "Large App",
            r"D:\Applications\Vendor\Large App",
            Some(Action::Uninstall {
                executable: r"D:\Applications\Vendor\Large App\uninstall.exe".into(),
                arguments: String::new(),
            }),
        );
        for path in [
            r"D:\Applications\Vendor\Large App",
            r"d:/applications/Vendor/Large App/assets",
            r"D:\Applications\Vendor\Large App\assets\large.pak",
        ] {
            let path = Path::new(path);
            assert!(recognized_with_roots(path, std::slice::from_ref(&root)));
            let result = Inventory {
                program_roots: vec![root.clone()],
                applications: vec![owner.clone()],
                ..Default::default()
            }
            .resolve(path)
            .unwrap();
            assert_eq!(result.applications.len(), 1);
            assert_eq!(result.applications[0].location, owner.location);
            assert_eq!(result.applications[0].action, owner.action);
        }
        assert!(!recognized_with_roots(Path::new(r"D:\Applications Backup\file"), &[root]));
    }

    #[test]
    fn configured_roots_and_component_boundaries_decide_protection() {
        let inventory = || Inventory { program_roots: vec![r"D:\Applications".into()], ..Default::default() };
        assert!(inventory().resolve(Path::new(r"d:/APPLICATIONS/Vendor/app.dll")).is_some());
        assert!(inventory().resolve(Path::new(r"D:\Applications")).is_some());
        assert!(inventory().resolve(Path::new(r"D:\Applications Backup")).is_none());
        assert!(inventory().resolve(Path::new(r"D:\Other\..\Applications\App")).is_some());
        assert!(within(Path::new(r"\\?\UNC\server\share\Game"), Path::new(r"\\server\share")));
        assert!(inventory().resolve(Path::new(r"C:\Program Files\Unregistered")).is_none());
    }

    #[test]
    fn ambiguous_vendor_folders_keep_all_owners_and_game_wins_over_steam() {
        let inventory = || Inventory {
            steam_roots: vec![r"E:\Games".into()],
            applications: vec![
                app("Steam", r"E:\Games", None),
                app("Game A", r"E:\Games\steamapps\common\A", Some(Action::Steam(10))),
                app("Game B", r"E:\Games\steamapps\common\B", Some(Action::Steam(20))),
            ],
            ..Default::default()
        };
        let game = inventory().resolve(Path::new(r"E:\Games\steamapps\common\A\data.bin")).unwrap();
        assert_eq!(game.applications.len(), 1);
        assert_eq!(game.applications[0].action, Some(Action::Steam(10)));
        assert_eq!(inventory().resolve(Path::new(r"E:\Games\steamapps\common")).unwrap().applications.len(), 3);
        assert!(inventory().resolve(Path::new(r"E:\Games\steamapps\shadercache")).is_some());
    }

    /// Which folders in Program Files offer an uninstaller on this machine.
    /// Run with `cargo test installed_program_folders -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads this machine's installations"]
    fn installed_program_folders() {
        let start = std::time::Instant::now();
        let inventory = registry::inventory();
        println!("{} registrations in {:?}", inventory.applications.len(), start.elapsed());
        let start = std::time::Instant::now();
        drop(registry::inventory());
        println!("again in {:?}", start.elapsed());
        let (mut offered, mut total) = (0, 0);
        for root in registry::program_roots() {
            let Ok(entries) = std::fs::read_dir(root) else { continue };
            for entry in entries.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) {
                let path = entry.path();
                let names: Vec<_> =
                    inspect(&path).map(|r| r.applications.into_iter().map(|a| a.name).collect()).unwrap_or_default();
                total += 1;
                offered += usize::from(!names.is_empty());
                println!("{} {} {names:?}", if names.is_empty() { "  " } else { "OK" }, path.display());
            }
        }
        println!("{offered} of {total} folders offer an uninstaller");
    }
}
