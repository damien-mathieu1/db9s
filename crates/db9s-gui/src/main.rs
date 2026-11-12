//! Desktop frontend: the same `App`, screens and keys as the TUI, drawn with GPUI.
mod state;

use db9s_core::app::{AddField, App, BrowseFocus, FieldEdit, Mode, Screen};
use db9s_core::config::Kind;
use db9s_core::datasource::Sort;
use db9s_core::input::Input;
use db9s_core::key::{KeyCode, KeyEvent};
use db9s_core::theme::{self, BorderType, Color};
use gpui::{
    div, point, prelude::*, px, rgb, size, uniform_list, Application, Bounds, ClipboardItem,
    Context, CursorStyle, Div, Entity, FocusHandle, Hsla, KeyDownEvent, Keystroke, MouseButton,
    MouseDownEvent, MouseMoveEvent, ScrollHandle, ScrollStrategy, SharedString, Task,
    UniformListScrollHandle, WeakEntity, Window, WindowBounds, WindowOptions,
};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

const BG: u32 = 0x1a1b1e;
const FG: u32 = 0xd4d4d4;
/// At zoom 1 (14px text); everything sized in px scales with the zoom.
const LINE_HEIGHT: f32 = 20.;
/// Menlo advance at text_sm (14px), to size columns from their content.
const CHAR_WIDTH: f32 = 8.5;
const ZOOMS: (f32, f32) = (0.6, 2.5);
/// Terminal editors: they need a terminal, which a window doesn't have.
const TERMINAL_EDITORS: [&str; 9] = [
    "vi", "vim", "nvim", "nano", "micro", "hx", "helix", "kak", "joe",
];
const MIN_COL_WIDTH: f32 = 40.;

fn main() {
    let read_only = std::env::args().any(|a| a == "--read-only");
    // sqlx, mongodb, lapin... need a tokio reactor; GPUI has its own executor.
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let cfg = db9s_core::config::load().unwrap_or_default();
    let app = rt.block_on(App::new(cfg, read_only));
    let saved = state::load();

    Application::new().run(move |cx: &mut gpui::App| {
        let (w, h) = saved.window.unwrap_or((1200., 780.));
        let bounds = Bounds::centered(None, size(px(w), px(h)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let handle = rt.handle().clone();
                let first = cx.new(|cx| Db9s::new(app, handle, saved, cx));
                let ws = cx.new(|_| Workspace {
                    rt,
                    tabs: vec![],
                    focus: vec![],
                    active: 0,
                    split: None,
                });
                ws.update(cx, |ws, cx| ws.add(first, window, cx));
                let weak = ws.downgrade();
                window.on_window_should_close(cx, move |_, cx| {
                    weak.update(cx, |ws, cx| ws.tabs[ws.active].read(cx).save_state())
                        .ok();
                    true
                });
                ws
            },
        )
        .unwrap();
        cx.activate(true);
    });
}

struct Db9s {
    app: App,
    rt: tokio::runtime::Handle,
    /// The tab bar this view lives in; None alone (tests).
    workspace: Option<WeakEntity<Workspace>>,
    focus: FocusHandle,
    conn_scroll: UniformListScrollHandle,
    db_scroll: UniformListScrollHandle,
    container_scroll: UniformListScrollHandle,
    row_scroll: UniformListScrollHandle,
    /// Widths the user dragged, by column name; others are sized from their content.
    col_widths: HashMap<String, f32>,
    /// Column being resized: (name, mouse x and column width when the drag started).
    resizing: Option<(String, f32, f32)>,
    /// Inspector nodes unfolded by a click, by path (everything starts folded).
    /// Kept across rows, so the same field stays open while moving through a table.
    unfolded: HashSet<String>,
    tree_scroll: UniformListScrollHandle,
    /// Horizontal scroll of the grid, moved to keep the edited cell in view.
    grid_scroll: ScrollHandle,
    font: SharedString,
    zoom: f32,
    /// Last rendered window size, saved on quit.
    window_size: (f32, f32),
    /// `E`: the GUI editor running on a row's temp file.
    editing: Option<(
        tokio::task::JoinHandle<std::io::Result<std::process::ExitStatus>>,
        db9s_core::app::EditorRequest,
    )>,
    _poll: Task<()>,
}

impl Db9s {
    fn new(
        app: App,
        rt: tokio::runtime::Handle,
        saved: state::GuiState,
        cx: &mut Context<Self>,
    ) -> Self {
        // same 100ms tick as the TUI loop, for background database calls and `E`
        let poll = cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            let alive = this.update(cx, |this, cx| {
                // browsing: keep polling so live metrics get sampled
                if this.app.is_busy() || this.app.screen == Screen::Browse {
                    this.rt.block_on(this.app.poll());
                    cx.notify();
                }
                if this.editing.as_ref().is_some_and(|(h, _)| h.is_finished()) {
                    this.finish_editing();
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
        });
        // Ctrl-C in the terminal that started it: quit like Cmd-Q, state saved
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        rt.spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                let _ = tx.send(());
            }
        });
        cx.spawn(async move |this, cx| {
            if rx.await.is_ok() {
                let _ = this.update(cx, |this, cx| this.quit(cx));
            }
        })
        .detach();
        Self {
            app,
            rt,
            focus: cx.focus_handle(),
            conn_scroll: Default::default(),
            db_scroll: Default::default(),
            container_scroll: Default::default(),
            row_scroll: Default::default(),
            workspace: None,
            col_widths: saved.col_widths,
            resizing: None,
            unfolded: HashSet::new(),
            tree_scroll: Default::default(),
            grid_scroll: ScrollHandle::new(),
            font: saved.font.unwrap_or_else(|| "Menlo".into()).into(),
            zoom: saved.zoom.unwrap_or(1.).clamp(ZOOMS.0, ZOOMS.1),
            window_size: saved.window.unwrap_or((1200., 780.)),
            editing: None,
            _poll: poll,
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        if k.modifiers.platform {
            match k.key.as_str() {
                "t" | "w" | "\\" | "[" | "]" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8"
                | "9" => {
                    let key = k.key.clone();
                    self.tab_key(&key, window, cx);
                }
                "q" => self.quit(cx),
                "c" => self.copy(k.modifiers.shift, cx),
                "=" | "+" => self.zoom = (self.zoom + 0.1).min(ZOOMS.1),
                "-" => self.zoom = (self.zoom - 0.1).max(ZOOMS.0),
                "0" => self.zoom = 1.,
                _ => return,
            }
            cx.notify();
            return;
        }
        if matches!(self.app.mode, Mode::Query) {
            match k.key.as_str() {
                "enter" if k.modifiers.shift => {
                    self.app.query_input.insert_str("\n");
                    return cx.notify();
                }
                "tab" if !k.modifiers.shift => {
                    self.complete_query();
                    return cx.notify();
                }
                _ => {}
            }
        }
        let Some(key) = to_core_key(k) else {
            return;
        };
        // returns at once: database calls run in the background, applied by the poll tick
        self.rt.block_on(self.app.handle_key(key));
        if let Some(req) = self.app.editor_request.take() {
            self.open_editor(req);
        }
        if self.app.should_quit {
            // q in one of several tabs closes that tab, in the last one quits
            let tabs = self
                .workspace
                .as_ref()
                .and_then(|w| w.upgrade())
                .map(|w| w.read(cx).tabs.len());
            if tabs.unwrap_or(1) > 1 {
                self.app.should_quit = false;
                self.tab_key("w", window, cx);
            } else {
                self.quit(cx);
            }
        }
        cx.notify();
    }

    /// Cmd-T new tab, Cmd-W close it, Cmd-1..9 / Cmd-[ ] switch, Cmd-\ split.
    fn tab_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ws) = self.workspace.as_ref().and_then(|w| w.upgrade()) else {
            return;
        };
        let me = cx.entity_id();
        let template = (key == "t" || key == "\\").then(|| self.new_tab_app());
        ws.update(cx, |ws, cx| match key {
            "t" => {
                let tab = cx.new(|cx| {
                    Db9s::new(template.unwrap(), ws.rt.handle().clone(), state::load(), cx)
                });
                ws.add(tab, window, cx);
            }
            "\\" => ws.toggle_split(template.unwrap(), window, cx),
            "w" => ws.close(me, window, cx),
            "[" => ws.activate((ws.active + ws.tabs.len() - 1) % ws.tabs.len(), window, cx),
            "]" => ws.activate((ws.active + 1) % ws.tabs.len(), window, cx),
            n => {
                let i = n.parse::<usize>().unwrap_or(1) - 1;
                if i < ws.tabs.len() {
                    ws.activate(i, window, cx);
                }
            }
        });
    }

    /// A fresh `App` for a new tab, on the connections this one already has open.
    fn new_tab_app(&self) -> App {
        let mut cfg = db9s_core::config::load().unwrap_or_default();
        cfg.connections.clear();
        let mut app = self.rt.block_on(App::new(cfg, self.app.force_read_only));
        app.conns = self
            .app
            .conns
            .iter()
            .map(|c| db9s_core::app::ConnEntry {
                name: c.name.clone(),
                kind: c.kind,
                url: c.url.clone(),
                read_only: c.read_only,
                source: c
                    .source
                    .as_ref()
                    .map(Arc::clone)
                    .map_err(|e| anyhow::anyhow!("{e}")),
            })
            .collect();
        app.mode = if app.conns.is_empty() {
            Mode::AddConnection
        } else {
            Mode::Normal
        };
        app
    }

    /// The tab label: connection, then database and container once open.
    fn title(&self) -> String {
        let a = &self.app;
        let conn = a.conns.get(a.conn_idx).map_or("db9s", |c| c.name.as_str());
        match a.screen {
            Screen::Connections => "connections".into(),
            Screen::Databases => conn.into(),
            Screen::Browse => format!(
                "{conn} › {}",
                a.containers
                    .get(a.container_idx)
                    .cloned()
                    .unwrap_or_default()
            ),
        }
    }

    /// Tab in the query editor: completes the word before the cursor from the tables
    /// and columns on screen, and SQL keywords.
    fn complete_query(&mut self) {
        let (before, _, _) = self.app.query_input.split_at_cursor();
        let mut word: Vec<char> = before
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        word.reverse();
        let word: String = word.into_iter().collect();
        let mut names: Vec<String> = self.app.containers.clone();
        if let Some(res) = &self.app.result {
            names.extend(res.columns.iter().cloned());
        }
        names.extend(SQL_KEYWORDS.iter().map(|k| k.to_string()));
        let Some(best) = db9s_core::app::complete(&names, &word).into_iter().next() else {
            return;
        };
        let rest: String = best.chars().skip(word.chars().count()).collect();
        // keywords follow the case being typed: `sel` -> `select`
        let lower = SQL_KEYWORDS.contains(&best.as_str()) && !word.chars().any(char::is_uppercase);
        self.app
            .query_input
            .insert_str(&if lower { rest.to_lowercase() } else { rest });
    }

    fn save_state(&self) {
        let state = state::GuiState {
            col_widths: self.col_widths.clone(),
            window: Some(self.window_size),
            font: Some(self.font.to_string()),
            zoom: Some(self.zoom),
        };
        // shortcut: a failed save only loses layout, not data; surface it if users ask
        let _ = state::save(&state);
    }

    fn quit(&self, cx: &mut Context<Self>) {
        self.save_state();
        cx.quit();
    }

    /// Cmd-C: the selected row as JSON; Cmd-Shift-C: the whole page as CSV.
    fn copy(&mut self, page: bool, cx: &mut Context<Self>) {
        let Some(res) = &self.app.result else {
            return;
        };
        let (text, what) = if page {
            (
                db9s_core::export::to_csv(res),
                format!("{} rows as CSV", res.rows.len()),
            )
        } else {
            let Some(row) = res.rows.get(self.app.row_idx) else {
                return;
            };
            (
                serde_json::to_string_pretty(row).unwrap_or_default(),
                "row as JSON".to_string(),
            )
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.app.status = format!("copied {what}");
    }

    /// `E` in a window: the configured editor must be a GUI one that waits, like
    /// `code --wait` or `zed --wait`; the result comes back through the poll tick.
    fn open_editor(&mut self, req: db9s_core::app::EditorRequest) {
        let mut words = self.app.editor_cmd.split_whitespace().map(String::from);
        let program = words.next().unwrap_or_default();
        let name = std::path::Path::new(&program)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if TERMINAL_EDITORS.contains(&name.as_str()) {
            let _ = std::fs::remove_file(&req.tmp_path);
            self.app.status = format!(
                "error: {name} needs a terminal; set a GUI editor in settings (s), e.g. \"code --wait\""
            );
            return;
        }
        let args: Vec<String> = words.collect();
        let path = req.tmp_path.clone();
        let handle = self.rt.spawn_blocking(move || {
            std::process::Command::new(&program)
                .args(&args)
                .arg(&path)
                .status()
        });
        self.app.status = format!("editing in {name}… (close the file to come back)");
        self.editing = Some((handle, req));
    }

    fn finish_editing(&mut self) {
        let Some((handle, req)) = self.editing.take() else {
            return;
        };
        let status = self.rt.block_on(handle);
        let content = std::fs::read_to_string(&req.tmp_path).unwrap_or_default();
        let _ = std::fs::remove_file(&req.tmp_path);
        match status {
            Ok(Ok(s)) if s.success() => self.app.finish_external_edit(&content, req.original),
            Ok(Ok(_)) => self.app.status = "editor exited with an error, edit discarded".into(),
            Ok(Err(e)) => self.app.status = format!("error: failed to launch editor: {e}"),
            Err(e) => self.app.status = format!("error: editor task failed: {e}"),
        }
    }
}

