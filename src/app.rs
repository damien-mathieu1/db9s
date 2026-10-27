use crate::config::{Config, Kind};
use crate::datasource::{DataSource, QueryResult};
use crate::input::Input;
use crate::value::Row;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;
use std::time::Duration;
use tokio::task::JoinHandle;

pub const PAGE_SIZE: i64 = 200;
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Connections,
    Databases,
    /// Split-pane browser: containers on the left, rows/documents on the right.
    Browse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseFocus {
    Containers,
    Rows,
}

pub enum Mode {
    Normal,
    Query,
    ConfirmDelete,
    ConfirmEditSave,
    FieldEdit,
    AddConnection,
    ConfirmDeleteConn,
    Settings,
}

/// Inline, field-by-field edit of one row/document, saved in one update.
pub struct FieldEdit {
    pub original: Row,
    pub edited: Row,
    pub idx: usize,
    /// `Some` while the selected field's value is being typed.
    pub input: Option<Input>,
}

/// A row/document sent out to the user's $EDITOR; picked up by the main loop
/// once set, since only it owns the terminal (needed to suspend/resume it).
pub struct EditorRequest {
    pub tmp_path: PathBuf,
    pub original: Row,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddField {
    Name,
    Url,
    Kind,
}

pub struct ConnEntry {
    pub name: String,
    pub kind: Kind,
    pub url: String,
    pub source: Result<Box<dyn DataSource>>,
}

/// Masks the password portion of a connection string, e.g.
/// `postgres://user:secret@host/db` -> `postgres://user:***@host/db`.
pub fn mask_password(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let after = &url[scheme_end + 3..];
    let Some(at_pos) = after.find('@') else {
        return url.to_string();
    };
    let userinfo = &after[..at_pos];
    let Some(colon_pos) = userinfo.find(':') else {
        return url.to_string();
    };
    let user = &userinfo[..colon_pos];
    format!("{}{}:***{}", &url[..scheme_end + 3], user, &after[at_pos..])
}

pub struct App {
    pub conns: Vec<ConnEntry>,
    pub conn_idx: usize,
    pub db_idx: usize,
    pub container_idx: usize,
    pub row_idx: usize,

    pub screen: Screen,
    pub browse_focus: BrowseFocus,
    pub mode: Mode,

    pub databases: Vec<String>,
    pub containers: Vec<String>,
    pub result: Option<QueryResult>,
    pub offset: i64,

    pub query_input: Input,
    pub status: String,
    pub should_quit: bool,

    pub editor_request: Option<EditorRequest>,
    pub pending_edit: Option<(Row, Row)>,
    pub field_edit: Option<FieldEdit>,
    pub editor_cmd: String,
    pub settings_editor_input: Input,
    pub settings_error: Option<String>,

