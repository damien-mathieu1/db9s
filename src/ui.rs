use crate::app::{App, BrowseFocus, Mode, Screen};
use crate::theme;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

pub fn draw(f: &mut Frame, app: &mut App) {
    let prompt = matches!(app.mode, Mode::Command);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(if prompt { 3 } else { 0 }),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);
    if prompt {
        draw_command_prompt(f, chunks[1], app);
    }
    let body = chunks[2];
    match app.screen {
        Screen::Connections => draw_connections(f, body, app),
        Screen::Databases => draw_list(
            f,
            body,
            "databases",
            &app.databases,
            app.db_idx,
            &mut app.db_list,
            true,
        ),
        Screen::Browse => draw_browse(f, body, app),
    }
    draw_footer(f, app, chunks[3]);

    if matches!(app.mode, Mode::ConfirmEditSave) {
        draw_confirm_edit_popup(f, app);
    }
    if matches!(app.mode, Mode::ConfirmDelete) {
        draw_confirm_popup(
            f,
            if app.is_queue() {
                "purge every message in this queue?"
            } else {
                "delete this row/document?"
            },
        );
    }
    if matches!(app.mode, Mode::ConfirmDeleteConn) {
        let name = app
            .conns
            .get(app.conn_idx)
            .map(|c| c.name.as_str())
            .unwrap_or("");
        draw_confirm_popup(f, &format!("delete connection '{name}'?"));
    }
    if matches!(app.mode, Mode::AddConnection) {
        draw_add_conn_popup(f, app);
    }
    if matches!(app.mode, Mode::Settings) {
        draw_settings_popup(f, app);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let path = match app.screen {
        Screen::Connections => "connections".to_string(),
        Screen::Databases => format!("{} >", conn_name(app)),
        Screen::Browse => format!(
            "{} > {} > {}",
            conn_name(app),
            app.databases.get(app.db_idx).cloned().unwrap_or_default(),
            app.containers
                .get(app.container_idx)
                .cloned()
                .unwrap_or_default()
        ),
    };
    let mut spans = vec![Span::styled(
        format!(" db9s :: {path} "),
        Style::default()
            .fg(theme::get().border_focus)
            .add_modifier(Modifier::BOLD),
    )];
    if app.is_read_only() {
        spans.push(read_only_badge());
    }
    let title = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(theme::get().border_type)
            .border_style(Style::default().fg(theme::get().border_blur)),
    );
    f.render_widget(title, area);
}

/// k9s-style command bar: `> typed` followed by the greyed rest of the suggestion.
fn read_only_badge() -> Span<'static> {
    Span::styled(
        "READ-ONLY ",
        Style::default()
            .fg(theme::get().error)
            .add_modifier(Modifier::BOLD),
    )
}

fn draw_command_prompt(f: &mut Frame, area: Rect, app: &App) {
    let mut spans = vec![Span::styled(
        "> ",
        Style::default()
            .fg(theme::get().border_focus)
            .add_modifier(Modifier::BOLD),
    )];
    match app.command_suggestion().filter(|s| !s.is_empty()) {
        Some(rest) => {
            let mut chars = rest.chars();
            let first = chars.next().unwrap_or(' ').to_string();
            spans.push(Span::raw(app.command_input.value().to_string()));
            spans.push(Span::styled(
                first,
                Style::default()
                    .fg(theme::get().muted)
                    .add_modifier(Modifier::REVERSED),
            ));
            spans.push(Span::styled(
                chars.as_str().to_string(),
                Style::default().fg(theme::get().muted),
            ));
        }
        None => spans.extend(app.command_input.spans(true)),
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(theme::get().border_type)
                .border_style(pane_border(true))
                .title("command"),
        ),
        area,
    );
}

fn conn_name(app: &App) -> String {
    app.conns
        .get(app.conn_idx)
        .map(|c| c.name.clone())
        .unwrap_or_default()
}

fn selected_style() -> Style {
    Style::default()
        .fg(theme::get().selected_fg)
        .bg(theme::get().selected_bg)
}

fn pane_border(focused: bool) -> Style {
    Style::default().fg(if focused {
        theme::get().border_focus
    } else {
        theme::get().border_blur
    })
}