/// GPUI keystroke -> core key. Cmd combos belong to the OS, not to db9s.
fn to_core_key(k: &Keystroke) -> Option<KeyEvent> {
    let m = &k.modifiers;
    if m.platform {
        return None;
    }
    let single = |s: &str| {
        let mut c = s.chars();
        c.next().filter(|_| c.next().is_none())
    };
    let code = match k.key.as_str() {
        "enter" => KeyCode::Enter,
        "escape" => KeyCode::Esc,
        "tab" if m.shift => KeyCode::BackTab,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "space" => KeyCode::Char(' '),
        key => match k
            .key_char
            .as_deref()
            .and_then(single)
            .filter(|_| !m.control)
        {
            // key_char carries shift and layout: "E" for shift-e, ":" for shift-;
            Some(c) => KeyCode::Char(c),
            None => single(key).map_or(KeyCode::Other, KeyCode::Char),
        },
    };
    Some(KeyEvent {
        code,
        ctrl: m.control,
    })
}

/// Theme colors resolved to GPUI; `Reset` falls back to the window's default.
struct Palette {
    border_focus: Hsla,
    border_blur: Hsla,
    selected_fg: Hsla,
    selected_bg: Hsla,
    field_name: Hsla,
    modal_border: Hsla,
    error: Hsla,
    success: Hsla,
    key_hint: Hsla,
    muted: Hsla,
    rounded: bool,
}

fn palette() -> Palette {
    let t = theme::get();
    let fg = |c| hsla(c, FG);
    Palette {
        border_focus: fg(t.border_focus),
        border_blur: fg(t.border_blur),
        selected_fg: fg(t.selected_fg),
        selected_bg: hsla(t.selected_bg, BG),
        field_name: fg(t.field_name),
        modal_border: fg(t.modal_border),
        error: fg(t.error),
        success: fg(t.success),
        key_hint: fg(t.key_hint),
        muted: fg(t.muted),
        rounded: t.border_type == BorderType::Rounded,
    }
}

fn hsla(c: Color, reset: u32) -> Hsla {
    let hex = match c {
        Color::Reset => reset,
        Color::Rgb(r, g, b) => u32::from_be_bytes([0, r, g, b]),
        Color::Indexed(i) => ansi(i),
        named => ansi(match named {
            Color::Black => 0,
            Color::Red => 1,
            Color::Green => 2,
            Color::Yellow => 3,
            Color::Blue => 4,
            Color::Magenta => 5,
            Color::Cyan => 6,
            Color::Gray => 7,
            Color::DarkGray => 8,
            Color::LightRed => 9,
            Color::LightGreen => 10,
            Color::LightYellow => 11,
            Color::LightBlue => 12,
            Color::LightMagenta => 13,
            Color::LightCyan => 14,
            _ => 15,
        }),
    };
    rgb(hex).into()
}

/// xterm 256-color palette, what the TUI's indexed colors look like in most terminals.
fn ansi(i: u8) -> u32 {
    const BASE: [u32; 16] = [
        0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5, 0x7f7f7f,
        0xff0000, 0x00ff00, 0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff,
    ];
    let i = i as u32;
    match i {
        0..=15 => BASE[i as usize],
        16..=231 => {
            let level = |v: u32| if v == 0 { 0 } else { 55 + v * 40 };
            let v = i - 16;
            (level(v / 36) << 16) | (level(v / 6 % 6) << 8) | level(v % 6)
        }
        _ => {
            let g = 8 + (i - 232) * 10;
            (g << 16) | (g << 8) | g
        }
    }
}

/// A run of styled text, the GUI's equivalent of a ratatui `Span`.
#[derive(Clone)]
struct Span {
    text: SharedString,
    fg: Option<Hsla>,
    bg: Option<Hsla>,
    bold: bool,
}

fn raw(text: impl Into<SharedString>) -> Span {
    Span {
        text: text.into(),
        fg: None,
        bg: None,
        bold: false,
    }
}

fn styled(text: impl Into<SharedString>, fg: Hsla) -> Span {
    Span {
        fg: Some(fg),
        ..raw(text)
    }
}

impl Span {
    fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
}

fn line(spans: Vec<Span>) -> Div {
    div()
        .flex()
        .flex_row()
        .whitespace_nowrap()
        .overflow_hidden()
        .children(spans.into_iter().map(|s| {
            let mut d = div().child(s.text);
            if let Some(fg) = s.fg {
                d = d.text_color(fg);
            }
            if let Some(bg) = s.bg {
                d = d.bg(bg);
            }
            if s.bold {
                d = d.font_weight(gpui::FontWeight::BOLD);
            }
            d
        }))
}