    pub new_conn_name: Input,
    pub new_conn_url: Input,
    pub new_conn_kind: Kind,
    pub new_conn_field: AddField,
    pub new_conn_error: Option<String>,
    pub edit_target: Option<usize>,
    pub connecting: Option<PendingConnect>,
}

async fn connect(kind: Kind, url: &str) -> Result<Box<dyn DataSource>> {
    use crate::datasource::{
        amqp::AmqpSource, mongo::MongoSource, postgres::PostgresSource, rabbitmq::RabbitMqSource,
        redis::RedisSource,
    };
    let fut = async {
        Ok(match kind {
            Kind::Postgres => Box::new(PostgresSource::connect(url).await?) as Box<dyn DataSource>,
            Kind::Mongo => Box::new(MongoSource::connect(url).await?),
            Kind::RabbitMq if url.starts_with("amqp") => Box::new(AmqpSource::connect(url).await?),
            Kind::RabbitMq => Box::new(RabbitMqSource::connect(url).await?),
            Kind::Redis => Box::new(RedisSource::connect(url).await?),
        })
    };
    tokio::time::timeout(CONNECT_TIMEOUT, fut)
        .await
        .unwrap_or_else(|_| {
            Err(anyhow::anyhow!(
                "connection timed out after {}s",
                CONNECT_TIMEOUT.as_secs()
            ))
        })
}

fn spawn_connect(kind: Kind, url: String) -> JoinHandle<Result<Box<dyn DataSource>>> {
    tokio::spawn(async move { connect(kind, &url).await })
}

/// A connection attempt running in the background, so the UI never blocks on
/// a slow or unreachable server; polled once per event-loop tick.
pub struct PendingConnect {
    kind: Kind,
    name: String,
    url: String,
    save: bool,
    edit_idx: Option<usize>,
    handle: JoinHandle<Result<Box<dyn DataSource>>>,
}

impl App {
    pub async fn new(cfg: Config) -> Self {
        let editor_cmd = cfg
            .editor
            .clone()
            .or_else(|| std::env::var("EDITOR").ok())
            .unwrap_or_else(|| "vi".to_string());

        // connect concurrently so N unreachable servers cost one timeout, not N
        let futs = cfg.connections.into_iter().map(|c| async move {
            let source = connect(c.kind, &c.url).await;
            ConnEntry {
                name: c.name,
                kind: c.kind,
                url: c.url,
                source,
            }
        });
        let conns = futures_util::future::join_all(futs).await;

        let mode = if conns.is_empty() {
            Mode::AddConnection
        } else {
            Mode::Normal
        };

        Self {
            conns,
            conn_idx: 0,
            db_idx: 0,
            container_idx: 0,
            row_idx: 0,
            screen: Screen::Connections,
            browse_focus: BrowseFocus::Containers,
            mode,
            databases: vec![],
            containers: vec![],
            result: None,
            offset: 0,
            query_input: Input::default(),
            status: String::new(),
            should_quit: false,
            editor_request: None,
            pending_edit: None,
            field_edit: None,
            editor_cmd: editor_cmd.clone(),
            settings_editor_input: Input::new(editor_cmd),
            settings_error: None,
            new_conn_name: Input::default(),
            new_conn_url: Input::default(),
            new_conn_kind: Kind::Postgres,
            new_conn_field: AddField::Name,
            new_conn_error: None,
            edit_target: None,
            connecting: None,
        }
    }

    /// Checks on any in-flight connection attempt; called once per event-loop tick
    /// so the UI stays responsive while (dis)connecting.
    pub async fn poll_connecting(&mut self) {
        let is_done = match &self.connecting {
            Some(pc) => pc.handle.is_finished(),
            None => false,
        };
        if !is_done {
            return;
        }
        let pc = self.connecting.take().unwrap();
        let result = match pc.handle.await {
            Ok(r) => r,
            Err(e) => Err(anyhow::anyhow!("connect task panicked: {e}")),
        };
        match result {
            Err(e) => self.new_conn_error = Some(format!("failed: {e}")),
            Ok(_source) if !pc.save => {
                self.new_conn_error = Some("connection ok".into());
            }
            Ok(source) => self.finalize_conn(pc.kind, pc.name, pc.url, source, pc.edit_idx),
        }
    }

    fn finalize_conn(
        &mut self,
        kind: Kind,
        name: String,
        url: String,
        source: Box<dyn DataSource>,
        edit_idx: Option<usize>,
    ) {
        if let Some(idx) = edit_idx {
            let old_name = self.conns[idx].name.clone();
            if old_name != name {
                let _ = crate::config::remove_connection(&old_name);
            }
            if let Err(e) = crate::config::add_connection(crate::config::Connection {
                name: name.clone(),
                kind,
                url: url.clone(),
            }) {
                self.status = format!("connected, but failed to save to config: {e}");
            } else {
                self.status = format!("updated '{name}'");
            }
            self.conns[idx] = ConnEntry {
                name,
                kind,
                url,
                source: Ok(source),
            };
            self.conn_idx = idx;
        } else {
            if let Err(e) = crate::config::add_connection(crate::config::Connection {
                name: name.clone(),
                kind,
                url: url.clone(),
            }) {
                self.status = format!("connected, but failed to save to config: {e}");
            } else {
                self.status = format!("connected and saved '{name}'");
            }
            self.conns.push(ConnEntry {
                name,
                kind,
                url,
                source: Ok(source),
            });
            self.conn_idx = self.conns.len() - 1;
        }
        self.edit_target = None;
        self.mode = Mode::Normal;
    }

