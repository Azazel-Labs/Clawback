//! Terminal frontend. Uses the same scan snapshots and nested layout as the GUI.

use crate::scanning::{Running, Update};
use clawback_core::{NodeId, ROOT, Settings, Tree, format, layout, palette};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const BACKGROUND: Color = Color::Rgb(15, 20, 30);
const ACCENT: Color = Color::Rgb(91, 219, 205);

pub fn run(path: PathBuf, settings: Settings) -> io::Result<()> {
    let mut app = App::new(path, settings)?;
    // Ratatui restores raw mode, the cursor and alternate screen on errors/panics.
    ratatui::run(|terminal| app.run(terminal))
}

struct App {
    root: PathBuf,
    settings: Settings,
    running: Option<Running>,
    live: Option<crate::watching::Live>,
    tree: Arc<Tree>,
    view: NodeId,
    entries: Vec<NodeId>,
    selection: ListState,
    status: String,
    started: Instant,
    map_only: bool,
}

impl App {
    fn new(root: PathBuf, settings: Settings) -> io::Result<Self> {
        let running = Some(Running::start_terminal(root.clone(), settings.scan_options())?);
        Ok(Self {
            tree: Arc::new(Tree::new(&root)),
            root,
            settings,
            running,
            live: None,
            view: ROOT,
            entries: Vec::new(),
            selection: ListState::default(),
            status: "Starting scan…".into(),
            started: Instant::now(),
            map_only: false,
        })
    }

    fn refresh_entries(&mut self) {
        self.entries.clone_from(&self.tree.node(self.view).children);
        self.entries.sort_by_key(|&id| std::cmp::Reverse(self.tree.node(id).size));
        self.selection = ListState::default().with_selected((!self.entries.is_empty()).then_some(0));
    }

    fn selected(&self) -> Option<NodeId> {
        self.selection.selected().and_then(|index| self.entries.get(index).copied())
    }