/// The input text, with the cursor drawn as a reversed cell when focused.
fn input_spans(input: &Input, focused: bool) -> Vec<Span> {
    if !focused {
        return vec![raw(input.value().to_string())];
    }
    let (before, at, after) = input.split_at_cursor();
    vec![
        raw(before.to_string()),
        Span {
            fg: Some(rgb(BG).into()),
            bg: Some(rgb(FG).into()),
            ..raw(at.map_or(" ".to_string(), String::from))
        },
        raw(after.to_string()),
    ]
}

fn pane(title: impl Into<SharedString>, border: Hsla, p: &Palette) -> Div {
    let d = div()
        .flex()
        .flex_col()
        .border_1()
        .border_color(border)
        .px_2()
        .py_1()
        .gap_1()
        .overflow_hidden();
    let d = if p.rounded { d.rounded_md() } else { d };
    d.child(div().text_xs().text_color(p.muted).child(title.into()))
}

/// A virtualized list of one-line items; scrolls just enough to keep `selected` visible.
fn list(
    id: &'static str,
    items: Vec<Vec<Span>>,
    selected: usize,
    scroll: &UniformListScrollHandle,
    p: &Palette,
) -> impl IntoElement {
    if !items.is_empty() {
        scroll.scroll_to_item(selected, ScrollStrategy::Top);
    }
    let items = Rc::new(items);
    let (sel_fg, sel_bg) = (p.selected_fg, p.selected_bg);
    uniform_list(id, items.len(), move |range, _, _| {
        range
            .map(|i| {
                let l = line(items[i].clone()).px_1();
                if i == selected {
                    l.text_color(sel_fg).bg(sel_bg)
                } else {
                    l
                }
            })
            .collect()
    })
    .track_scroll(scroll.clone())
    .flex_1()
}

fn compact(v: &serde_json::Value) -> String {
    let s = match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "null".into(),
        other => other.to_string(),
    };
    if s.chars().count() > 100 {
        format!("{}...", s.chars().take(97).collect::<String>())
    } else {
        s
    }
}

/// The window: a tab bar over one tab, or two side by side (Cmd-\\).
struct Workspace {
    /// Owned here so it lives as long as the window; tabs get a handle.
    rt: tokio::runtime::Runtime,
    tabs: Vec<Entity<Db9s>>,
    /// Each tab's focus, kept here: a tab asking to switch is mid-update, so the
    /// workspace can't read it then.
    focus: Vec<FocusHandle>,
    active: usize,
    /// The tab shown right of the active one.
    split: Option<usize>,
}

impl Workspace {
    fn add(&mut self, tab: Entity<Db9s>, window: &mut Window, cx: &mut Context<Self>) {
        let me = cx.entity().downgrade();
        let focus = tab.update(cx, |t, _| {
            t.workspace = Some(me);
            t.focus.clone()
        });
        self.tabs.push(tab);
        self.focus.push(focus);
        self.activate(self.tabs.len() - 1, window, cx);
    }

    fn activate(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        // clicking the split pane swaps the two
        if self.split == Some(i) {
            self.split = Some(self.active);
        }
        self.active = i;
        window.focus(&self.focus[i]);
        cx.notify();
    }

    fn close(&mut self, tab: gpui::EntityId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = self.tabs.iter().position(|t| t.entity_id() == tab) else {
            return;
        };
        if self.tabs.len() == 1 {
            return;
        }
        self.tabs.remove(i);
        self.focus.remove(i);
        self.split = self.split.and_then(|s| match s {
            s if s == i => None,
            s if s > i => Some(s - 1),
            s => Some(s),
        });
        let next = if self.active >= i {
            self.active.saturating_sub(1)
        } else {
            self.active
        };
        self.active = next.min(self.tabs.len() - 1);
        if self.split == Some(self.active) {
            self.split = None;
        }
        self.activate(self.active, window, cx);
    }

    fn toggle_split(&mut self, template: App, window: &mut Window, cx: &mut Context<Self>) {
        if self.split.take().is_some() {
            return cx.notify();
        }
        let active = self.active;
        if self.tabs.len() == 1 {
            let tab = cx.new(|cx| Db9s::new(template, self.rt.handle().clone(), state::load(), cx));
            self.add(tab, window, cx);
        }
        // the new or next tab goes right, focus stays left
        self.split = Some(if self.active != active {
            self.active
        } else {
            (active + 1) % self.tabs.len()
        });
        self.activate(active, window, cx);
    }

    /// A click inside a tab's pane makes it the active one.
    fn focus_tab(&mut self, tab: gpui::EntityId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(i) = self.tabs.iter().position(|t| t.entity_id() == tab) {
            if i != self.active {
                self.activate(i, window, cx);
            }
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette();
        let tabs = self.tabs.iter().enumerate().map(|(i, t)| {
            let label = format!("{}  {}", i + 1, t.read(cx).title());
            let tab = div()
                .id(i)
                .px_3()
                .py_1()
                .cursor_pointer()
                .debug_selector(|| format!("tab-{i}"))
                .on_click(cx.listener(move |ws, _, window, cx| ws.activate(i, window, cx)))
                .child(label);
            if i == self.active {
                tab.bg(p.selected_bg).text_color(p.selected_fg)
            } else if Some(i) == self.split {
                tab.text_color(p.border_focus)
            } else {
                tab.text_color(p.muted)
            }
        });
        let bar = div()
            .flex()
            .flex_row()
            .bg(rgb(BG))
            .text_color(rgb(FG))
            .font_family("Menlo")
            .text_sm()
            .border_b_1()
            .border_color(p.border_blur)
            .children(tabs);
        let panes = div()
            .flex()
            .flex_row()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(self.tabs[self.active].clone()),
            )
            .children(self.split.map(|i| {
                div()
                    .flex_1()
                    .min_w_0()
                    .border_l_1()
                    .border_color(p.border_blur)
                    .child(self.tabs[i].clone())
            }));
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .children((self.tabs.len() > 1).then_some(bar))
            .child(panes)
    }
}

impl Render for Db9s {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = window.bounds().size;
        self.window_size = (f32::from(size.width), f32::from(size.height));
        // rem-based sizes (text, spacing) follow the zoom; px ones are scaled by hand
        window.set_rem_size(px(16. * self.zoom));
        let p = palette();
        let app = &self.app;
        let mut root = div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    let me = cx.entity_id();
                    if let Some(ws) = this.workspace.as_ref().and_then(|w| w.upgrade()) {
                        ws.update(cx, |ws, cx| ws.focus_tab(me, window, cx));
                    }
                }),
            )
            .on_mouse_move(cx.listener(Self::on_resize_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.resizing = None),
            )
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .bg(rgb(BG))
            .text_color(rgb(FG))
            .font_family(self.font.clone())
            .text_sm()
            .line_height(px(LINE_HEIGHT * self.zoom))
            .child(header(app, &p));
        if matches!(app.mode, Mode::Command) {
            root = root.child(command_prompt(app, &p));
            if let Some(list) = self.command_palette(&p, cx) {
                root = root.child(list);
            }
        }
        let body = match app.screen {
            Screen::Connections => connections(app, &self.conn_scroll, &p).into_any_element(),
            Screen::Databases => pane("databases", p.border_focus, &p)
                .flex_1()
                .child(list(
                    "databases",
                    app.databases.iter().map(|d| vec![raw(d.clone())]).collect(),
                    app.db_idx,
                    &self.db_scroll,
                    &p,
                ))
                .into_any_element(),
            Screen::Browse => self.browse(&p, cx).into_any_element(),
        };
        root = root.child(body);
        if matches!(app.mode, Mode::Query) {
            root = root.child(query_editor(app, &p));
        }
        root = root.child(footer(app, &p));
        if let Some(modal) = modal(app, &p) {
            root = root.child(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(modal),
            );
        }
        root
    }
}

impl Db9s {
    fn browse(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let app = &self.app;
        let containers_focused = app.browse_focus == BrowseFocus::Containers;
        let focus_color = |f: bool| if f { p.border_focus } else { p.border_blur };
        let containers = pane(
            "containers  (enter open)",
            focus_color(containers_focused),
            p,
        )
        .w_1_3()
        .child(list(
            "containers",
            app.containers
                .iter()
                .map(|c| vec![raw(c.clone())])
                .collect(),
            app.container_idx,
            &self.container_scroll,
            p,
        ));
        // the row being edited shows in the grid; the panel below lists every field,
        // including ones the grid has no column for
        let below = match &app.field_edit {
            Some(fe) => field_editor(fe, p),
            None => self.inspector(p, cx),
        };
        let right = div()
            .flex()
            .flex_col()
            .gap_1()
            .min_h_0()
            .child(self.grid(!containers_focused, p, cx).flex_1().min_h_0())
            .children((!containers_focused).then(|| below.h_2_5()))
            .children(live_charts(app, p));
        div()
            .flex()
            .flex_row()
            .flex_1()
            .gap_1()
            .min_h_0()
            .child(containers)
            .child(right.flex_1())
    }

    /// Width of a column: what the user dragged it to, else `auto_width`.
    fn col_width(&self, col: &str, rows: &[db9s_core::value::Row]) -> f32 {
        self.col_widths
            .get(col)
            .copied()
            .unwrap_or_else(|| auto_width(col, rows, CHAR_WIDTH * self.zoom))
    }
}

