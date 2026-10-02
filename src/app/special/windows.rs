//! Providers compiled into Windows builds, in discovery priority order.
pub(in crate::app) mod programs;
pub(in crate::app) mod recycle_bin;
pub(in crate::app) mod temp_folder;
pub(in crate::app) mod wsl_disks;

use super::{Kind, Provider};

pub(super) const PROVIDERS: &[Provider] = &[
    Provider {
        kind: Kind::RecycleBin,
        cell: Some(recycle_bin::find),
        recognizes: None,
        activate: Some(|app, _, _| app.request_empty_recycle_bin()),
        removal: None,
    },
    Provider {
        kind: Kind::TempFolder,
        cell: Some(|tree, _| temp_folder::find(tree)),
        recognizes: None,
        activate: Some(|app, _, _| app.request_empty_temp()),
        removal: None,
    },
    Provider {
        kind: Kind::WslDisk,
        cell: None,
        recognizes: Some(wsl_disks::recognized),
        activate: Some(|app, node, ctx| {
            if let Some(doc) = &app.doc {
                app.request_wsl_disk(doc.tree.path(node), ctx);
            }
        }),
        removal: None,
    },
    Provider {
        kind: Kind::InstalledSoftware,
        cell: None,
        recognizes: Some(programs::recognized),
        activate: Some(crate::app::ClawbackApp::delete),
        removal: Some(programs::inspect),
    },
];