    fn updates(&mut self) -> bool {
        let mut changed = true;
        let update = self.running.as_ref().map(|running| running.rx.try_recv());
        match update {
            Some(Ok(Update::Preview(preview))) => {
                self.status = format!(
                    "{} · {} files · {} · {} not readable · up to {} workers · {}",
                    if self.running.as_ref().is_some_and(Running::is_paused) { "Scan paused" } else { "Scanning" },
                    format::count(preview.progress.files),
                    format::size(preview.progress.bytes),
                    preview.progress.denied,
                    preview.progress.workers,
                    preview.current.display()
                );
                crate::background::retire(std::mem::replace(&mut self.tree, Arc::new(preview.tree)));
                self.view = ROOT;
                self.refresh_entries();
            }
            Some(Ok(Update::Finished(result, _, _, started))) => {
                self.status = format!(
                    "{} · {} files · {} folders · {} not readable · {} skipped · {}",
                    if result.cancelled { "Partial scan" } else { "Scan complete" },
                    format::count(result.files),
                    format::count(result.dirs),
                    result.denied,
                    result.skipped.len(),
                    format::duration(result.elapsed)
                );
                self.status.push_str(" · ");
                self.status.push_str(&started.status);
                self.live = started.live;
                crate::background::retire(std::mem::replace(&mut self.tree, Arc::new(result.tree)));
                self.view = ROOT;
                self.running = None;
                self.refresh_entries();
            }
            Some(Ok(Update::Failed(error))) => {
                self.status = format!("Scan failed: {error} · Press r to retry");
                self.running = None;
            }
            Some(Ok(Update::Cancelled)) => {
                self.status = "Scan cancelled · Press r to rescan".into();
                self.running = None;
            }
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                self.status = "Scanner stopped unexpectedly · Press r to retry".into();
                self.running = None;
            }
            _ => changed = false,
        }
        let live_update = self.live.as_ref().map(|live| live.rx.try_recv());
        match live_update {
            Some(Ok(crate::watching::Update::Snapshot(snapshot))) => {
                let selected = self.selected();
                if snapshot.reset {
                    self.view = ROOT;
                } else {
                    while !snapshot.tree.is_live(self.view) {
                        self.view = self.tree.parent(self.view).unwrap_or(ROOT);
                    }
                }
                crate::background::retire(std::mem::replace(&mut self.tree, snapshot.tree));
                self.refresh_entries();
                if !snapshot.reset
                    && let Some(index) = selected.and_then(|id| self.entries.iter().position(|&n| n == id))
                {
                    self.selection.select(Some(index));
                }
                changed = true;
            }
            Some(Ok(crate::watching::Update::Status(status))) => {
                if status.starts_with("Live stopped:")
                    && let Some(live) = self.live.take()
                {
                    crate::background::retire(live);
                }
                self.status = status;
                changed = true;
            }
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                if self.status.starts_with("Live ·") {
                    self.status = "Live stopped · Press r to reconnect".into();
                }
                if let Some(live) = self.live.take() {
                    crate::background::retire(live);
                }
                changed = true;
            }
            _ => {}
        }
        changed
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let mut redraw = true;
        loop {
            redraw |= self.updates();
            if redraw || self.running.is_some() {
                terminal.draw(|frame| self.draw(frame))?;
                redraw = false;
            }
            if !event::poll(Duration::from_millis(100))? {
                continue;
            }
            let event = event::read()?;
            if matches!(event, Event::Resize(_, _)) {
                redraw = true;
            }
            if let Event::Key(key) = event {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                redraw = true;
                if key.code == KeyCode::Char('q')
                    || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    return Ok(());
                }
                if key.code == KeyCode::Esc || key.code == KeyCode::Char('p') {
                    if let Some(running) = &self.running {
                        running.toggle_pause();
                        self.status =
                            if running.is_paused() { "Scan paused · Press p to resume" } else { "Scanning…" }.into();
                    } else if key.code == KeyCode::Esc {
                        self.up();
                    }
                }
                if key.code == KeyCode::Tab {
                    self.map_only = !self.map_only;
                }
                // Preview node IDs change between snapshots; navigation starts
                // once the final (or cancelled) tree has stable IDs.
                if self.running.is_none() {
                    self.key(key.code)?;
                }
            }
        }
    }

    fn up(&mut self) {
        if let Some(parent) = self.tree.parent(self.view) {
            let child = self.view;
            self.view = parent;
            self.refresh_entries();
            self.selection.select(self.entries.iter().position(|&id| id == child));
        }
    }

    fn key(&mut self, key: KeyCode) -> io::Result<()> {
        match key {
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.entries.is_empty() {
                    let index = self.selection.selected().unwrap_or(0);
                    self.selection.select(Some((index + 1).min(self.entries.len() - 1)));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if !self.entries.is_empty() {
                    self.selection.select(Some(self.selection.selected().unwrap_or(0).saturating_sub(1)));
                }
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                if let Some(id) = self.selected().filter(|&id| self.tree.node(id).is_dir()) {
                    self.view = id;
                    self.refresh_entries();
                }
            }
            KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => self.up(),
            KeyCode::Home => {
                self.view = ROOT;
                self.refresh_entries();
            }
            KeyCode::Char('r') | KeyCode::F(5) => {
                if let Some(live) = self.live.take() {
                    crate::background::retire(live);
                }
                self.running = Some(Running::start_terminal(self.root.clone(), self.settings.scan_options())?);
                self.started = Instant::now();
                self.status = "Starting scan…".into();
            }
            _ => {}
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame<'_>) {
        let area = frame.area();
        frame.render_widget(Block::default().style(Style::default().bg(BACKGROUND).fg(Color::White)), area);
        if area.width < 30 || area.height < 10 {
            frame.render_widget(
                Paragraph::new("Clawback · enlarge terminal\nMinimum 30 × 10 · q quit").fg(ACCENT),
                area,
            );
            return;
        }
        let [header, path, body, detail, status, keys] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        let spinner = if self.running.as_ref().is_some_and(Running::is_paused) {
            "Ⅱ"
        } else if self.running.is_some() {
            ["◐", "◓", "◑", "◒"][(self.started.elapsed().as_millis() / 150 % 4) as usize]
        } else {
            "◆"
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!(" {spinner} CLAWBACK "), Style::default().fg(ACCENT).bold()),
                Span::raw(format!(
                    "  {}  ·  see where your disk space went",
                    format::size(self.tree.node(self.view).size)
                )),
            ])),
            header,
        );
        frame.render_widget(Paragraph::new(format!(" {}", self.tree.path(self.view).display())).fg(ACCENT), path);
        let (map, list) = if self.map_only || area.width < 80 {
            (body, None)
        } else {
            let [map, list] = Layout::horizontal([Constraint::Percentage(65), Constraint::Percentage(35)]).areas(body);
            (map, Some(list))
        };
        self.draw_map(frame, map);
        if let Some(list) = list {
            let rows = usize::from(list.height.saturating_sub(2));
            let start = self
                .selection
                .selected()
                .unwrap_or(0)
                .saturating_sub(rows / 2)
                .min(self.entries.len().saturating_sub(rows));
            let mut visible_selection =
                ListState::default().with_selected(self.selection.selected().map(|selected| selected - start));
            let items: Vec<_> = self
                .entries
                .iter()
                .skip(start)
                .take(rows)
                .map(|&id| {
                    let node = self.tree.node(id);
                    ListItem::new(format!(
                        "{:>9} {} {}",
                        format::size(node.size),
                        if node.is_dir() { "▸" } else { "·" },
                        node.name_lossy()
                    ))
                })
                .collect();
            frame.render_stateful_widget(
                List::new(items)
                    .block(
                        Block::bordered().title(" Largest first ").border_style(Style::default().fg(Color::DarkGray)),
                    )
                    .highlight_style(Style::default().bg(ACCENT).fg(BACKGROUND).add_modifier(Modifier::BOLD))
                    .highlight_symbol("› "),
                list,
                &mut visible_selection,
            );
        }
        let info = self.selected().map_or_else(
            || " Empty folder or no entries yet".into(),
            |id| {
                let node = self.tree.node(id);
                format!(
                    " {}\n {} · {} of this folder{}",
                    self.tree.path(id).display(),
                    format::size(node.size),
                    format::percent(node.size, self.tree.node(self.view).size),
                    if node.is_dir() { " · Enter to explore" } else { "" }
                )
            },
        );
        frame.render_widget(Paragraph::new(info), detail);
        frame.render_widget(Paragraph::new(format!(" {}", self.status)).fg(Color::Gray), status);
        let help = if self.running.is_some() {
            if self.running.as_ref().is_some_and(Running::is_paused) {
                " p/Esc resume scan   Tab map only   q quit"
            } else {
                " p/Esc pause scan   Tab map only   q quit"
            }
        } else if area.width < 80 {
            " ↑↓ select  Enter zoom  ← up  q quit"
        } else if area.width < 110 {
            " ↑↓ select  Enter zoom  ← up  Home root  r rescan  Tab map  q quit"
        } else {
            " ↑↓/jk select   Enter/→ zoom   ←/Backspace up   Home root   r rescan   Tab map only   q quit"
        };
        frame.render_widget(Paragraph::new(help).bg(ACCENT).fg(BACKGROUND), keys);
    }

    fn draw_map(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::bordered().title(" Disk map ").border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if self.tree.node(self.view).size == 0 {
            frame.render_widget(Paragraph::new(" No measured space yet").fg(Color::Gray), inner);
            return;
        }
        // A terminal cell is roughly twice as tall as it is wide. Lay out in
        // virtual pixels so the shared GUI algorithm keeps those proportions.
        let boxes = layout::build(
            &self.tree,
            self.view,
            i32::from(inner.width) * 4,
            i32::from(inner.height) * 8,
            &layout::LayoutSettings { density: 3, bias: self.settings.layout().bias },
            None,
        );
        let selected = self.selected();
        for b in boxes {
            let x = (b.x / 4).max(0) as u16;
            let y = (b.y / 8).max(0) as u16;
            let right = ((b.x + b.w + 1) / 4).max(0) as u16;
            let bottom = ((b.y + b.h + 1) / 8).max(0) as u16;
            let rect = Rect::new(inner.x + x, inner.y + y, right.saturating_sub(x), bottom.saturating_sub(y))
                .intersection(inner);
            if rect.is_empty() {
                continue;
            }
            let scheme = if b.folder { self.settings.folder_color } else { self.settings.file_color };
            let rgb = palette::display_color(scheme, b.depth, self.settings.mute_palette);
            let [r, g, blue] = rgb;
            let color = Color::Rgb(r, g, blue);
            let is_selected = b.node().is_some() && b.node() == selected;
            let style = Style::default().bg(color).fg(if palette::dark_ink(rgb) { Color::Black } else { Color::White });
            let border = if rect.height >= 3 && rect.width >= 6 { Borders::ALL } else { Borders::NONE };
            let block = Block::default()
                .borders(border)
                .style(style)
                .border_style(Style::default().fg(if is_selected { Color::White } else { BACKGROUND }));
            frame.render_widget(block, rect);
            if let Some(id) = b.node() {
                let node = self.tree.node(id);
                let label = format!("{}{}", if is_selected { "›" } else { "" }, node.name_lossy());
                // Folder titles stay on their first row; later child rectangles
                // fill the interior without painting over that title.
                let label_area = Rect::new(rect.x, rect.y, rect.width, 1);
                frame.render_widget(
                    Paragraph::new(label).style(if is_selected { style.fg(Color::White).bold() } else { style }),
                    label_area,
                );
            }
        }
    }
}