/// A column sized to its name and the first rows' values, capped so one long JSON
/// blob doesn't take the whole screen.
fn auto_width(col: &str, rows: &[db9s_core::value::Row], char_width: f32) -> f32 {
    let longest = rows
        .iter()
        .take(50)
        .filter_map(|r| r.get(col))
        .map(|v| compact(v).chars().count())
        .chain([col.chars().count()])
        .max()
        .unwrap_or(0);
    (longest.clamp(4, 40) as f32 * char_width + 16.).max(MIN_COL_WIDTH)
}

impl Db9s {
    /// Rows as a grid: one line per row, one resizable column per field, scrolling both
    /// ways; only the visible rows are laid out.
    fn grid(&self, focused: bool, p: &Palette, cx: &mut Context<Self>) -> Div {
        let app = &self.app;
        let title = match &app.result {
            Some(res) => format!("rows  (offset {}, showing {})", app.offset, res.rows.len()),
            None => "rows".to_string(),
        };
        let outer = pane(
            title,
            if focused {
                p.border_focus
            } else {
                p.border_blur
            },
            p,
        );
        let Some(res) = &app.result else {
            return outer.child("no data — select a container and press enter");
        };
        if res.rows.is_empty() {
            return outer.child("no rows");
        }
        let widths: Vec<f32> = res
            .columns
            .iter()
            .map(|c| self.col_width(c, &res.rows))
            .collect();
        let total: f32 = widths.iter().sum();
        self.scroll_to_edited_cell(&res.columns, &widths);
        let header = div()
            .flex()
            .flex_row()
            .w(px(total))
            .border_b_1()
            .border_color(p.border_blur)
            .children(res.columns.iter().zip(&widths).map(|(col, w)| {
                let (col_name, w) = (col.clone(), *w);
                let arrow = match &app.sort {
                    Some(s) if &s.column == col => {
                        if s.desc {
                            " ↓"
                        } else {
                            " ↑"
                        }
                    }
                    _ => "",
                };
                div()
                    .id(SharedString::from(format!("head-{col}")))
                    .relative()
                    .flex_shrink_0()
                    .w(px(w))
                    .px_1()
                    .truncate()
                    .text_color(p.field_name)
                    .font_weight(gpui::FontWeight::BOLD)
                    .cursor_pointer()
                    .debug_selector(|| format!("head-{col}"))
                    // click: ascending, again: descending, again: natural order
                    .on_click(cx.listener({
                        let col = col.clone();
                        move |this, _, _, cx| {
                            let next = match &this.app.sort {
                                Some(s) if s.column == col && s.desc => None,
                                Some(s) if s.column == col => Some(Sort {
                                    column: col.clone(),
                                    desc: true,
                                }),
                                _ => Some(Sort {
                                    column: col.clone(),
                                    desc: false,
                                }),
                            };
                            this.app.set_sort(next);
                            cx.notify();
                        }
                    }))
                    .child(format!("{col}{arrow}"))
                    .child(
                        // drag handle on the right edge
                        div()
                            .absolute()
                            .top_0()
                            .right_0()
                            .h_full()
                            .w(px(6.))
                            .cursor(CursorStyle::ResizeColumn)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                    this.resizing =
                                        Some((col_name.clone(), f32::from(ev.position.x), w));
                                    cx.stop_propagation();
                                }),
                            ),
                    )
            }));

        self.row_scroll
            .scroll_to_item(app.row_idx, ScrollStrategy::Top);
        let widths = Rc::new(widths);
        let body = uniform_list(
            "rows",
            res.rows.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                let p = palette();
                let app = &this.app;
                let Some(res) = &app.result else {
                    return vec![];
                };
                let selected = (app.browse_focus == BrowseFocus::Rows).then_some(app.row_idx);
                range
                    .filter_map(|i| Some((i, res.rows.get(i)?)))
                    .map(|(i, row)| {
                        // the selected row, while field-edited, shows its pending values
                        // (a new row isn't in the page: it only shows in the panel below)
                        let fe = app
                            .field_edit
                            .as_ref()
                            .filter(|fe| selected == Some(i) && !fe.insert);
                        let row = fe.map_or(row, |fe| &fe.edited);
                        let current = fe.and_then(|fe| fe.edited.keys().nth(fe.idx));
                        let cells = res.columns.iter().zip(widths.iter()).map(|(col, w)| {
                            let cell = div().flex_shrink_0().w(px(*w)).px_1().truncate();
                            let cell = match fe {
                                Some(fe) if fe.original.get(col) != fe.edited.get(col) => {
                                    cell.text_color(p.success)
                                }
                                _ => cell,
                            };
                            if let Some(fe) = fe.filter(|_| current == Some(col)) {
                                let content = match &fe.input {
                                    Some(input) => line(input_spans(input, true)),
                                    None => line(vec![raw(row
                                        .get(col)
                                        .map(compact)
                                        .unwrap_or_default())]),
                                };
                                // no border: uniform_list needs every row the same height
                                return cell
                                    .bg(rgb(BG))
                                    .text_color(p.border_focus)
                                    .debug_selector(|| format!("cell-edit-{col}"))
                                    .child(content);
                            }
                            match row.get(col) {
                                None | Some(serde_json::Value::Null) => {
                                    cell.text_color(p.muted).child("null")
                                }
                                Some(v) => cell.child(compact(v)),
                            }
                        });
                        let r = div()
                            .id(i)
                            .flex()
                            .flex_row()
                            .w(px(total))
                            .children(cells)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.app.row_idx = i;
                                this.app.browse_focus = BrowseFocus::Rows;
                                cx.notify();
                            }));
                        if selected == Some(i) {
                            r.text_color(p.selected_fg).bg(p.selected_bg)
                        } else {
                            r
                        }
                    })
                    .collect()
            }),
        )
        .track_scroll(self.row_scroll.clone())
        .w(px(total))
        .flex_1();

        outer.child(
            div()
                .id("grid")
                .flex_1()
                .min_h_0()
                .overflow_x_scroll()
                .track_scroll(&self.grid_scroll)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .h_full()
                        .w(px(total))
                        .child(header)
                        .child(body),
                ),
        )
    }

    /// The selected row as a foldable JSON tree; click an object/array line to (un)fold it.
    fn inspector(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let outer = pane("inspect  (click ▸ to unfold)", p.border_blur, p);
        let Some(row) = self
            .app
            .result
            .as_ref()
            .and_then(|r| r.rows.get(self.app.row_idx))
        else {
            return outer;
        };
        let lines = Rc::new(flatten(row, &self.unfolded));
        outer.child(
            uniform_list(
                "inspect",
                lines.len(),
                cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                    let p = palette();
                    range
                        .map(|i| {
                            let l = &lines[i];
                            let mut spans = vec![raw("  ".repeat(l.depth))];
                            if let Some((open, _)) = &l.fold {
                                spans.push(styled(if *open { "▾ " } else { "▸ " }, p.muted));
                            }
                            if let Some(k) = &l.key {
                                spans.push(styled(format!("{k}: "), p.field_name).bold());
                            }
                            spans.push(match &l.leaf {
                                Some(v) => leaf_span(v, &p),
                                None => styled(l.summary.clone(), p.muted),
                            });
                            let key = l.key.clone().unwrap_or_default();
                            let d = line(spans).id(i).debug_selector(|| format!("tree-{key}"));
                            match &l.fold {
                                Some((_, path)) => {
                                    let path = path.clone();
                                    d.cursor_pointer().on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            if !this.unfolded.remove(&path) {
                                                this.unfolded.insert(path.clone());
                                            }
                                            cx.notify();
                                        },
                                    ))
                                }
                                None => d,
                            }
                        })
                        .collect()
                }),
            )
            .track_scroll(self.tree_scroll.clone())
            .flex_1(),
        )
    }

    /// Every `:` match under the command bar, the current one highlighted (↑/↓ move,
    /// tab completes, enter runs what's typed, as in the TUI); a click runs that match.
    fn command_palette(&self, p: &Palette, cx: &mut Context<Self>) -> Option<Div> {
        const SHOWN: usize = 8;
        let matches = self.app.command_matches();
        if matches.is_empty() {
            return None;
        }
        let sel = self.app.command_idx % matches.len();
        let start = sel.saturating_sub(SHOWN - 1);
        let typed = self.app.command_input.value().chars().count();
        let items = matches
            .iter()
            .enumerate()
            .skip(start)
            .take(SHOWN)
            .map(|(i, m)| {
                let head: String = m.chars().take(typed).collect();
                let tail: String = m.chars().skip(typed).collect();
                let row = line(vec![raw(head).bold(), styled(tail, p.muted)])
                    .px_1()
                    .id(i)
                    .debug_selector(|| format!("palette-{m}"))
                    .cursor_pointer()
                    .on_click(cx.listener({
                        let m = m.clone();
                        move |this, _, _, cx| {
                            this.app.command_input = Input::new(m.clone());
                            this.rt
                                .block_on(this.app.handle_key(KeyEvent::new(KeyCode::Enter)));
                            cx.notify();
                        }
                    }));
                if i == sel {
                    row.text_color(p.selected_fg).bg(p.selected_bg)
                } else {
                    row
                }
            });
        let more = matches.len().saturating_sub(start + SHOWN);
        Some(
            pane(format!("{} matches", matches.len()), p.border_blur, p)
                .children(items)
                .children(
                    (more > 0).then(|| line(vec![styled(format!("… {more} more"), p.muted)])),
                ),
        )
    }

    /// While a row is field-edited, scrolls the grid sideways just enough to show the
    /// current field's column.
    fn scroll_to_edited_cell(&self, columns: &[String], widths: &[f32]) {
        let Some(fe) = self.app.field_edit.as_ref().filter(|fe| !fe.insert) else {
            return;
        };
        let Some(field) = fe.edited.keys().nth(fe.idx) else {
            return;
        };
        let Some(i) = columns.iter().position(|c| c == field) else {
            return;
        };
        let start: f32 = widths[..i].iter().sum();
        let end = start + widths[i];
        let offset = self.grid_scroll.offset();
        let view = f32::from(self.grid_scroll.bounds().size.width);
        let shown = -f32::from(offset.x);
        let target = if start < shown {
            start
        } else if view > 0. && end > shown + view {
            end - view
        } else {
            return;
        };
        self.grid_scroll.set_offset(point(px(-target), offset.y));
    }

    fn on_resize_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some((col, x0, w0)) = &self.resizing else {
            return;
        };
        if ev.pressed_button != Some(MouseButton::Left) {
            // released outside the window
            self.resizing = None;
            return;
        }
        let w = (w0 + f32::from(ev.position.x) - x0).max(MIN_COL_WIDTH);
        self.col_widths.insert(col.clone(), w);
        cx.notify();
    }
}

