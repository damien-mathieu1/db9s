use crate::app::{App, BrowseFocus, Mode, Screen};
use crate::theme;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph},
    Frame,
};

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);
    match app.screen {
        Screen::Connections => draw_connections(f, chunks[1], app),
        Screen::Databases => draw_list(f, chunks[1], "databases", &app.databases, app.db_idx, true),
        Screen::Browse => draw_browse(f, chunks[1], app),
    }
    draw_footer(f, app, chunks[2]);

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
    let title = Paragraph::new(Line::from(vec![Span::styled(
        format!(" db9s :: {path} "),
        Style::default()
            .fg(theme::BORDER_FOCUS)
            .add_modifier(Modifier::BOLD),
    )]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(theme::BORDER_TYPE)
            .border_style(Style::default().fg(theme::BORDER_BLUR)),
    );
    f.render_widget(title, area);
}

fn conn_name(app: &App) -> String {
    app.conns
        .get(app.conn_idx)
        .map(|c| c.name.clone())
        .unwrap_or_default()
}

fn selected_style() -> Style {
    Style::default()
        .fg(theme::SELECTED_FG)
        .bg(theme::SELECTED_BG)
}

fn pane_border(focused: bool) -> Style {
    Style::default().fg(if focused {
        theme::BORDER_FOCUS
    } else {
        theme::BORDER_BLUR
    })
}

fn draw_connections(f: &mut Frame, area: Rect, app: &App) {
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
                    Style::default().fg(theme::DOC_TEXT),
                ),
                Span::styled(masked, Style::default().fg(theme::HELP_TEXT)),
                Span::styled(status, Style::default().fg(theme::ERROR)),
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
            .border_type(theme::BORDER_TYPE)
            .border_style(pane_border(true))
            .title("connections  (a add, e edit, d delete, s settings)"),
    );
    f.render_widget(list, area);
}

fn draw_list(
    f: &mut Frame,
    area: Rect,
    title: &str,
    items: &[String],
    selected: usize,
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
            .border_type(theme::BORDER_TYPE)
            .border_style(pane_border(focused))
            .title(title),
    );
    f.render_widget(list, area);
}

fn draw_browse(f: &mut Frame, area: Rect, app: &App) {
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
        containers_focused,
    );
    match &app.field_edit {
        Some(fe) => draw_field_editor(f, cols[1], fe),
        None => draw_rows_pane(f, cols[1], app, !containers_focused),
    }
}

/// Inline row editor: one `field: value` line per field, changed fields marked with `●`.
fn draw_field_editor(f: &mut Frame, area: Rect, fe: &crate::app::FieldEdit) {
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
                    Style::default().fg(theme::CONFIRM),
                ),
                Span::styled(
                    format!("{k}: "),
                    Style::default()
                        .fg(theme::DOC_TEXT)
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
            .border_type(theme::BORDER_TYPE)
            .border_style(pane_border(true))
            .title("edit row"),
    );
    f.render_widget(list, area);
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
        .border_type(theme::BORDER_TYPE)
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
                        .fg(theme::DOC_TEXT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(compact(&val)),
            ]));
        }
        let card = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(theme::BORDER_TYPE)
                .border_style(if selected {
                    Style::default().fg(theme::BORDER_FOCUS)
                } else {
                    Style::default().fg(theme::BORDER_BLUR)
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
            .style(Style::default().fg(theme::SELECTED_FG)),
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
                    .fg(theme::ERROR)
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
            theme::ERROR
        } else {
            theme::HELP_TEXT
        };
        line.spans.push(Span::styled(
            format!("  │ {}", app.status),
            Style::default().fg(color),
        ));
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
                        .fg(theme::HELP_KEY)
                        .add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    desc.to_string(),
                    Style::default().fg(theme::HELP_TEXT),
                ));
            }
            None => spans.push(Span::styled(
                chunk.to_string(),
                Style::default().fg(theme::HELP_TEXT),
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
        .border_style(Style::default().fg(theme::MODAL_BORDER))
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
            .fg(theme::CONFIRM)
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
                    .fg(theme::DOC_TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(old_s, Style::default().fg(theme::ERROR)),
            Span::raw(" -> "),
            Span::styled(new_s, Style::default().fg(theme::CONFIRM)),
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
    let area = centered(f.area(), 55, 25);
    f.render_widget(ratatui::widgets::Clear, area);

    let outer = modal_block("settings (enter save, esc cancel)");
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(Line::from(app.settings_editor_input.spans(true)))
            .style(selected_style())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER_TYPE)
                    .title("editor (for row/doc edit)"),
            ),
        rows[0],
    );

    if let Some(err) = &app.settings_error {
        f.render_widget(
            Paragraph::new(err.as_str()).style(Style::default().fg(theme::ERROR)),
            rows[2],
        );
    } else {
        f.render_widget(
            Paragraph::new("saved to config.toml, used next time you press 'e'")
                .style(Style::default().fg(theme::HELP_TEXT)),
            rows[2],
        );
    }
}

fn draw_add_conn_popup(f: &mut Frame, app: &App) {
    use crate::app::AddField;

    let area = centered(f.area(), 60, 40);
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
                .border_type(theme::BORDER_TYPE)
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
                .border_type(theme::BORDER_TYPE)
                .title("url"),
        ),
        rows[1],
    );
    let kind_label = format!("< {} >", app.new_conn_kind.label());
    f.render_widget(
        Paragraph::new(kind_label)
            .style(field_style(AddField::Kind))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER_TYPE)
                    .title("kind (left/right)"),
            ),
        rows[2],
    );

    if let Some(msg) = &app.new_conn_error {
        let color = if msg.starts_with("connected") || msg == "connection ok" {
            theme::CONFIRM
        } else if msg == "connecting..." {
            theme::SELECTED_FG
        } else {
            theme::ERROR
        };
        f.render_widget(
            Paragraph::new(msg.as_str()).style(Style::default().fg(color)),
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
                    .fg(theme::CONFIRM)
                    .add_modifier(Modifier::BOLD),
            )
            .block(modal_block("confirm")),
        area,
    );
}