#[cfg(feature = "screenshots")]
pub fn capture_demo(path: &std::path::Path) -> io::Result<()> {
    use ratatui::{Terminal, backend::TestBackend};
    use std::io::Write;
    let tree = crate::demo::tree();
    let mut app = App {
        root: tree.root_path().to_path_buf(),
        tree: Arc::new(tree),
        settings: Settings {
            file_color: crate::demo::PALETTE,
            folder_color: crate::demo::PALETTE,
            ..Settings::default()
        },
        running: None,
        live: None,
        view: ROOT,
        entries: Vec::new(),
        selection: ListState::default(),
        status: "Demo data - fictional files and sizes".into(),
        started: Instant::now(),
        map_only: false,
    };
    app.refresh_entries();
    let mut terminal = Terminal::new(TestBackend::new(132, 38)).expect("infallible test backend");
    terminal.draw(|frame| app.draw(frame)).expect("infallible test backend");
    let mut file = std::fs::File::create(path)?;
    writeln!(file, "132 38")?;
    for cell in &terminal.backend().buffer().content {
        writeln!(file, "{}\t{:?}\t{:?}", cell.symbol(), cell.fg, cell.bg)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clawback_core::{Kind, tree::NewEntry};
    use ratatui::{Terminal, backend::TestBackend};

    fn fixture() -> App {
        let root = PathBuf::from("demo");
        let mut tree = Tree::new(&root);
        let entry = |name: &str, kind, size| NewEntry {
            name: name.into(),
            kind,
            size,
            len: size,
            mtime: 0,
            flags: 0,
            file_id: None,
        };
        tree.add_children(ROOT, vec![entry("Photos", Kind::Dir, 0), entry("archive.zip", Kind::File, 4096)]);
        tree.add_children(1, vec![entry("Vacation.jpg", Kind::File, 8192), entry("家族.png", Kind::File, 2048)]);
        let mut app = App {
            root,
            settings: Settings::default(),
            running: None,
            live: None,
            tree: Arc::new(tree),
            view: ROOT,
            entries: Vec::new(),
            selection: ListState::default(),
            status: "Scan complete".into(),
            started: Instant::now(),
            map_only: false,
        };
        app.refresh_entries();
        app
    }

    #[test]
    fn navigation_preserves_parent_selection_and_handles_empty_folders() {
        let mut app = fixture();
        assert_eq!(app.selected(), Some(1));
        app.key(KeyCode::Enter).unwrap();
        assert_eq!(app.view, 1);
        app.key(KeyCode::Down).unwrap();
        assert_eq!(app.selected(), Some(4));
        app.key(KeyCode::Enter).unwrap();
        assert_eq!(app.view, 1, "files do not change the view");
        app.key(KeyCode::Backspace).unwrap();
        assert_eq!(app.selected(), Some(1));
        app.tree = Arc::new(Tree::new(&app.root));
        app.refresh_entries();
        for key in [KeyCode::Up, KeyCode::Down, KeyCode::Enter, KeyCode::Backspace, KeyCode::Home] {
            app.key(key).unwrap();
        }
        assert_eq!(app.selected(), None);
    }

    #[test]
    fn renders_color_map_and_resizes_without_overflow() {
        let mut app = fixture();
        for (width, height) in [(120, 32), (80, 24), (40, 12), (30, 10), (8, 3), (1, 1)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let buffer = terminal.backend().buffer();
            let text: String = buffer.content.iter().map(ratatui::buffer::Cell::symbol).collect();
            if width >= 80 {
                assert!(text.contains("Largest first"));
                assert!(text.contains("archive.zip"));
                assert!(text.contains("Scan complete"));
                assert!(buffer.content.iter().any(|cell| matches!(cell.bg, Color::Rgb(r, g, b) if Color::Rgb(r, g, b) != BACKGROUND && Color::Rgb(r, g, b) != ACCENT)));
            }
        }
    }
}