fn draw_connections(f: &mut Frame, area: Rect, app: &mut App) {
    let items: Vec<ListItem> = app
        .conns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let masked = crate::app::mask_password(&c.url);
            let status = match &c.source {
                Ok(_) => String::new(),
                Err(e) => format!("  (error: {e})"),
            };
            let line = Line::from(vec![
                Span::styled(
                    format!("{:<20}", c.name),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("[{}]  ", c.kind.label()),
                    Style::default().fg(theme::get().field_name),
                ),
                if c.read_only || app.force_read_only {
                    read_only_badge()
                } else {
                    Span::raw("")
                },
                Span::styled(masked, Style::default().fg(theme::get().muted)),
                Span::styled(status, Style::default().fg(theme::get().error)),
            ]);
            let style = if i == app.conn_idx {
                selected_style()
            } else {
                Style::default()
            };
            ListItem::new(line).style(style)
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(theme::get().border_type)
            .border_style(pane_border(true))
            .title("connections  (a add, e edit, d delete, s settings)"),
    );
    app.conn_list
        .select((!app.conns.is_empty()).then_some(app.conn_idx));
    f.render_stateful_widget(list, area, &mut app.conn_list);
}

fn draw_list(
    f: &mut Frame,
    area: Rect,
    title: &str,
    items: &[String],
    selected: usize,
    state: &mut ListState,
    focused: bool,
) {
    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let style = if i == selected {
                selected_style()
            } else {
                Style::default()
            };
            ListItem::new(s.as_str()).style(style)
        })
        .collect();
    let list = List::new(list_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(theme::get().border_type)
            .border_style(pane_border(focused))
            .title(title),
    );
    state.select((!items.is_empty()).then_some(selected));
    f.render_stateful_widget(list, area, state);
}

fn draw_browse(f: &mut Frame, area: Rect, app: &mut App) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(33), Constraint::Percentage(67)])
        .split(area);

    let containers_focused = app.browse_focus == BrowseFocus::Containers;
    draw_list(
        f,
        cols[0],
        "containers  (enter open)",
        &app.containers,
        app.container_idx,
        &mut app.container_list,
        containers_focused,
    );
    match &app.field_edit {
        Some(fe) => draw_field_editor(f, cols[1], fe, &mut app.field_list),
        None => draw_rows_pane(f, cols[1], app, !containers_focused),
    }
}

/// Inline row editor: one `field: value` line per field, changed fields marked with `●`.
fn draw_field_editor(f: &mut Frame, area: Rect, fe: &crate::app::FieldEdit, state: &mut ListState) {
    let items: Vec<ListItem> = fe
        .edited
        .iter()
        .enumerate()
        .map(|(i, (k, v))| {
            let changed = fe.original.get(k) != Some(v);
            let value = match (&fe.input, i == fe.idx) {
                (Some(input), true) => input.spans(true),
                _ => vec![Span::raw(compact(v))],
            };
            let mut style = Style::default();
            if i == fe.idx {
                style = selected_style();
            }
            let mut spans = vec![
                Span::styled(
                    if changed { "● " } else { "  " },
                    Style::default().fg(theme::get().success),
                ),
                Span::styled(
                    format!("{k}: "),
                    Style::default()
                        .fg(theme::get().field_name)
                        .add_modifier(Modifier::BOLD),
                ),
            ];
            spans.extend(value);
            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(theme::get().border_type)
            .border_style(pane_border(true))
            .title("edit row"),
    );
    state.select((!fe.edited.is_empty()).then_some(fe.idx));
    f.render_stateful_widget(list, area, state);
}

/// Each row/document renders as its own card, mongotui-style: up to 4
/// `field: value` lines inside a bordered box, selected card highlighted.
fn draw_rows_pane(f: &mut Frame, area: Rect, app: &App, focused: bool) {
    let title = match &app.result {
        Some(res) => format!("rows  (offset {}, showing {})", app.offset, res.rows.len()),
        None => "rows".to_string(),
    };
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(theme::get().border_type)
        .border_style(pane_border(focused))
        .title(title);
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let Some(res) = &app.result else {
        f.render_widget(
            Paragraph::new("no data — select a container and press enter"),
            inner,
        );
        return;
    };
    if res.rows.is_empty() {
        f.render_widget(Paragraph::new("no rows"), inner);
        return;
    }

    const CARD_HEIGHT: u16 = 6;
    let visible = ((inner.height / CARD_HEIGHT).max(1)) as usize;
    let start = if app.row_idx >= visible {
        app.row_idx + 1 - visible
    } else {
        0
    };
    let end = (start + visible).min(res.rows.len());

    let constraints: Vec<Constraint> = (start..end)
        .map(|_| Constraint::Length(CARD_HEIGHT))
        .collect();
    let card_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    for (slot, idx) in (start..end).enumerate() {
        let row = &res.rows[idx];
        let selected = focused && idx == app.row_idx;
        let mut lines = vec![];
        for col in res.columns.iter().take(4) {
            let val = row.get(col).cloned().unwrap_or(serde_json::Value::Null);
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{col}: "),
                    Style::default()
                        .fg(theme::get().field_name)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(compact(&val)),
            ]));
        }
        let card = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(theme::get().border_type)
                .border_style(if selected {
                    Style::default().fg(theme::get().border_focus)
                } else {
                    Style::default().fg(theme::get().border_blur)
                }),
        );
        f.render_widget(card, card_areas[slot]);
    }
}

