//! Desktop frontend: the same `App`, screens and keys as the TUI, drawn with GPUI.
use db9s_core::app::{AddField, App, BrowseFocus, FieldEdit, Mode, Screen};
use db9s_core::input::Input;
use db9s_core::key::{KeyCode, KeyEvent};
use db9s_core::theme::{self, BorderType, Color};
use gpui::{
    div, prelude::*, px, rgb, size, uniform_list, Application, Bounds, Context, CursorStyle, Div,
    FocusHandle, Hsla, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, MouseMoveEvent,
    ScrollStrategy, SharedString, Task, UniformListScrollHandle, Window, WindowBounds,
    WindowOptions,
};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

const BG: u32 = 0x1a1b1e;
const FG: u32 = 0xd4d4d4;
const LINE_HEIGHT: f32 = 20.;
/// Menlo advance at text_sm (14px), to size columns from their content.
const CHAR_WIDTH: f32 = 8.5;
const MIN_COL_WIDTH: f32 = 40.;

fn main() {
    let read_only = std::env::args().any(|a| a == "--read-only");
    // sqlx, mongodb, lapin... need a tokio reactor; GPUI has its own executor.
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let cfg = db9s_core::config::load().unwrap_or_default();
    let app = rt.block_on(App::new(cfg, read_only));

    Application::new().run(move |cx: &mut gpui::App| {
        let bounds = Bounds::centered(None, size(px(1200.), px(780.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| Db9s::new(app, rt, cx));
                window.focus(&view.read(cx).focus);
                view
            },
        )
        .unwrap();
        cx.activate(true);
    });
}

struct Db9s {
    app: App,
    rt: tokio::runtime::Runtime,
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
    _poll: Task<()>,
}

impl Db9s {
    fn new(app: App, rt: tokio::runtime::Runtime, cx: &mut Context<Self>) -> Self {
        // same 100ms tick as the TUI loop, for background connection attempts
        let poll = cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            let alive = this.update(cx, |this, cx| {
                if this.app.connecting.is_some() {
                    this.rt.block_on(this.app.poll_connecting());
                    cx.notify();
                }
            });
            if alive.is_err() {
                break;
            }
        });
        Self {
            app,
            rt,
            focus: cx.focus_handle(),
            conn_scroll: Default::default(),
            db_scroll: Default::default(),
            container_scroll: Default::default(),
            row_scroll: Default::default(),
            col_widths: HashMap::new(),
            resizing: None,
            unfolded: HashSet::new(),
            tree_scroll: Default::default(),
            _poll: poll,
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if ev.keystroke.modifiers.platform && ev.keystroke.key == "q" {
            cx.quit();
            return;
        }
        let Some(key) = to_core_key(&ev.keystroke) else {
            return;
        };
        // shortcut: blocks the UI while a query runs, exactly like the TUI loop does;
        // move to a background task + "loading" state once queries need to be cancellable
        self.rt.block_on(self.app.handle_key(key));
        if let Some(req) = self.app.editor_request.take() {
            let _ = std::fs::remove_file(&req.tmp_path);
            self.app.status = "error: E ($EDITOR) is TUI-only for now, use e".into();
        }
        if self.app.should_quit {
            cx.quit();
        }
        cx.notify();
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

impl Render for Db9s {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette();
        let app = &self.app;
        let mut root = div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
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
            .font_family("Menlo")
            .text_sm()
            .line_height(px(LINE_HEIGHT))
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
        root = root.child(body).child(footer(app, &p));
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
        let right = match &app.field_edit {
            Some(fe) => field_editor(fe, p),
            None if containers_focused => self.grid(false, p, cx),
            None => div()
                .flex()
                .flex_col()
                .gap_1()
                .min_h_0()
                .child(self.grid(true, p, cx).flex_1().min_h_0())
                .child(self.inspector(p, cx).h_2_5()),
        };
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
            .unwrap_or_else(|| auto_width(col, rows))
    }
}

/// A column sized to its name and the first rows' values, capped so one long JSON
/// blob doesn't take the whole screen.
fn auto_width(col: &str, rows: &[db9s_core::value::Row]) -> f32 {
    let longest = rows
        .iter()
        .take(50)
        .filter_map(|r| r.get(col))
        .map(|v| compact(v).chars().count())
        .chain([col.chars().count()])
        .max()
        .unwrap_or(0);
    (longest.clamp(4, 40) as f32 * CHAR_WIDTH + 16.).max(MIN_COL_WIDTH)
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
        let header = div()
            .flex()
            .flex_row()
            .w(px(total))
            .border_b_1()
            .border_color(p.border_blur)
            .children(res.columns.iter().zip(&widths).map(|(col, w)| {
                let (col_name, w) = (col.clone(), *w);
                div()
                    .relative()
                    .flex_shrink_0()
                    .w(px(w))
                    .px_1()
                    .truncate()
                    .text_color(p.field_name)
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(col.clone())
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
                        let cells = res.columns.iter().zip(widths.iter()).map(|(col, w)| {
                            let cell = div().flex_shrink_0().w(px(*w)).px_1().truncate();
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
    pane("edit row", p.border_focus, p).children(lines)
}

fn footer(app: &App, p: &Palette) -> Div {
    if matches!(app.mode, Mode::Query) {
        let prompt = if app.is_queue() {
            "publish> "
        } else {
            "query> "
        };
        let mut spans = vec![raw(prompt)];
        spans.extend(input_spans(&app.query_input, true));
        return line(spans).text_color(p.selected_fg);
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
        Mode::ConfirmDelete => modal_box("confirm", p).child(line(vec![styled(
            if app.is_queue() {
                "purge every message in this queue?"
            } else {
                "delete this row/document?"
            },
            p.success,
        )
        .bold()])),
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
        assert_eq!(auto_width("id", &rows), 4. * CHAR_WIDTH + 16.);
        assert_eq!(auto_width("bio", &rows), 40. * CHAR_WIDTH + 16.);
        assert_eq!(
            auto_width("a_long_column_name", &[]),
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
        let (view, cx) = cx.add_window_view(|_, cx| Db9s::new(app, rt, cx));
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
        let (view, cx) = cx.add_window_view(|_, cx| Db9s::new(app, rt, cx));
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

    #[test]
    fn ansi_palette() {
        assert_eq!(ansi(9), 0xff0000);
        assert_eq!(ansi(67), 0x5f87af);
        assert_eq!(ansi(229), 0xffffaf);
        assert_eq!(ansi(240), 0x585858);
    }
}