    fn current_source(&self) -> Option<&dyn DataSource> {
        self.conns
            .get(self.conn_idx)
            .and_then(|c| c.source.as_deref().ok())
    }

    fn current_db(&self) -> Option<&str> {
        self.databases.get(self.db_idx).map(|s| s.as_str())
    }

    fn current_container(&self) -> Option<&str> {
        self.containers.get(self.container_idx).map(|s| s.as_str())
    }

    pub async fn handle_key(&mut self, key: KeyEvent) {
        match self.mode {
            Mode::Normal => self.handle_normal_key(key).await,
            Mode::Query => self.handle_query_key(key).await,
            Mode::ConfirmEditSave => self.handle_confirm_edit_save_key(key).await,
            Mode::FieldEdit => self.handle_field_edit_key(key).await,
            Mode::ConfirmDelete => self.handle_confirm_key(key).await,
            Mode::AddConnection => self.handle_add_conn_key(key).await,
            Mode::ConfirmDeleteConn => self.handle_confirm_delete_conn_key(key).await,
            Mode::Settings => self.handle_settings_key(key),
        }
    }

    async fn handle_normal_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('q') => match self.screen {
                Screen::Connections => self.should_quit = true,
                _ => self.go_back(),
            },
            KeyCode::Esc => self.go_back(),
            KeyCode::Tab if self.screen == Screen::Browse => {
                self.browse_focus = match self.browse_focus {
                    BrowseFocus::Containers => BrowseFocus::Rows,
                    BrowseFocus::Rows => BrowseFocus::Containers,
                };
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                self.preview_container().await;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                self.preview_container().await;
            }
            KeyCode::Enter => self.enter().await,
            KeyCode::Char('r') if self.rows_focused() => self.load_rows().await,
            KeyCode::Char('n') if self.rows_focused() => {
                self.offset += PAGE_SIZE;
                self.load_rows().await;
            }
            KeyCode::Char('p') if self.rows_focused() => {
                self.offset = (self.offset - PAGE_SIZE).max(0);
                self.load_rows().await;
            }
            KeyCode::Char('/') if self.rows_focused() => {
                self.mode = Mode::Query;
                self.query_input = Input::default();
            }
            KeyCode::Char('e') | KeyCode::Char('E') if self.rows_focused() && self.is_queue() => {
                self.status = "rabbitmq messages are read-only".into();
            }
            KeyCode::Char('e') if self.rows_focused() => self.start_field_edit(),
            KeyCode::Char('E') if self.rows_focused() => self.start_edit(),
            KeyCode::Char('d') if self.rows_focused() && self.selected_row().is_some() => {
                self.mode = Mode::ConfirmDelete;
            }
            KeyCode::Char('s') if self.screen == Screen::Connections => self.start_settings(),
            KeyCode::Char('a') if self.screen == Screen::Connections => self.start_add_conn(),
            KeyCode::Char('e') if self.screen == Screen::Connections => self.start_edit_conn(),
            KeyCode::Char('d') if self.screen == Screen::Connections && !self.conns.is_empty() => {
                self.mode = Mode::ConfirmDeleteConn;
            }
            _ => {}
        }
    }

    /// Key hints for the footer, same for every backend since all go through `DataSource`.
    pub fn key_hints(&self) -> &'static str {
        if let Some(fe) = &self.field_edit {
            return if fe.input.is_some() {
                "←/→ move  enter apply  esc cancel  42/true/null/{..} keep_type  \"..\" forces_text"
            } else {
                "j/k move  enter edit field  ctrl+s save  esc discard"
            };
        }
        match self.screen {
            Screen::Connections => "j/k move  enter open  a add  e edit  d delete  s settings  q quit",
            Screen::Databases => "j/k move  enter open  esc back",
            Screen::Browse => match self.browse_focus {
                BrowseFocus::Containers => "j/k move+preview  enter/tab rows  esc back",
                BrowseFocus::Rows if self.is_queue() => {
                    "j/k move  / publish  d purge queue  n/p page  r refresh  tab queues  esc back"
                }
                BrowseFocus::Rows => {
                    "j/k move  e edit  E $EDITOR  d delete  / query  n/p page  r refresh  tab containers  esc back"
                }
            },
        }
    }

    // shortcut: one fetch per keypress, no debounce; add one if holding j/k on many tables gets slow
    async fn preview_container(&mut self) {
        if self.screen == Screen::Browse && self.browse_focus == BrowseFocus::Containers {
            self.offset = 0;
            self.load_rows().await;
        }
    }

    fn selected_row(&self) -> Option<&Row> {
        self.result.as_ref().and_then(|r| r.rows.get(self.row_idx))
    }

    /// Queues (RabbitMQ) get publish/purge instead of edit/delete.
    pub fn is_queue(&self) -> bool {
        self.current_source().is_some_and(|s| s.is_queue())
    }

    fn rows_focused(&self) -> bool {
        self.screen == Screen::Browse && self.browse_focus == BrowseFocus::Rows
    }

    fn move_selection(&mut self, delta: i32) {
        let len = match self.screen {
            Screen::Connections => self.conns.len(),
            Screen::Databases => self.databases.len(),
            Screen::Browse => match self.browse_focus {
                BrowseFocus::Containers => self.containers.len(),
                BrowseFocus::Rows => self.result.as_ref().map(|r| r.rows.len()).unwrap_or(0),
            },
        };
        if len == 0 {
            return;
        }
        let idx = match self.screen {
            Screen::Connections => &mut self.conn_idx,
            Screen::Databases => &mut self.db_idx,
            Screen::Browse => match self.browse_focus {
                BrowseFocus::Containers => &mut self.container_idx,
                BrowseFocus::Rows => &mut self.row_idx,
            },
        };
        let new = *idx as i32 + delta;
        *idx = new.clamp(0, len as i32 - 1) as usize;
    }

    fn go_back(&mut self) {
        match self.screen {
            Screen::Connections => {}
            Screen::Databases => self.screen = Screen::Connections,
            Screen::Browse => match self.browse_focus {
                BrowseFocus::Rows => {
                    self.browse_focus = BrowseFocus::Containers;
                }
                BrowseFocus::Containers => {
                    self.db_idx = 0;
                    self.containers.clear();
                    self.container_idx = 0;
                    self.result = None;
                    self.offset = 0;
                    self.row_idx = 0;
                    self.screen = Screen::Databases;
                }
            },
        }
    }

    async fn enter(&mut self) {
        match self.screen {
            Screen::Connections => {
                let entry = match self.conns.get(self.conn_idx) {
                    Some(e) => e,
                    None => return,
                };
                if entry.source.is_err() {
                    self.status = format!("connection '{}' failed to connect", entry.name);
                    return;
                }
                self.status = "loading databases...".into();
                match self.current_source().unwrap().list_databases().await {
                    Ok(dbs) => {
                        self.databases = dbs;
                        self.db_idx = 0;
                        self.screen = Screen::Databases;
                        self.status = String::new();
                    }
                    Err(e) => self.status = format!("error: {e}"),
                }
            }
            Screen::Databases => {
                let db = match self.current_db() {
                    Some(d) => d.to_string(),
                    None => return,
                };
                self.status = "loading containers...".into();
                match self.current_source().unwrap().list_containers(&db).await {
                    Ok(c) => {
                        self.containers = c;
                        self.container_idx = 0;
                        self.result = None;
                        self.screen = Screen::Browse;
                        self.browse_focus = BrowseFocus::Containers;
                        self.status = String::new();
                        self.load_rows().await;
                    }
                    Err(e) => self.status = format!("error: {e}"),
                }
            }
            Screen::Browse => match self.browse_focus {
                BrowseFocus::Containers if self.result.is_some() => {
                    self.browse_focus = BrowseFocus::Rows;
                }
                BrowseFocus::Containers => {}
                BrowseFocus::Rows => {}
            },
        }
    }

    async fn load_rows(&mut self) {
        let db = match self.current_db() {
            Some(d) => d.to_string(),
            None => return,
        };
        let container = match self.current_container() {
            Some(c) => c.to_string(),
            None => return,
        };
        self.status = "loading rows...".into();
        match self
            .current_source()
            .unwrap()
            .fetch_rows(&db, &container, PAGE_SIZE, self.offset)
            .await
        {
            Ok(res) => {
                self.row_idx = 0;
                self.status = format!("{} rows (offset {})", res.rows.len(), self.offset);
                self.result = Some(res);
            }
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    async fn handle_query_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => {
                let db = self.current_db().unwrap_or_default().to_string();
                let container = self.current_container().unwrap_or_default().to_string();
                let raw = self.query_input.value().to_string();
                self.mode = Mode::Normal;
                self.status = "running query...".into();
                match self
                    .current_source()
                    .unwrap()
                    .run_query(&db, &container, &raw)
                    .await
                {
                    Ok(res) => {
                        self.row_idx = 0;
                        self.status = match res.affected {
                            Some(_) if self.is_queue() => "message published".into(),
                            Some(n) => format!("{n} rows affected"),
                            None => format!("{} rows", res.rows.len()),
                        };
                        self.result = Some(res);
                    }
                    Err(e) => self.status = format!("error: {e}"),
                }
            }
            _ => {
                self.query_input.handle(key);
            }
        }
    }

    async fn save_row(&mut self, original: &Row, edited: &Row) {
        let db = self.current_db().unwrap_or_default().to_string();
        let container = self.current_container().unwrap_or_default().to_string();
        match self
            .current_source()
            .unwrap()
            .update_row(&db, &container, original, edited)
            .await
        {
            Ok(_) => {
                self.status = "saved".into();
                self.load_rows().await;
            }
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    fn start_field_edit(&mut self) {
        let Some(row) = self.selected_row().cloned() else {
            return;
        };
        self.field_edit = Some(FieldEdit {
            original: row.clone(),
            edited: row,
            idx: 0,
            input: None,
        });
        self.mode = Mode::FieldEdit;
    }

    async fn handle_field_edit_key(&mut self, key: KeyEvent) {
        let Some(fe) = self.field_edit.as_mut() else {
            self.mode = Mode::Normal;
            return;
        };
        let Some(field) = fe.edited.keys().nth(fe.idx).cloned() else {
            self.mode = Mode::Normal;
            return;
        };

        if let Some(input) = fe.input.as_mut() {
            match key.code {
                KeyCode::Enter => {
                    let before = crate::value::value_to_input(&fe.edited[&field]);
                    // untouched text keeps its original type (string "42" stays a string)
                    if input.value() != before {
                        let v = crate::value::input_to_value(input.value());
                        fe.edited.insert(field, v);
                    }
                    fe.input = None;
                }
                KeyCode::Esc => fe.input = None,
                _ => {
                    input.handle(key);
                }
            }
            return;
        }

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                fe.idx = (fe.idx + 1).min(fe.edited.len().saturating_sub(1))
            }
            KeyCode::Char('k') | KeyCode::Up => fe.idx = fe.idx.saturating_sub(1),
            KeyCode::Enter => {
                fe.input = Some(Input::new(crate::value::value_to_input(&fe.edited[&field])))
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let fe = self.field_edit.take().unwrap();
                self.mode = Mode::Normal;
                if fe.edited == fe.original {
                    self.status = "no changes".into();
                } else {
                    self.save_row(&fe.original, &fe.edited).await;
                }
            }
            KeyCode::Esc => {
                let changed = fe.edited != fe.original;
                self.field_edit = None;
                self.mode = Mode::Normal;
                if changed {
                    self.status = "edit discarded".into();
                }
            }
            _ => {}
        }
    }

    /// Writes the selected row to a temp file and hands the request off to the
    /// main loop, which suspends the TUI and opens $EDITOR (or vi) on it.
    fn start_edit(&mut self) {
        let row = match self.selected_row() {
            Some(r) => r.clone(),
            None => return,
        };
        let pretty = serde_json::to_string_pretty(&row).unwrap_or_default();
        let tmp_path = std::env::temp_dir().join("db9s_edit.json");
        if let Err(e) = std::fs::write(&tmp_path, pretty) {
            self.status = format!("failed to open editor: {e}");
            return;
        }
        self.editor_request = Some(EditorRequest {
            tmp_path,
            original: row,
        });
    }

    /// Called by the main loop once $EDITOR exits and the temp file was read back.
    pub fn finish_external_edit(&mut self, raw: &str, original: Row) {
        let edited: Row = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(e) => {
                self.status = format!("invalid json, edit discarded: {e}");
                return;
            }
        };
        if edited == original {
            self.status = "no changes".into();
            return;
        }
        self.pending_edit = Some((original, edited));
        self.mode = Mode::ConfirmEditSave;
    }

    async fn handle_confirm_edit_save_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') => {
                self.mode = Mode::Normal;
                if let Some((original, edited)) = self.pending_edit.take() {
                    self.save_row(&original, &edited).await;
                }
            }
            _ => {
                self.pending_edit = None;
                self.status = "edit discarded".into();
                self.mode = Mode::Normal;
            }
        }
    }

    async fn handle_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') => {
                let row = match self.selected_row() {
                    Some(r) => r.clone(),
                    None => {
                        self.mode = Mode::Normal;
                        return;
                    }
                };
                let db = self.current_db().unwrap_or_default().to_string();
                let container = self.current_container().unwrap_or_default().to_string();
                self.mode = Mode::Normal;
                match self
                    .current_source()
                    .unwrap()
                    .delete_row(&db, &container, &row)
                    .await
                {
                    Ok(_) => {
                        self.status = "deleted".into();
                        self.load_rows().await;
                    }
                    Err(e) => self.status = format!("error: {e}"),
                }
            }
            _ => self.mode = Mode::Normal,
        }
    }

    fn start_settings(&mut self) {
        self.settings_editor_input = Input::new(self.editor_cmd.clone());
        self.settings_error = None;
        self.mode = Mode::Settings;
    }

    fn handle_settings_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => {
                let value = self.settings_editor_input.value().trim().to_string();
                if value.is_empty() {
                    self.settings_error = Some("editor command cannot be empty".into());
                    return;
                }
                match crate::config::set_editor(&value) {
                    Ok(_) => {
                        self.editor_cmd = value;
                        self.status = "settings saved".into();
                        self.mode = Mode::Normal;
                    }
                    Err(e) => self.settings_error = Some(format!("failed to save: {e}")),
                }
            }
            _ => {
                self.settings_editor_input.handle(key);
            }
        }
    }

    fn start_add_conn(&mut self) {
        self.new_conn_name = Input::default();
        self.new_conn_url = Input::default();
        self.new_conn_kind = Kind::Postgres;
        self.new_conn_field = AddField::Name;
        self.new_conn_error = None;
        self.edit_target = None;
        self.mode = Mode::AddConnection;
    }

    fn start_edit_conn(&mut self) {
        let Some(entry) = self.conns.get(self.conn_idx) else {
            return;
        };
        self.new_conn_name = Input::new(entry.name.clone());
        self.new_conn_url = Input::new(entry.url.clone());
        self.new_conn_kind = entry.kind;
        self.new_conn_field = AddField::Name;
        self.new_conn_error = None;
        self.edit_target = Some(self.conn_idx);
        self.mode = Mode::AddConnection;
    }

    async fn handle_confirm_delete_conn_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') => {
                if self.conn_idx < self.conns.len() {
                    let removed = self.conns.remove(self.conn_idx);
                    if let Err(e) = crate::config::remove_connection(&removed.name) {
                        self.status =
                            format!("removed from list, but failed to update config: {e}");
                    } else {
                        self.status = format!("removed '{}'", removed.name);
                    }
                    if self.conn_idx >= self.conns.len() && self.conn_idx > 0 {
                        self.conn_idx -= 1;
                    }
                }
                self.mode = Mode::Normal;
            }
            _ => self.mode = Mode::Normal,
        }
    }

    async fn handle_add_conn_key(&mut self, key: KeyEvent) {
        if let Some(pc) = &self.connecting {
            if key.code == KeyCode::Esc {
                pc.handle.abort();
                self.connecting = None;
                self.new_conn_error = Some("cancelled".into());
            }
            return;
        }
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
            }
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.start_test_conn()
            }
            KeyCode::Tab | KeyCode::Down => {
                self.new_conn_field = match self.new_conn_field {
                    AddField::Name => AddField::Url,
                    AddField::Url => AddField::Kind,
                    AddField::Kind => AddField::Name,
                };
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.new_conn_field = match self.new_conn_field {
                    AddField::Name => AddField::Kind,
                    AddField::Url => AddField::Name,
                    AddField::Kind => AddField::Url,
                };
            }
            KeyCode::Left | KeyCode::Right if self.new_conn_field == AddField::Kind => {
                self.new_conn_kind = self.new_conn_kind.next();
            }
            KeyCode::Enter => {
                if self.new_conn_field != AddField::Kind {
                    self.new_conn_field = match self.new_conn_field {
                        AddField::Name => AddField::Url,
                        AddField::Url => AddField::Kind,
                        AddField::Kind => AddField::Kind,
                    };
                } else {
                    self.start_submit_conn();
                }
            }
            _ => match self.new_conn_field {
                AddField::Name => {
                    self.new_conn_name.handle(key);
                }
                AddField::Url => {
                    self.new_conn_url.handle(key);
                }
                AddField::Kind => {}
            },
        }
    }

    /// Validates the form and returns (name, url), or sets new_conn_error and returns None.
    fn validate_conn_form(&mut self) -> Option<(String, String)> {
        let name = self.new_conn_name.value().trim().to_string();
        let url = self.new_conn_url.value().trim().to_string();
        if name.is_empty() || url.is_empty() {
            self.new_conn_error = Some("name and url are required".into());
            return None;
        }
        let name_taken = self
            .conns
            .iter()
            .enumerate()
            .any(|(i, c)| c.name == name && Some(i) != self.edit_target);
        if name_taken {
            self.new_conn_error = Some(format!("connection '{name}' already exists"));
            return None;
        }
        Some((name, url))
    }

    fn start_test_conn(&mut self) {
        let Some((name, url)) = self.validate_conn_form() else {
            return;
        };
        let kind = self.new_conn_kind;
        self.new_conn_error = Some("connecting...".into());
        self.connecting = Some(PendingConnect {
            kind,
            name: name.clone(),
            url: url.clone(),
            save: false,
            edit_idx: None,
            handle: spawn_connect(kind, url),
        });
    }

    fn start_submit_conn(&mut self) {
        let Some((name, url)) = self.validate_conn_form() else {
            return;
        };
        let kind = self.new_conn_kind;
        self.new_conn_error = Some("connecting...".into());
        self.connecting = Some(PendingConnect {
            kind,
            name: name.clone(),
            url: url.clone(),
            save: true,
            edit_idx: self.edit_target,
            handle: spawn_connect(kind, url),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::mask_password;

    #[test]
    fn masks_postgres_password() {
        assert_eq!(
            mask_password("postgres://db9s:db9s@localhost:55432/db9s"),
            "postgres://db9s:***@localhost:55432/db9s"
        );
    }

    #[test]
    fn masks_mongo_password() {
        assert_eq!(
            mask_password("mongodb://root:s3cr3t@localhost:27017/?authSource=admin"),
            "mongodb://root:***@localhost:27017/?authSource=admin"
        );
    }

    #[test]
    fn leaves_url_without_credentials_unchanged() {
        assert_eq!(
            mask_password("mongodb://localhost:27017"),
            "mongodb://localhost:27017"
        );
    }

    #[test]
    fn leaves_userinfo_without_password_unchanged() {
        assert_eq!(
            mask_password("postgres://user@localhost/db"),
            "postgres://user@localhost/db"
        );
    }
}