fn compact(v: &serde_json::Value) -> String {
    let s = match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "null".into(),
        other => other.to_string(),
    };
    if s.len() > 100 {
        format!("{}...", &s[..97])
    } else {
        s
    }
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    if matches!(app.mode, Mode::Query) {
        f.render_widget(
            Paragraph::new(Line::from(
                [
                    vec![Span::raw(if app.is_queue() {
                        "publish> "
                    } else {
                        "query> "
                    })],
                    app.query_input.spans(true),
                ]
                .concat(),
            ))
            .style(Style::default().fg(theme::get().selected_fg)),
            area,
        );
        return;
    }
    if matches!(
        app.mode,
        Mode::ConfirmDelete | Mode::ConfirmDeleteConn | Mode::ConfirmEditSave
    ) {
        f.render_widget(
            Paragraph::new("y/n").style(
                Style::default()
                    .fg(theme::get().error)
                    .add_modifier(Modifier::BOLD),
            ),
            area,
        );
        return;
    }
    let mut line = help_line(app.key_hints());
    if !app.status.is_empty() {
        let is_err = app.status.starts_with("error") || app.status.starts_with("failed");
        let color = if is_err {
            theme::get().error
        } else {
            theme::get().muted
        };
        let status = Span::styled(app.status.clone(), Style::default().fg(color));
        // errors go first so a long key list never truncates them
        if is_err {
            line.spans.insert(0, Span::raw("  │ "));
            line.spans.insert(0, status);
        } else {
            line.spans.push(Span::raw("  │ "));
            line.spans.push(status);
        }
    }
    f.render_widget(Paragraph::new(line), area);
}

/// Renders a "key action  key action" hint string with bold keys, muted descriptions.
fn help_line(status: &str) -> Line<'static> {
    let mut spans = vec![];
    for (i, chunk) in status.split("  ").enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        match chunk.split_once(' ') {
            Some((key, desc)) => {
                spans.push(Span::styled(
                    key.to_string(),
                    Style::default()
                        .fg(theme::get().key_hint)
                        .add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    desc.to_string(),
                    Style::default().fg(theme::get().muted),
                ));
            }
            None => spans.push(Span::styled(
                chunk.to_string(),
                Style::default().fg(theme::get().muted),
            )),
        }
    }
    Line::from(spans)
}

fn centered(area: Rect, pct_x: u16, pct_y: u16) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .split(v[1])[1]
}

fn modal_block(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::get().modal_border))
        .title(title.to_string())
}

/// Shows what changed after $EDITOR closed: one line per added/removed/changed
/// field, before asking the user to confirm the write.
fn draw_confirm_edit_popup(f: &mut Frame, app: &App) {
    let Some((original, edited)) = &app.pending_edit else {
        return;
    };

    let mut keys: Vec<&String> = original.keys().chain(edited.keys()).collect();
    keys.sort();
    keys.dedup();

    let mut lines = vec![Line::from(Span::styled(
        "save these changes? y/n",
        Style::default()
            .fg(theme::get().success)
            .add_modifier(Modifier::BOLD),
    ))];
    lines.push(Line::raw(""));

    for key in keys {
        let old = original.get(key);
        let new = edited.get(key);
        if old == new {
            continue;
        }
        let old_s = old.map(compact).unwrap_or_else(|| "<absent>".into());
        let new_s = new.map(compact).unwrap_or_else(|| "<removed>".into());
        lines.push(Line::from(vec![
            Span::styled(
                format!("{key}: "),
                Style::default()
                    .fg(theme::get().field_name)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(old_s, Style::default().fg(theme::get().error)),
            Span::raw(" -> "),
            Span::styled(new_s, Style::default().fg(theme::get().success)),
        ]));
    }

    let area = centered(f.area(), 70, 60);
    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(modal_block("confirm edit")),
        area,
    );
}