const SQL_KEYWORDS: [&str; 52] = [
    "SELECT",
    "FROM",
    "WHERE",
    "AND",
    "OR",
    "NOT",
    "NULL",
    "IS",
    "IN",
    "LIKE",
    "ILIKE",
    "ORDER",
    "BY",
    "GROUP",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "JOIN",
    "LEFT",
    "RIGHT",
    "INNER",
    "OUTER",
    "FULL",
    "ON",
    "AS",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "DISTINCT",
    "COUNT",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "ASC",
    "DESC",
    "WITH",
    "UNION",
    "ALL",
    "EXISTS",
    "BETWEEN",
    "CREATE",
    "TABLE",
    "DROP",
    "ALTER",
    "RETURNING",
    "EXPLAIN",
    "SHOW",
];

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tok {
    Plain,
    Keyword,
    Str,
    Num,
    /// true / false / null
    Lit,
}

/// Splits a query into colored runs: strings, numbers, true/false/null, and (for SQL)
/// keywords. Good enough to read a query at a glance, not a parser.
fn tokenize(text: &str, sql: bool) -> Vec<(String, Tok)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<(String, Tok)> = vec![];
    let mut push = |s: String, t: Tok| match out.last_mut() {
        Some((last, Tok::Plain)) if t == Tok::Plain => last.push_str(&s),
        _ => out.push((s, t)),
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        if c == '\'' || c == '"' {
            i += 1;
            while i < chars.len() && chars[i] != c {
                i += 1;
            }
            i = (i + 1).min(chars.len());
            push(chars[start..i].iter().collect(), Tok::Str);
        } else if c.is_ascii_digit() {
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') {
                i += 1;
            }
            push(chars[start..i].iter().collect(), Tok::Num);
        } else if c.is_alphabetic() || c == '_' {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let upper = word.to_uppercase();
            let tok = if ["TRUE", "FALSE", "NULL"].contains(&upper.as_str()) {
                Tok::Lit
            } else if sql && SQL_KEYWORDS.contains(&upper.as_str()) {
                Tok::Keyword
            } else {
                Tok::Plain
            };
            push(word, tok);
        } else {
            i += 1;
            push(c.to_string(), Tok::Plain);
        }
    }
    out
}

fn highlight(text: &str, sql: bool, p: &Palette) -> Vec<Span> {
    tokenize(text, sql)
        .into_iter()
        .map(|(t, tok)| match tok {
            Tok::Plain => raw(t),
            Tok::Keyword => styled(t, p.border_focus).bold(),
            Tok::Str => styled(t, p.success),
            Tok::Num => styled(t, p.key_hint),
            Tok::Lit => styled(t, p.muted),
        })
        .collect()
}

/// The `/` prompt as a multi-line, highlighted editor with the cursor drawn in.
fn query_editor(app: &App, p: &Palette) -> Div {
    let sql = !matches!(
        app.conns.get(app.conn_idx).map(|c| c.kind),
        Some(Kind::Mongo | Kind::Redis | Kind::RabbitMq)
    );
    let (before, at, after) = app.query_input.split_at_cursor();
    let (cur_line, col) = match before.rsplit_once('\n') {
        Some((head, tail)) => (head.matches('\n').count() + 1, tail),
        None => (0, before),
    };
    let text = format!(
        "{before}{}{after}",
        at.map(String::from).unwrap_or_default()
    );
    let cursor = Span {
        fg: Some(rgb(BG).into()),
        bg: Some(rgb(FG).into()),
        // at a line end the char under the cursor is the newline itself
        ..raw(at
            .filter(|c| *c != '\n')
            .map_or(" ".to_string(), String::from))
    };
    let lines = text.split('\n').enumerate().map(|(i, l)| {
        if i != cur_line {
            return line(highlight(l, sql, p));
        }
        let col_chars = col.chars().count();
        let head: String = l.chars().take(col_chars).collect();
        let skip = col_chars + usize::from(at.is_some_and(|c| c != '\n'));
        let tail: String = l.chars().skip(skip).collect();
        let mut spans = highlight(&head, sql, p);
        spans.push(cursor.clone());
        spans.extend(highlight(&tail, sql, p));
        line(spans)
    });
    let title = if app.is_queue() { "publish" } else { "query" };
    pane(
        format!(
            "{title}  (shift+enter new line, tab complete, ↑/↓ history, enter run, esc cancel)"
        ),
        p.border_focus,
        p,
    )
    .children(lines)
}

/// Sparklines of the open container's metrics (queue depth, Redis memory...), newest
/// on the right; nothing for backends without metrics.
fn live_charts(app: &App, p: &Palette) -> Option<Div> {
    if app.metrics.is_empty() {
        return None;
    }
    const HEIGHT: f32 = 36.;
    let charts = app.metrics.iter().map(|(name, series)| {
        let max = series.iter().cloned().fold(0., f64::max);
        let last = series.back().copied().unwrap_or(0.);
        let bars = series.iter().map(|v| {
            let h = if max > 0. {
                (v / max) as f32 * HEIGHT
            } else {
                0.
            };
            div().w(px(3.)).h(px(h.max(1.))).bg(p.border_focus)
        });
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(line(vec![
                styled(format!("{name} "), p.muted),
                styled(format_metric(last), p.key_hint).bold(),
            ]))
            .child(
                div()
                    .h(px(HEIGHT))
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(px(1.))
                    .children(bars),
            )
    });
    Some(
        pane(
            format!("live  (every {}s)", db9s_core::app::METRICS_EVERY.as_secs()),
            p.border_blur,
            p,
        )
        .child(div().flex().flex_row().gap_6().children(charts)),
    )
}

/// 3, 2.5, 1234 — no trailing zeros for counts, two decimals for rates and sizes.
fn format_metric(v: f64) -> String {
    if v.fract() == 0. {
        format!("{v:.0}")
    } else {
        format!("{v:.2}")
    }
}

/// One line of the inspector tree.
#[derive(Debug, PartialEq)]
struct TreeLine {
    depth: usize,
    /// Field name or array index.
    key: Option<String>,
    /// Scalars only.
    leaf: Option<serde_json::Value>,
    /// Objects/arrays: whether shown unfolded, and the path that toggles it.
    fold: Option<(bool, String)>,
    /// What a folded or empty container shows, e.g. `{3 fields}` / `[]`.
    summary: String,
}

/// Flattens a row into visible tree lines; objects/arrays show their content only when
/// their path (`/key/0/key`) is in `unfolded`.
fn flatten(row: &db9s_core::value::Row, unfolded: &HashSet<String>) -> Vec<TreeLine> {
    fn walk(
        key: String,
        v: &serde_json::Value,
        path: String,
        depth: usize,
        unfolded: &HashSet<String>,
        out: &mut Vec<TreeLine>,
    ) {
        use serde_json::Value;
        let children: Vec<(String, &Value)> = match v {
            Value::Object(m) => m.iter().map(|(k, v)| (k.clone(), v)).collect(),
            Value::Array(a) => a
                .iter()
                .enumerate()
                .map(|(i, v)| (i.to_string(), v))
                .collect(),
            _ => {
                out.push(TreeLine {
                    depth,
                    key: Some(key),
                    leaf: Some(v.clone()),
                    fold: None,
                    summary: String::new(),
                });
                return;
            }
        };
        let (open_c, close_c, unit) = if v.is_object() {
            ('{', '}', "field")
        } else {
            ('[', ']', "item")
        };
        let plural = if children.len() == 1 { "" } else { "s" };
        let open = !children.is_empty() && unfolded.contains(&path);
        let summary = if children.is_empty() {
            format!("{open_c}{close_c}")
        } else if open {
            open_c.to_string()
        } else {
            format!("{open_c}{} {unit}{plural}{close_c}", children.len())
        };
        out.push(TreeLine {
            depth,
            key: Some(key),
            leaf: None,
            fold: (!children.is_empty()).then(|| (open, path.clone())),
            summary,
        });
        if open {
            for (k, child) in children {
                let child_path = format!("{path}/{k}");
                walk(k, child, child_path, depth + 1, unfolded, out);
            }
        }
    }
    let mut out = vec![];
    for (k, v) in row {
        walk(k.clone(), v, format!("/{k}"), 0, unfolded, &mut out);
    }
    out
}

