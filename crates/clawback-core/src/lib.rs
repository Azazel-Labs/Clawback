//! Clawback core: a parallel filesystem scanner, an arena-backed size tree, and
//! a faithful reimplementation of SpaceMonger 1.4's nested box layout.
//!
//! Everything in this crate uses only the Rust standard library.

pub mod adaptive;
pub mod format;
mod hardlinks;
pub mod icon;
pub mod layout;
pub mod live;
#[cfg(any(target_os = "macos", test))]
mod macos;
#[cfg(any(windows, test))]
mod ntfs;
pub mod palette;
#[cfg(feature = "profiling")]
pub mod profiling;
pub mod report;
pub mod scan;
pub mod settings;
pub mod tree;
#[cfg(windows)]
mod windows;

pub use scan::{Scan, ScanOptions, ScanResult, SkipReason, Skipped};
pub use settings::Settings;
pub use tree::{Kind, NO_NODE, NodeId, ROOT, Tree};