fn draw_settings_popup(f: &mut Frame, app: &App) {
    let mut area = centered(f.area(), 55, 35);
    // two 3-line fields + message + borders must fit, whatever the terminal height
    area.height = area.height.max(10).min(f.area().height);
    area.y = (f.area().height - area.height) / 2;
    f.render_widget(ratatui::widgets::Clear, area);

    let outer = modal_block("settings (tab field, ←/→ theme, enter save, esc cancel)");
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    let field_style = |focused: bool| {
        if focused {
            selected_style()
        } else {
            Style::default()
        }
    };
    f.render_widget(
        Paragraph::new(Line::from(
            app.settings_editor_input.spans(!app.settings_on_theme),
        ))
        .style(field_style(!app.settings_on_theme))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(theme::get().border_type)
                .title("editor (for row/doc edit)"),
        ),
        rows[0],
    );
    f.render_widget(
        Paragraph::new(format!("< {} >", app.settings_theme))
            .style(field_style(app.settings_on_theme))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::get().border_type)
                    .title("theme (live preview)"),
            ),
        rows[1],
    );

    let (msg, color) = match &app.settings_error {
        Some(err) => (err.as_str(), theme::get().error),
        None => (
            "saved to config.toml; custom themes go in <config dir>/themes/<name>.toml",
            theme::get().muted,
        ),
    };
    f.render_widget(
        Paragraph::new(msg).style(Style::default().fg(color)),
        rows[3],
    );
}

fn draw_add_conn_popup(f: &mut Frame, app: &App) {
    use crate::app::AddField;

    let area = centered(f.area(), 60, 60);
    f.render_widget(ratatui::widgets::Clear, area);

    let base = if app.edit_target.is_some() {
        "edit connection"
    } else {
        "add connection"
    };
    let title = format!("{base} (tab move, enter next/submit, ctrl+t test, esc cancel)");
    let outer = modal_block(&title);
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    let field_style = |f: AddField| {
        if app.new_conn_field == f {
            selected_style()
        } else {
            Style::default()
        }
    };

    f.render_widget(
        Paragraph::new(Line::from(
            app.new_conn_name
                .spans(app.new_conn_field == AddField::Name),
        ))
        .style(field_style(AddField::Name))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(theme::get().border_type)
                .title("name"),
        ),
        rows[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(
            app.new_conn_url.spans(app.new_conn_field == AddField::Url),
        ))
        .style(field_style(AddField::Url))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(theme::get().border_type)
                .title("url"),
        ),
        rows[1],
    );
    let last_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[2]);
    let kind_label = format!("< {} >", app.new_conn_kind.label());
    f.render_widget(
        Paragraph::new(kind_label)
            .style(field_style(AddField::Kind))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::get().border_type)
                    .title("kind (left/right)"),
            ),
        last_row[0],
    );
    let read_only_label = if app.new_conn_read_only {
        "< on >"
    } else {
        "< off >"
    };
    f.render_widget(
        Paragraph::new(read_only_label)
            .style(field_style(AddField::ReadOnly))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::get().border_type)
                    .title("read-only (space)"),
            ),
        last_row[1],
    );

    if let Some(msg) = &app.new_conn_error {
        let color = if msg.starts_with("connected") || msg == "connection ok" {
            theme::get().success
        } else if msg == "connecting..." {
            theme::get().selected_fg
        } else {
            theme::get().error
        };
        f.render_widget(
            Paragraph::new(msg.as_str()).style(Style::default().fg(color)),
            rows[4],
        );
    } else {
        f.render_widget(
            Paragraph::new("tip: ${PG_PASSWORD} or \"cmd: op read op://...\" keeps passwords out of config.toml")
                // DIM rather than a theme color: renders right under every theme
                .style(Style::default().add_modifier(Modifier::DIM)),
            rows[4],
        );
    }
}

fn draw_confirm_popup(f: &mut Frame, text: &str) {
    let area = centered(f.area(), 40, 15);
    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(
        Paragraph::new(text)
            .style(
                Style::default()
                    .fg(theme::get().success)
                    .add_modifier(Modifier::BOLD),
            )
            .block(modal_block("confirm")),
        area,
    );
}