fn leaf_span(v: &serde_json::Value, p: &Palette) -> Span {
    use serde_json::Value;
    match v {
        Value::String(s) => styled(format!("{s:?}"), p.success),
        Value::Number(n) => styled(n.to_string(), p.key_hint),
        other => styled(other.to_string(), p.muted),
    }
}

fn header(app: &App, p: &Palette) -> Div {
    let conn = app
        .conns
        .get(app.conn_idx)
        .map(|c| c.name.clone())
        .unwrap_or_default();
    let path = match app.screen {
        Screen::Connections => "connections".to_string(),
        Screen::Databases => format!("{conn} >"),
        Screen::Browse => format!(
            "{conn} > {} > {}",
            app.databases.get(app.db_idx).cloned().unwrap_or_default(),
            app.containers
                .get(app.container_idx)
                .cloned()
                .unwrap_or_default()
        ),
    };
    let mut spans = vec![styled(format!("db9s :: {path} "), p.border_focus).bold()];
    if app.is_read_only() {
        spans.push(styled("READ-ONLY", p.error).bold());
    }
    let d = div()
        .border_1()
        .border_color(p.border_blur)
        .px_2()
        .py_1()
        .child(line(spans));
    if p.rounded {
        d.rounded_md()
    } else {
        d
    }
}

/// k9s-style command bar: `> typed` followed by the greyed rest of the suggestion.
fn command_prompt(app: &App, p: &Palette) -> Div {
    let mut spans = vec![styled("> ", p.border_focus).bold()];
    match app.command_suggestion().filter(|s| !s.is_empty()) {
        Some(rest) => {
            let mut chars = rest.chars();
            let first = chars.next().unwrap_or(' ').to_string();
            spans.push(raw(app.command_input.value().to_string()));
            spans.push(Span {
                fg: Some(rgb(BG).into()),
                bg: Some(p.muted),
                ..raw(first)
            });
            spans.push(styled(chars.as_str().to_string(), p.muted));
        }
        None => spans.extend(input_spans(&app.command_input, true)),
    }
    pane("command", p.border_focus, p).child(line(spans))
}

fn connections(app: &App, scroll: &UniformListScrollHandle, p: &Palette) -> Div {
    let items = app
        .conns
        .iter()
        .map(|c| {
            let mut spans = vec![
                raw(format!("{:<20}", c.name)).bold(),
                styled(format!("[{}]  ", c.kind.label()), p.field_name),
            ];
            if c.read_only || app.force_read_only {
                spans.push(styled("READ-ONLY ", p.error).bold());
            }
            spans.push(styled(db9s_core::app::mask_password(&c.url), p.muted));
            if let Err(e) = &c.source {
                spans.push(styled(format!("  (error: {e})"), p.error));
            }
            spans
        })
        .collect();
    pane(
        "connections  (a add, e edit, d delete, s settings)",
        p.border_focus,
        p,
    )
    .flex_1()
    .child(list("connections", items, app.conn_idx, scroll, p))
}

/// Inline row editor: one `field: value` line per field, changed fields marked with `●`.
fn field_editor(fe: &FieldEdit, p: &Palette) -> Div {
    let lines = fe.edited.iter().enumerate().map(|(i, (k, v))| {
        let changed = fe.original.get(k) != Some(v);
        let mut spans = vec![
            styled(if changed { "● " } else { "  " }, p.success),
            styled(format!("{k}: "), p.field_name).bold(),
        ];
        match (&fe.input, i == fe.idx) {
            (Some(input), true) => spans.extend(input_spans(input, true)),
            _ => spans.push(raw(compact(v))),
        }
        let l = line(spans).px_1();
        if i == fe.idx {
            l.text_color(p.selected_fg).bg(p.selected_bg)
        } else {
            l
        }
    });
    pane(
        if fe.insert {
            "new row  (null = database default)"
        } else {
            "edit row"
        },
        p.border_focus,
        p,
    )
    .children(lines)
}

fn footer(app: &App, p: &Palette) -> Div {
    if matches!(app.mode, Mode::Query) {
        return line(help_spans(
            "enter run  shift+enter new_line  tab complete  ↑/↓ history  esc cancel",
            p,
        ));
    }
    if matches!(
        app.mode,
        Mode::ConfirmDelete | Mode::ConfirmDeleteConn | Mode::ConfirmEditSave
    ) {
        return line(vec![styled("y/n", p.error).bold()]);
    }
    let mut spans = help_spans(app.key_hints(), p);
    if !app.status.is_empty() {
        let is_err = app.status.starts_with("error") || app.status.starts_with("failed");
        let status = styled(app.status.clone(), if is_err { p.error } else { p.muted });
        // errors go first so a long key list never truncates them
        if is_err {
            spans.splice(0..0, [status, raw("  │ ")]);
        } else {
            spans.extend([raw("  │ "), status]);
        }
    }
    line(spans)
}

/// "key action  key action" -> bold keys, muted descriptions.
fn help_spans(hints: &str, p: &Palette) -> Vec<Span> {
    let mut spans = vec![];
    for (i, chunk) in hints.split("  ").enumerate() {
        if i > 0 {
            spans.push(raw("  "));
        }
        match chunk.split_once(' ') {
            Some((key, desc)) => {
                spans.push(styled(key.to_string(), p.key_hint).bold());
                spans.push(raw(" "));
                spans.push(styled(desc.to_string(), p.muted));
            }
            None => spans.push(styled(chunk.to_string(), p.muted)),
        }
    }
    spans
}

fn modal_box(title: impl Into<SharedString>, p: &Palette) -> Div {
    div()
        .w(px(640.))
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .bg(rgb(BG))
        .border_1()
        .border_color(p.modal_border)
        .rounded_md()
        .shadow_lg()
        .child(div().text_color(p.modal_border).child(title.into()))
}

/// The statement the open confirm would run, wrapped, once the backend says.
fn preview(app: &App, p: &Palette) -> Option<Div> {
    let sql = app.confirm_preview.clone()?;
    Some(div().text_color(p.muted).child(sql))
}

/// A labelled form field, highlighted when it has focus.
fn field(label: &str, value: Vec<Span>, focused: bool, p: &Palette) -> Div {
    let d = div()
        .flex()
        .flex_col()
        .border_1()
        .border_color(p.border_blur)
        .px_2()
        .py_1()
        .child(div().text_xs().text_color(p.muted).child(label.to_string()))
        .child(line(value));
    if focused {
        d.text_color(p.selected_fg).bg(p.selected_bg)
    } else {
        d
    }
}

fn modal(app: &App, p: &Palette) -> Option<Div> {
    Some(match app.mode {
        Mode::ConfirmDelete => modal_box("confirm", p)
            .child(line(vec![styled(
                if app.is_queue() {
                    "purge every message in this queue?"
                } else {
                    "delete this row/document?"
                },
                p.success,
            )
            .bold()]))
            .children(preview(app, p)),
        Mode::ConfirmDeleteConn => {
            let name = app.conns.get(app.conn_idx).map_or("", |c| c.name.as_str());
            modal_box("confirm", p).child(line(vec![styled(
                format!("delete connection '{name}'?"),
                p.success,
            )
            .bold()]))
        }
        Mode::ConfirmEditSave => {
            let (original, edited) = app.pending_edit.as_ref()?;
            let mut keys: Vec<&String> = original.keys().chain(edited.keys()).collect();
            keys.sort();
            keys.dedup();
            let diffs = keys.into_iter().filter_map(|k| {
                let (old, new) = (original.get(k), edited.get(k));
                (old != new).then(|| {
                    line(vec![
                        styled(format!("{k}: "), p.field_name).bold(),
                        styled(old.map_or("<absent>".into(), compact), p.error),
                        raw(" -> "),
                        styled(new.map_or("<removed>".into(), compact), p.success),
                    ])
                })
            });
            modal_box("confirm edit", p)
                .child(line(vec![
                    styled("save these changes? y/n", p.success).bold()
                ]))
                .children(diffs)
                .children(preview(app, p))
        }
        Mode::Settings => modal_box("settings (tab field, ←/→ theme, enter save, esc cancel)", p)
            .child(field(
                "editor (for row/doc edit)",
                input_spans(&app.settings_editor_input, !app.settings_on_theme),
                !app.settings_on_theme,
                p,
            ))
            .child(field(
                "theme (live preview)",
                vec![raw(format!("< {} >", app.settings_theme))],
                app.settings_on_theme,
                p,
            ))
            .child(match &app.settings_error {
                Some(err) => line(vec![styled(err.clone(), p.error)]),
                None => line(vec![styled(
                    "saved to config.toml; custom themes go in <config dir>/themes/<name>.toml",
                    p.muted,
                )]),
            }),
        Mode::AddConnection => {
            let base = if app.edit_target.is_some() {
                "edit connection"
            } else {
                "add connection"
            };
            let f = app.new_conn_field;
            let msg = match &app.new_conn_error {
                Some(msg) => {
                    let color = if msg.starts_with("connected") || msg == "connection ok" {
                        p.success
                    } else if msg == "connecting..." {
                        p.selected_fg
                    } else {
                        p.error
                    };
                    styled(msg.clone(), color)
                }
                None => styled(
                    "tip: ${PG_PASSWORD} or \"cmd: op read op://...\" keeps passwords out of config.toml",
                    p.muted,
                ),
            };
            modal_box(
                format!("{base} (tab move, enter next/submit, ctrl+t test, esc cancel)"),
                p,
            )
            .child(field(
                "name",
                input_spans(&app.new_conn_name, f == AddField::Name),
                f == AddField::Name,
                p,
            ))
            .child(field(
                "url",
                input_spans(&app.new_conn_url, f == AddField::Url),
                f == AddField::Url,
                p,
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        field(
                            "kind (left/right)",
                            vec![raw(format!("< {} >", app.new_conn_kind.label()))],
                            f == AddField::Kind,
                            p,
                        )
                        .flex_1(),
                    )
                    .child(
                        field(
                            "read-only (space)",
                            vec![raw(if app.new_conn_read_only {
                                "< on >"
                            } else {
                                "< off >"
                            })],
                            f == AddField::ReadOnly,
                            p,
                        )
                        .flex_1(),
                    ),
            )
            .child(line(vec![msg]))
        }
        Mode::Schema => {
            let schema = app.schema.as_ref()?;
            let container = app
                .containers
                .get(app.container_idx)
                .cloned()
                .unwrap_or_default();
            let row = |cells: Vec<Span>| {
                div().flex().flex_row().children(cells.into_iter().map(|c| {
                    div()
                        .w(px(150.))
                        .flex_shrink_0()
                        .truncate()
                        .child(line(vec![c]))
                }))
            };
            modal_box(format!("schema: {container}  (esc close)"), p)
                .w(px(820.))
                .child(row(schema
                    .columns
                    .iter()
                    .map(|c| styled(c.clone(), p.field_name).bold())
                    .collect()))
                .children(schema.rows.iter().map(|r| {
                    row(schema
                        .columns
                        .iter()
                        .map(|c| raw(r.get(c).map(compact).unwrap_or_default()))
                        .collect())
                }))
        }
        Mode::Normal | Mode::Query | Mode::FieldEdit | Mode::Command => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ks(key: &str, key_char: Option<&str>, f: impl FnOnce(&mut gpui::Modifiers)) -> Keystroke {
        let mut k = Keystroke {
            key: key.into(),
            key_char: key_char.map(String::from),
            ..Default::default()
        };
        f(&mut k.modifiers);
        k
    }

    #[test]
    fn maps_keys_like_the_tui() {
        let code = |k: Keystroke| to_core_key(&k).map(|e| (e.code, e.ctrl));
        assert_eq!(
            code(ks("e", Some("E"), |m| m.shift = true)),
            Some((KeyCode::Char('E'), false))
        );
        assert_eq!(
            code(ks(";", Some(":"), |m| m.shift = true)),
            Some((KeyCode::Char(':'), false))
        );
        assert_eq!(
            code(ks("t", None, |m| m.control = true)),
            Some((KeyCode::Char('t'), true))
        );
        assert_eq!(
            code(ks("tab", None, |m| m.shift = true)),
            Some((KeyCode::BackTab, false))
        );
        assert_eq!(
            code(ks("space", Some(" "), |_| ())),
            Some((KeyCode::Char(' '), false))
        );
        assert_eq!(
            code(ks("escape", None, |_| ())),
            Some((KeyCode::Esc, false))
        );
        assert_eq!(code(ks("f1", None, |_| ())), Some((KeyCode::Other, false)));
        assert_eq!(code(ks("q", Some("q"), |m| m.platform = true)), None);
    }

    #[test]
    fn columns_fit_their_content_within_bounds() {
        let rows: Vec<db9s_core::value::Row> = vec![
            serde_json::from_str(r#"{"id": 1, "bio": "x"}"#).unwrap(),
            serde_json::from_str(&format!(r#"{{"id": 22, "bio": "{}"}}"#, "y".repeat(500)))
                .unwrap(),
        ];
        assert_eq!(auto_width("id", &rows, CHAR_WIDTH), 4. * CHAR_WIDTH + 16.);
        assert_eq!(auto_width("bio", &rows, CHAR_WIDTH), 40. * CHAR_WIDTH + 16.);
        assert_eq!(
            auto_width("a_long_column_name", &[], CHAR_WIDTH),
            18. * CHAR_WIDTH + 16.
        );
    }

    #[test]
    fn tree_starts_folded_and_unfolds_by_path() {
        let row: db9s_core::value::Row = serde_json::from_str(
            r#"{"id": 7, "tags": [], "addr": {"city": "Lyon", "geo": {"lat": 45.7}}}"#,
        )
        .unwrap();
        let show = |unfolded: &[&str]| -> Vec<String> {
            let u = unfolded.iter().map(|s| s.to_string()).collect();
            flatten(&row, &u)
                .into_iter()
                .map(|l| {
                    let v = l.leaf.map(|v| v.to_string()).unwrap_or(l.summary);
                    format!("{}{}={v}", " ".repeat(l.depth), l.key.unwrap_or_default())
                })
                .collect()
        };
        assert_eq!(show(&[]), ["id=7", "tags=[]", "addr={2 fields}"]);
        assert_eq!(
            show(&["/addr"]),
            [
                "id=7",
                "tags=[]",
                "addr={",
                " city=\"Lyon\"",
                " geo={1 field}"
            ]
        );
        // a child stays unfolded under a folded parent, and shows again with it
        assert_eq!(show(&["/addr/geo"]), ["id=7", "tags=[]", "addr={2 fields}"]);
        assert_eq!(show(&["/addr", "/addr/geo"]).last().unwrap(), "  lat=45.7");
    }

    /// Clicking a folded line in the rendered inspector unfolds it.
    #[gpui::test]
    fn clicking_a_tree_line_unfolds_it(cx: &mut gpui::TestAppContext) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut app = rt.block_on(App::new(Default::default(), false));
        app.screen = Screen::Browse;
        app.browse_focus = BrowseFocus::Rows;
        app.mode = Mode::Normal;
        app.result = Some(db9s_core::datasource::QueryResult {
            columns: vec!["name".into(), "tags".into()],
            rows: vec![serde_json::from_str(r#"{"name": "Mouse", "tags": ["a", "b"]}"#).unwrap()],
            affected: None,
        });
        let (view, cx) =
            cx.add_window_view(|_, cx| Db9s::new(app, rt.handle().clone(), Default::default(), cx));
        cx.run_until_parked();
        let bounds = cx.debug_bounds("tree-tags").expect("tags line is rendered");
        cx.simulate_click(bounds.center(), Default::default());
        cx.run_until_parked();
        view.read_with(cx, |v, _| {
            assert!(v.unfolded.contains("/tags"), "click reached the line")
        });
        assert!(
            cx.debug_bounds("tree-0").is_some(),
            "unfolded items are rendered"
        );
    }

    /// Typing in `:` lists every match; ↓ moves the highlight; clicking one runs it.
    /// (clicks `settings`, which only opens a popup: `theme ..` would write config.toml)
    #[gpui::test]
    fn command_palette_lists_and_runs_matches(cx: &mut gpui::TestAppContext) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut app = rt.block_on(App::new(Default::default(), false));
        app.mode = Mode::Normal; // no connections opens the add-connection form
        let (view, cx) =
            cx.add_window_view(|_, cx| Db9s::new(app, rt.handle().clone(), Default::default(), cx));
        let typing = |text: &str| {
            let keys: Vec<KeyEvent> = text
                .chars()
                .map(|c| KeyEvent::new(KeyCode::Char(c)))
                .collect();
            move |v: &mut Db9s, cx: &mut Context<Db9s>| {
                for k in &keys {
                    v.rt.block_on(v.app.handle_key(*k));
                }
                cx.notify();
            }
        };
        view.update(cx, typing(":th"));
        cx.run_until_parked();
        assert!(cx.debug_bounds("palette-theme default").is_some());
        assert!(cx.debug_bounds("palette-theme nord").is_some());
        view.update(cx, |v, _| {
            v.rt.block_on(v.app.handle_key(KeyEvent::new(KeyCode::Down)));
            assert_eq!(v.app.command_idx, 1);
            v.rt.block_on(v.app.handle_key(KeyEvent::new(KeyCode::Esc)));
        });
        view.update(cx, typing(":se"));
        cx.run_until_parked();
        let settings = cx
            .debug_bounds("palette-settings")
            .expect("settings is listed");
        cx.simulate_click(settings.center(), Default::default());
        cx.run_until_parked();
        view.read_with(cx, |v, _| assert!(matches!(v.app.mode, Mode::Settings)));
    }

    /// `e` then enter edits the selected row's field right in the grid.
    #[gpui::test]
    fn field_edit_happens_in_the_grid(cx: &mut gpui::TestAppContext) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut app = rt.block_on(App::new(Default::default(), false));
        app.screen = Screen::Browse;
        app.browse_focus = BrowseFocus::Rows;
        app.mode = Mode::Normal;
        app.result = Some(db9s_core::datasource::QueryResult {
            columns: vec!["id".into(), "name".into()],
            rows: vec![serde_json::from_str(r#"{"id": 1, "name": "Mouse"}"#).unwrap()],
            affected: None,
        });
        let (view, cx) =
            cx.add_window_view(|_, cx| Db9s::new(app, rt.handle().clone(), Default::default(), cx));
        view.update(cx, |v, cx| {
            for k in [
                KeyCode::Char('e'),
                KeyCode::Char('j'),
                KeyCode::Enter,
                KeyCode::Char('X'),
            ] {
                v.rt.block_on(v.app.handle_key(KeyEvent::new(k)));
            }
            cx.notify();
        });
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("cell-edit-name").is_some(),
            "name cell is in edit"
        );
        assert!(cx.debug_bounds("cell-edit-id").is_none());
        view.update(cx, |v, _| {
            v.rt.block_on(v.app.handle_key(KeyEvent::new(KeyCode::Enter)));
            let fe = v.app.field_edit.as_ref().unwrap();
            assert_eq!(fe.edited["name"], "MouseX");
        });
    }

    /// Clicking a grid header asks the core to sort by that column (the sorting itself
    /// is tested in the core; here, no connection: the core refuses, proving the click).
    #[gpui::test]
    fn clicking_a_header_sorts(cx: &mut gpui::TestAppContext) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut app = rt.block_on(App::new(Default::default(), false));
        app.screen = Screen::Browse;
        app.browse_focus = BrowseFocus::Rows;
        app.mode = Mode::Normal;
        app.result = Some(db9s_core::datasource::QueryResult {
            columns: vec!["id".into()],
            rows: vec![serde_json::from_str(r#"{"id": 1}"#).unwrap()],
            affected: None,
        });
        let (view, cx) =
            cx.add_window_view(|_, cx| Db9s::new(app, rt.handle().clone(), Default::default(), cx));
        cx.run_until_parked();
        let head = cx.debug_bounds("head-id").expect("header is rendered");
        cx.simulate_click(head.center(), Default::default());
        cx.run_until_parked();
        view.read_with(cx, |v, _| {
            assert_eq!(v.app.status, "error: this backend can't sort")
        });
    }

    /// Cmd-C copies the selected row, Cmd-Shift-C the page; Cmd +/-/0 zoom; `E` with
    /// a terminal editor is refused instead of hanging on a terminal that isn't there.
    #[gpui::test]
    fn cmd_keys_copy_and_zoom(cx: &mut gpui::TestAppContext) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut app = rt.block_on(App::new(Default::default(), false));
        app.screen = Screen::Browse;
        app.browse_focus = BrowseFocus::Rows;
        app.mode = Mode::Normal;
        app.editor_cmd = "nvim".into();
        app.result = Some(db9s_core::datasource::QueryResult {
            columns: vec!["id".into(), "name".into()],
            rows: vec![serde_json::from_str(r#"{"id": 1, "name": "a,b"}"#).unwrap()],
            affected: None,
        });
        let (view, cx) =
            cx.add_window_view(|_, cx| Db9s::new(app, rt.handle().clone(), Default::default(), cx));
        cx.update(|window, cx| window.focus(&view.read(cx).focus));
        cx.run_until_parked();
        cx.simulate_keystrokes("cmd-c");
        let copied = cx.read_from_clipboard().and_then(|c| c.text()).unwrap();
        assert!(copied.contains("\"name\": \"a,b\""), "{copied}");
        cx.simulate_keystrokes("cmd-shift-c");
        let csv = cx.read_from_clipboard().and_then(|c| c.text()).unwrap();
        assert_eq!(csv, "id,name\r\n1,\"a,b\"\r\n");
        cx.simulate_keystrokes("cmd-= cmd-=");
        view.read_with(cx, |v, _| assert!((v.zoom - 1.2).abs() < 1e-4));
        cx.simulate_keystrokes("cmd-0");
        view.read_with(cx, |v, _| assert_eq!(v.zoom, 1.));
        cx.simulate_keystrokes("shift-e");
        view.read_with(cx, |v, _| {
            assert!(
                v.app.status.contains("nvim needs a terminal"),
                "{}",
                v.app.status
            );
            assert!(v.editing.is_none());
        });
    }

    /// Cmd-T opens a tab on the same connections, Cmd-1 switches, Cmd-\\ splits,
    /// Cmd-W closes.
    #[gpui::test]
    fn tabs_open_switch_split_and_close(cx: &mut gpui::TestAppContext) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut app = rt.block_on(App::new(Default::default(), false));
        app.mode = Mode::Normal;
        let (ws, cx) = cx.add_window_view(|window, cx| {
            let first = cx.new(|cx| Db9s::new(app, rt.handle().clone(), Default::default(), cx));
            let weak = cx.weak_entity();
            let focus = first.update(cx, |t, _| {
                t.workspace = Some(weak);
                t.focus.clone()
            });
            window.focus(&focus);
            Workspace {
                rt,
                tabs: vec![first],
                focus: vec![focus],
                active: 0,
                split: None,
            }
        });
        let state = |cx: &mut gpui::VisualTestContext| {
            ws.read_with(cx, |w, _| (w.tabs.len(), w.active, w.split))
        };
        cx.simulate_keystrokes("cmd-t");
        assert_eq!(state(cx), (2, 1, None));
        cx.simulate_keystrokes("cmd-1");
        assert_eq!(state(cx), (2, 0, None));
        cx.simulate_keystrokes("cmd-\\");
        assert_eq!(state(cx), (2, 0, Some(1)));
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("tab-1").is_some(),
            "tab bar shows with 2 tabs"
        );
        cx.simulate_keystrokes("cmd-w");
        assert_eq!(state(cx), (1, 0, None));
    }

    #[test]
    fn tokenizes_queries() {
        let marked: Vec<(String, Tok)> = tokenize(
            "select * from t where n = 'it''s' and x > 4.5 or y is null",
            true,
        )
        .into_iter()
        .filter(|(_, t)| *t != Tok::Plain)
        .collect();
        let expect: Vec<(String, Tok)> = [
            ("select", Tok::Keyword),
            ("from", Tok::Keyword),
            ("where", Tok::Keyword),
            ("'it'", Tok::Str),
            ("'s'", Tok::Str),
            ("and", Tok::Keyword),
            ("4.5", Tok::Num),
            ("or", Tok::Keyword),
            ("is", Tok::Keyword),
            ("null", Tok::Lit),
        ]
        .into_iter()
        .map(|(s, t)| (s.to_string(), t))
        .collect();
        assert_eq!(marked, expect);
        // mongo: no keywords, JSON strings and literals still
        let mongo = tokenize(r#"{"name": "select", "n": 1, "ok": true}"#, false);
        assert!(mongo.iter().all(|(_, t)| *t != Tok::Keyword));
        assert!(mongo.contains(&("true".into(), Tok::Lit)));
        // plain runs merge: one span per gap, not per char
        assert_eq!(tokenize("a  +  b", true).len(), 1);
    }

    /// Shift+enter adds a line, tab completes a table, a column and keywords.
    #[gpui::test]
    fn query_editor_multiline_and_completion(cx: &mut gpui::TestAppContext) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut app = rt.block_on(App::new(Default::default(), false));
        app.screen = Screen::Browse;
        app.browse_focus = BrowseFocus::Rows;
        app.mode = Mode::Query;
        app.containers = vec!["products".into()];
        app.result = Some(db9s_core::datasource::QueryResult {
            columns: vec!["price".into()],
            rows: vec![],
            affected: None,
        });
        let (view, cx) =
            cx.add_window_view(|_, cx| Db9s::new(app, rt.handle().clone(), Default::default(), cx));
        cx.update(|window, cx| window.focus(&view.read(cx).focus));
        cx.simulate_keystrokes("s e l tab space * space f r o m space p r o tab");
        cx.simulate_keystrokes("shift-enter w h e r tab space p r i tab");
        view.read_with(cx, |v, _| {
            assert_eq!(
                v.app.query_input.value(),
                "select * from products\nwhere price"
            );
        });
        cx.run_until_parked();
    }

    #[test]
    fn formats_metrics() {
        assert_eq!(format_metric(3.), "3");
        assert_eq!(format_metric(2.5), "2.50");
        assert_eq!(format_metric(0.), "0");
    }

    #[test]
    fn ansi_palette() {
        assert_eq!(ansi(9), 0xff0000);
        assert_eq!(ansi(67), 0x5f87af);
        assert_eq!(ansi(229), 0xffffaf);
        assert_eq!(ansi(240), 0x585858);
    }
}
