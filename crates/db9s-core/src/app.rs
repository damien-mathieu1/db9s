use crate::config::{Config, Kind};
use crate::datasource::{DataSource, QueryResult, ReadOnly, Sort};
use crate::input::Input;
use crate::key::{KeyCode, KeyEvent};
use crate::value::Row;
use anyhow::Result;
use std::collections::VecDeque;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

pub const PAGE_SIZE: i64 = 200;
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(6);
/// How often the open container's metrics are sampled, and how many samples are kept.
pub const METRICS_EVERY: Duration = Duration::from_secs(2);
pub const METRICS_KEPT: usize = 60;

/// One sample: (series name, value) per metric.
type Metrics = Vec<(String, f64)>;

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
    /// k9s-style `:` prompt to jump to a connection, database or container by name.
    Command,
    /// The current container's columns (`i`), in `App::schema`.
    Schema,
}

/// Inline, field-by-field edit of one row/document, saved in one update.
pub struct FieldEdit {
    pub original: Row,
    pub edited: Row,
    pub idx: usize,
    /// `Some` while the selected field's value is being typed.
    pub input: Option<Input>,
    /// A new row (`a`): saved with an insert, fields left null get the database default.
    pub insert: bool,
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
    ReadOnly,
}

pub struct ConnEntry {
    pub name: String,
    pub kind: Kind,
    pub url: String,
    /// As saved in the config; `--read-only` comes on top, see `App::force_read_only`.
    pub read_only: bool,
    /// Shared so a background task can use it while the UI keeps running.
    pub source: Result<Arc<dyn DataSource>>,
}

/// Masks the password portion of a connection string, e.g.
/// `postgres://user:secret@host/db` -> `postgres://user:***@host/db`.
const COMMANDS: [&str; 6] = ["connections", "ctx", "settings", "add", "quit", "q"];

/// Candidates that extend `typed` (case-insensitive), shortest first, for fish-style
/// completion. Nothing is suggested before the first character is typed.
pub fn complete(candidates: &[String], typed: &str) -> Vec<String> {
    if typed.is_empty() {
        return vec![];
    }
    let t = typed.to_lowercase();
    let mut out: Vec<String> = candidates
        .iter()
        .filter(|c| c.to_lowercase().starts_with(&t) && c.chars().count() > typed.chars().count())
        .cloned()
        .collect();
    out.sort_by(|a, b| a.len().cmp(&b.len()).then(a.cmp(b)));
    out.dedup();
    out
}

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
    /// Server-side order of the rows pane, set with `:sort <column> [desc]`.
    pub sort: Option<Sort>,
    /// Rows in the current container, when the backend counts them.
    pub total: Option<u64>,
    /// What `i` shows: one row per column of the current container.
    pub schema: Option<QueryResult>,
    /// Live series for the open container (`DataSource::metrics`), oldest sample first.
    pub metrics: Vec<(String, VecDeque<f64>)>,
    /// Which (database, container) `metrics` belongs to.
    metrics_for: Option<(String, String)>,
    sampler: Option<JoinHandle<Result<Metrics>>>,
    last_sample: Option<Instant>,
    /// The statement the open delete/save confirm would run, once the backend says.
    pub confirm_preview: Option<String>,
    history: crate::history::History,
    /// Which history entry ↑/↓ shows in the query prompt; None = what's being typed.
    history_idx: Option<usize>,

    pub query_input: Input,
    pub status: String,
    pub should_quit: bool,

    pub editor_request: Option<EditorRequest>,
    pub pending_edit: Option<(Row, Row)>,
    pub field_edit: Option<FieldEdit>,
    pub editor_cmd: String,
    pub settings_editor_input: Input,
    pub settings_error: Option<String>,
    /// Active theme name; the theme itself lives in `theme::get()`.
    pub theme_name: String,
    /// Settings popup: whether the theme selector (vs the editor field) is focused.
    pub settings_on_theme: bool,
    /// Theme shown while browsing themes in settings, and the one to restore on esc.
    pub settings_theme: String,
    settings_theme_before: String,
    pub command_input: Input,
    /// Which matching suggestion is shown, cycled with up/down.
    pub command_idx: usize,

    pub new_conn_name: Input,
    pub new_conn_url: Input,
    pub new_conn_kind: Kind,
    pub new_conn_field: AddField,
    pub new_conn_error: Option<String>,
    pub edit_target: Option<usize>,
    pub connecting: Option<PendingConnect>,
    /// The database call in flight, if any; its result is applied by `poll`.
    pending: Option<Pending>,
    pub new_conn_read_only: bool,
    /// `--read-only`: every connection of this session, without touching the config.
    pub force_read_only: bool,
}

async fn connect(kind: Kind, url: &str, read_only: bool) -> Result<Box<dyn DataSource>> {
    // outside the timeout: a secret manager may wait on a Touch ID / GUI prompt
    let url = &crate::secrets::resolve(url).await?;
    use crate::datasource::{
        amqp::AmqpSource, mongo::MongoSource, mysql::MySqlSource, postgres::PostgresSource,
        rabbitmq::RabbitMqSource, redis::RedisSource, sqlite::SqliteSource,
    };
    let fut = async {
        Ok(match kind {
            Kind::Postgres => {
                Box::new(PostgresSource::connect(url, read_only).await?) as Box<dyn DataSource>
            }
            Kind::Mongo => Box::new(MongoSource::connect(url).await?),
            Kind::RabbitMq if url.starts_with("amqp") => Box::new(AmqpSource::connect(url).await?),
            Kind::RabbitMq => Box::new(RabbitMqSource::connect(url).await?),
            Kind::Redis => Box::new(RedisSource::connect(url).await?),
            Kind::MySql => Box::new(MySqlSource::connect(url, read_only).await?),
            Kind::Sqlite => Box::new(SqliteSource::connect(url, read_only).await?),
        })
    };
    let source = tokio::time::timeout(CONNECT_TIMEOUT, fut)
        .await
        .unwrap_or_else(|_| {
            Err(anyhow::anyhow!(
                "connection timed out after {}s",
                CONNECT_TIMEOUT.as_secs()
            ))
        })?;
    Ok(if read_only {
        Box::new(ReadOnly(source))
    } else {
        source
    })
}

fn spawn_connect(
    kind: Kind,
    url: String,
    read_only: bool,
) -> JoinHandle<Result<Box<dyn DataSource>>> {
    tokio::spawn(async move { connect(kind, &url, read_only).await })
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

/// What a background database call produced, applied to the app by `poll`.
enum Done {
    Databases(Vec<String>),
    Containers(Vec<String>),
    /// Rows of the current container; `true` focuses the rows pane once they're in.
    /// ...and the container's total row count, when known.
    Rows(QueryResult, bool, Option<u64>),
    Query(QueryResult),
    Saved,
    Deleted,
    Inserted,
    Schema(QueryResult),
    /// The statement a pending confirm would run.
    Preview(Option<String>),
}

struct Pending {
    /// Writes are never aborted: a write cut mid-flight may or may not have applied.
    write: bool,
    handle: JoinHandle<Result<Done>>,
}

impl App {
    pub async fn new(cfg: Config, force_read_only: bool) -> Self {
        let editor_cmd = cfg
            .editor
            .clone()
            .or_else(|| std::env::var("EDITOR").ok())
            .unwrap_or_else(|| "vi".to_string());

        let theme_name = cfg.theme.clone().unwrap_or_else(|| "default".into());
        let theme_error = match crate::theme::load(&theme_name) {
            Ok(t) => {
                crate::theme::set(t);
                None
            }
            Err(e) => Some(format!("error: {e:#}; using the default theme")),
        };

        // connect concurrently so N unreachable servers cost one timeout, not N
        let futs = cfg.connections.into_iter().map(|c| async move {
            let source = connect(c.kind, &c.url, c.read_only || force_read_only)
                .await
                .map(Arc::from);
            ConnEntry {
                name: c.name,
                kind: c.kind,
                url: c.url,
                read_only: c.read_only,
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
            sort: None,
            total: None,
            schema: None,
            confirm_preview: None,
            metrics: vec![],
            metrics_for: None,
            sampler: None,
            last_sample: None,
            history: crate::history::load(),
            history_idx: None,
            query_input: Input::default(),
            status: theme_error.unwrap_or_default(),
            should_quit: false,
            editor_request: None,
            pending_edit: None,
            field_edit: None,
            editor_cmd: editor_cmd.clone(),
            settings_editor_input: Input::new(editor_cmd),
            settings_error: None,
            settings_on_theme: false,
            settings_theme: theme_name.clone(),
            settings_theme_before: theme_name.clone(),
            theme_name,
            command_input: Input::default(),
            command_idx: 0,
            new_conn_name: Input::default(),
            new_conn_url: Input::default(),
            new_conn_kind: Kind::Postgres,
            new_conn_field: AddField::Name,
            new_conn_error: None,
            edit_target: None,
            connecting: None,
            pending: None,
            new_conn_read_only: false,
            force_read_only,
        }
    }

    /// Applies whatever background work finished (a connection attempt, a database
    /// call); called once per event-loop tick so the UI never waits on the network.
    pub async fn poll(&mut self) {
        self.poll_connecting().await;
        self.sample_metrics().await;
        if !self
            .pending
            .as_ref()
            .is_some_and(|p| p.handle.is_finished())
        {
            return;
        }
        let p = self.pending.take().unwrap();
        match p.handle.await {
            Ok(Ok(done)) => self.apply(done),
            Ok(Err(e)) => self.status = format!("error: {e}"),
            Err(e) if e.is_cancelled() => {}
            Err(e) => self.status = format!("error: database task panicked: {e}"),
        }
    }

    /// Keeps `metrics` sampling the open container every `METRICS_EVERY`, starting
    /// over when another one opens. Runs beside `pending`, so it never cancels a read.
    async fn sample_metrics(&mut self) {
        if self.sampler.as_ref().is_some_and(|h| h.is_finished()) {
            if let Ok(Ok(values)) = self.sampler.take().unwrap().await {
                for (name, v) in values {
                    let series = match self.metrics.iter_mut().find(|(n, _)| *n == name) {
                        Some((_, s)) => s,
                        None => {
                            self.metrics.push((name, VecDeque::new()));
                            &mut self.metrics.last_mut().unwrap().1
                        }
                    };
                    series.push_back(v);
                    if series.len() > METRICS_KEPT {
                        series.pop_front();
                    }
                }
            }
        }
        let open = match (self.screen, self.current_db(), self.current_container()) {
            (Screen::Browse, Some(db), Some(c)) => Some((db.to_string(), c.to_string())),
            _ => None,
        };
        if open != self.metrics_for {
            if let Some(h) = self.sampler.take() {
                h.abort();
            }
            self.metrics.clear();
            self.last_sample = None;
            self.metrics_for = open.clone();
        }
        let Some((db, container)) = open else {
            return;
        };
        let due = self
            .last_sample
            .is_none_or(|t| t.elapsed() >= METRICS_EVERY);
        if self.sampler.is_some() || !due {
            return;
        }
        let Some(source) = self
            .conns
            .get(self.conn_idx)
            .and_then(|c| c.source.as_ref().ok().cloned())
        else {
            return;
        };
        self.last_sample = Some(Instant::now());
        self.sampler = Some(tokio::spawn(async move {
            source.metrics(&db, &container).await
        }));
    }

    /// Whether something is still running in the background (to keep polling).
    pub fn is_busy(&self) -> bool {
        self.pending.is_some() || self.connecting.is_some()
    }

    fn apply(&mut self, done: Done) {
        match done {
            Done::Databases(dbs) => {
                self.databases = dbs;
                self.db_idx = 0;
                self.screen = Screen::Databases;
                self.status = String::new();
            }
            Done::Containers(c) => {
                self.containers = c;
                self.container_idx = 0;
                self.result = None;
                self.sort = None;
                self.screen = Screen::Browse;
                self.browse_focus = BrowseFocus::Containers;
                self.status = String::new();
                self.load_rows();
            }
            Done::Rows(res, focus_rows, total) => {
                self.row_idx = 0;
                self.total = total;
                self.status = self.rows_status(res.rows.len());
                self.result = Some(res);
                if focus_rows {
                    self.browse_focus = BrowseFocus::Rows;
                }
            }
            Done::Query(res) => {
                self.row_idx = 0;
                self.status = match res.affected {
                    Some(_) if self.is_queue() => "message published".into(),
                    Some(n) => format!("{n} rows affected"),
                    None => format!("{} rows", res.rows.len()),
                };
                self.result = Some(res);
            }
            Done::Saved => {
                self.status = "saved".into();
                self.load_rows();
            }
            Done::Deleted => {
                self.status = "deleted".into();
                self.load_rows();
            }
            Done::Inserted => {
                self.status = "inserted".into();
                self.load_rows();
            }
            Done::Preview(sql) => {
                if matches!(self.mode, Mode::ConfirmDelete | Mode::ConfirmEditSave) {
                    self.confirm_preview = sql;
                }
            }
            Done::Schema(res) => {
                self.status = String::new();
                // only if nothing else took the screen while it loaded
                if matches!(self.mode, Mode::Normal) {
                    self.schema = Some(res);
                    self.mode = Mode::Schema;
                }
            }
        }
    }

    /// Runs `op` on the current connection in the background. A new call replaces a
    /// read still in flight; while a write runs, it is refused instead.
    fn start<F>(&mut self, status: &str, write: bool, op: impl FnOnce(Arc<dyn DataSource>) -> F)
    where
        F: Future<Output = Result<Done>> + Send + 'static,
    {
        let Some(source) = self
            .conns
            .get(self.conn_idx)
            .and_then(|c| c.source.as_ref().ok().cloned())
        else {
            return;
        };
        if let Some(p) = &self.pending {
            if p.write {
                self.status = "busy: a write is still running".into();
                return;
            }
            p.handle.abort();
        }
        self.status = status.into();
        self.pending = Some(Pending {
            write,
            handle: tokio::spawn(op(source)),
        });
    }

    /// Aborts a read in flight; true if there was one.
    fn cancel_read(&mut self) -> bool {
        if !self.pending.as_ref().is_some_and(|p| !p.write) {
            return false;
        }
        self.pending.take().unwrap().handle.abort();
        self.status = "cancelled".into();
        true
    }

    async fn poll_connecting(&mut self) {
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
        let read_only = self.new_conn_read_only;
        if let Some(idx) = edit_idx {
            let old_name = self.conns[idx].name.clone();
            if old_name != name {
                let _ = crate::config::remove_connection(&old_name);
            }
            if let Err(e) = crate::config::add_connection(crate::config::Connection {
                name: name.clone(),
                kind,
                url: url.clone(),
                read_only,
            }) {
                self.status = format!("connected, but failed to save to config: {e}");
            } else {
                self.status = format!("updated '{name}'");
            }
            self.conns[idx] = ConnEntry {
                name,
                kind,
                url,
                read_only,
                source: Ok(Arc::from(source)),
            };
            self.conn_idx = idx;
        } else {
            if let Err(e) = crate::config::add_connection(crate::config::Connection {
                name: name.clone(),
                kind,
                url: url.clone(),
                read_only,
            }) {
                self.status = format!("connected, but failed to save to config: {e}");
            } else {
                self.status = format!("connected and saved '{name}'");
            }
            self.conns.push(ConnEntry {
                name,
                kind,
                url,
                read_only,
                source: Ok(Arc::from(source)),
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
        if key.code == KeyCode::Esc && matches!(self.mode, Mode::Normal) && self.cancel_read() {
            return;
        }
        match self.mode {
            Mode::Normal => self.handle_normal_key(key).await,
            Mode::Query => self.handle_query_key(key).await,
            Mode::ConfirmEditSave => self.handle_confirm_edit_save_key(key).await,
            Mode::FieldEdit => self.handle_field_edit_key(key).await,
            Mode::ConfirmDelete => self.handle_confirm_key(key).await,
            Mode::AddConnection => self.handle_add_conn_key(key).await,
            Mode::ConfirmDeleteConn => self.handle_confirm_delete_conn_key(key).await,
            Mode::Settings => self.handle_settings_key(key),
            Mode::Command => self.handle_command_key(key).await,
            Mode::Schema => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q' | 'i')
                ) {
                    self.schema = None;
                    self.mode = Mode::Normal;
                }
            }
        }
    }

    async fn handle_normal_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('c') if key.ctrl => {
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
                self.preview_container();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                self.preview_container();
            }
            KeyCode::Enter => self.enter(),
            KeyCode::Char('r') if self.rows_focused() => self.load_rows(),
            KeyCode::Char('n') if self.rows_focused() => {
                self.offset += PAGE_SIZE;
                self.load_rows();
            }
            KeyCode::Char('p') if self.rows_focused() => {
                self.offset = (self.offset - PAGE_SIZE).max(0);
                self.load_rows();
            }
            KeyCode::Char('/') if self.rows_focused() && self.is_queue() && self.is_read_only() => {
                self.status = "read-only connection: publishing is off".into();
            }
            KeyCode::Char('/') if self.rows_focused() => {
                self.mode = Mode::Query;
                self.query_input = Input::default();
                self.history_idx = None;
            }
            KeyCode::Char('e' | 'E' | 'd' | 'a') if self.rows_focused() && self.is_read_only() => {
                self.status = "read-only connection: insert, edit and delete are off".into();
            }
            KeyCode::Char('a') if self.rows_focused() && self.is_queue() => {
                self.status = "use / to publish a message".into();
            }
            KeyCode::Char('a') if self.rows_focused() => self.start_insert(),
            KeyCode::Char('i') if self.screen == Screen::Browse && !self.is_queue() => {
                self.load_schema()
            }
            KeyCode::Char('e') | KeyCode::Char('E') if self.rows_focused() && self.is_queue() => {
                self.status = "rabbitmq messages are read-only".into();
            }
            KeyCode::Char('e') if self.rows_focused() => self.start_field_edit(),
            KeyCode::Char('E') if self.rows_focused() => self.start_edit(),
            KeyCode::Char('d') if self.rows_focused() && self.selected_row().is_some() => {
                self.mode = Mode::ConfirmDelete;
                let row = self.selected_row().unwrap().clone();
                self.preview(move |s, db, c| async move { s.preview_delete(&db, &c, &row).await });
            }
            KeyCode::Char(':') => {
                self.command_input = Input::default();
                self.command_idx = 0;
                self.mode = Mode::Command;
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
        if matches!(self.mode, Mode::Command) {
            return "enter run  tab/→ complete  ↑/↓ suggestions  esc cancel";
        }
        if matches!(self.mode, Mode::Schema) {
            return "esc/i close";
        }
        if matches!(self.mode, Mode::Settings) {
            return "tab switch_field  ←/→ preview_theme  enter save  esc cancel";
        }
        if let Some(fe) = &self.field_edit {
            return if fe.input.is_some() {
                "←/→ move  enter apply  esc cancel  42/true/null/{..} keep_type  \"..\" forces_text"
            } else {
                "j/k move  enter edit field  ctrl+s save  esc discard"
            };
        }
        match self.screen {
            Screen::Connections => "j/k move  enter open  a add  e edit  d delete  s settings  : command  q quit",
            Screen::Databases => "j/k move  enter open  : command  esc back",
            Screen::Browse => match self.browse_focus {
                BrowseFocus::Containers if self.is_queue() => {
                    "j/k move+preview  enter/tab rows  : command  esc back"
                }
                BrowseFocus::Containers => {
                    "j/k move+preview  enter/tab rows  i schema  : command  esc back"
                }
                BrowseFocus::Rows if self.is_read_only() && self.is_queue() => {
                    "j/k move  n/p page  r refresh  tab queues  esc back"
                }
                BrowseFocus::Rows if self.is_read_only() => {
                    "j/k move  / query  i schema  n/p page  r refresh  tab containers  esc back"
                }
                BrowseFocus::Rows if self.is_queue() => {
                    "j/k move  / publish  d purge queue  n/p page  r refresh  tab queues  esc back"
                }
                BrowseFocus::Rows => {
                    "j/k move  e edit  E $EDITOR  a insert  d delete  / query  i schema  n/p page  r refresh  tab containers  esc back"
                }
            },
        }
    }

    /// Everything `:` can jump to from here: built-ins, connections, and the
    /// databases / containers already listed for the current connection.
    fn command_candidates(&self) -> Vec<String> {
        let mut out: Vec<String> = COMMANDS.iter().map(|c| c.to_string()).collect();
        out.extend(self.conns.iter().map(|c| c.name.clone()));
        if self.screen != Screen::Connections {
            out.extend(self.databases.iter().cloned());
        }
        if self.screen == Screen::Browse {
            out.extend(self.containers.iter().cloned());
            if let Some(res) = &self.result {
                out.extend(["export csv".into(), "export json".into()]);
                out.push("sort".into());
                for c in &res.columns {
                    out.push(format!("sort {c}"));
                    out.push(format!("sort {c} desc"));
                }
            }
        }
        out.extend(
            crate::theme::names()
                .into_iter()
                .map(|t| format!("theme {t}")),
        );
        out
    }

    /// Loads, applies and optionally saves a theme; errors go to the status line.
    fn apply_theme(&mut self, name: &str, save: bool) -> bool {
        match crate::theme::load(name) {
            Ok(t) => {
                crate::theme::set(t);
                if save {
                    self.theme_name = name.to_string();
                    self.status = match crate::config::set_theme(name) {
                        Ok(_) => format!("theme: {name}"),
                        Err(e) => format!("error: theme applied but not saved: {e}"),
                    };
                }
                true
            }
            Err(e) => {
                self.status = format!("error: {e:#}");
                false
            }
        }
    }

    /// Every candidate extending what's typed, best first; `command_idx` picks one.
    pub fn command_matches(&self) -> Vec<String> {
        complete(&self.command_candidates(), self.command_input.value())
    }

    /// The full candidate currently suggested for what's typed, if any.
    fn command_choice(&self) -> Option<String> {
        let matches = self.command_matches();
        (!matches.is_empty()).then(|| matches[self.command_idx % matches.len()].clone())
    }

    /// The not-yet-typed rest of the suggestion, shown greyed after the input.
    pub fn command_suggestion(&self) -> Option<String> {
        let typed = self.command_input.value().chars().count();
        self.command_choice()
            .map(|full| full.chars().skip(typed).collect())
    }

    async fn handle_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                let cmd = self.command_input.value().trim().to_string();
                self.mode = Mode::Normal;
                self.run_command(&cmd).await;
            }
            KeyCode::Tab | KeyCode::Right if self.command_choice().is_some() => {
                self.command_input = Input::new(self.command_choice().unwrap());
                self.command_idx = 0;
            }
            // down = next, so the GUI's top-down suggestion list moves the way the arrow points
            KeyCode::Down => self.command_idx += 1,
            KeyCode::Up => self.command_idx = self.command_idx.saturating_sub(1),
            _ => {
                self.command_input.handle(key);
                self.command_idx = 0;
            }
        }
    }

    async fn run_command(&mut self, cmd: &str) {
        if let Some(format) = cmd.strip_prefix("export ") {
            self.status = match (&self.result, self.current_container()) {
                (Some(res), Some(name)) => match crate::export::write(res, format.trim(), name) {
                    Ok(path) => format!("exported {} rows to {}", res.rows.len(), path.display()),
                    Err(e) => format!("error: {e:#}"),
                },
                _ => "error: open a table first".into(),
            };
            return;
        }
        if cmd == "sort" || cmd.starts_with("sort ") {
            self.sort_command(&cmd[4..]);
            return;
        }
        if cmd == "theme" {
            self.status = format!(
                "theme: {} (available: {})",
                self.theme_name,
                crate::theme::names().join(", ")
            );
            return;
        }
        // re-reads the file each time, so editing a theme then `:theme <name>` reloads it
        if let Some(name) = cmd.strip_prefix("theme ") {
            self.apply_theme(name.trim(), true);
            return;
        }
        let find = |names: &[String]| names.iter().position(|n| n.eq_ignore_ascii_case(cmd));
        match cmd.to_lowercase().as_str() {
            "" => {}
            "q" | "q!" | "quit" => self.should_quit = true,
            "connections" | "conn" | "ctx" => {
                self.screen = Screen::Connections;
                self.browse_focus = BrowseFocus::Containers;
            }
            "settings" => self.start_settings(),
            "add" => self.start_add_conn(),
            _ => {
                if let Some(i) = (self.screen == Screen::Browse)
                    .then(|| find(&self.containers))
                    .flatten()
                {
                    self.container_idx = i;
                    self.browse_focus = BrowseFocus::Containers;
                    self.offset = 0;
                    self.sort = None;
                    self.fetch_rows(true);
                } else if let Some(i) = (self.screen != Screen::Connections)
                    .then(|| find(&self.databases))
                    .flatten()
                {
                    self.db_idx = i;
                    self.screen = Screen::Databases;
                    self.enter();
                } else if let Some(i) = self
                    .conns
                    .iter()
                    .position(|c| c.name.eq_ignore_ascii_case(cmd))
                {
                    self.conn_idx = i;
                    self.screen = Screen::Connections;
                    self.enter();
                } else {
                    self.status = format!("error: unknown command '{cmd}'");
                }
            }
        }
    }

    // shortcut: one fetch per keypress, no debounce; add one if holding j/k on many tables gets slow
    fn preview_container(&mut self) {
        if self.screen == Screen::Browse && self.browse_focus == BrowseFocus::Containers {
            // another container, other columns: its own natural order
            self.offset = 0;
            self.sort = None;
            self.load_rows();
        }
    }

    fn selected_row(&self) -> Option<&Row> {
        self.result.as_ref().and_then(|r| r.rows.get(self.row_idx))
    }

    /// Queues (RabbitMQ) get publish/purge instead of edit/delete.
    pub fn is_queue(&self) -> bool {
        self.current_source().is_some_and(|s| s.is_queue())
    }

    /// Whether what's on screen is read-only, for the header badge and the keys.
    pub fn is_read_only(&self) -> bool {
        self.force_read_only
            || (self.screen != Screen::Connections
                && self.conns.get(self.conn_idx).is_some_and(|c| c.read_only))
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
        // a late result would otherwise jump back to the screen just left
        self.cancel_read();
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
                    self.sort = None;
                    self.total = None;
                    self.row_idx = 0;
                    self.screen = Screen::Databases;
                }
            },
        }
    }

    fn enter(&mut self) {
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
                self.start("loading databases...", false, |s| async move {
                    s.list_databases().await.map(Done::Databases)
                });
            }
            Screen::Databases => {
                let db = match self.current_db() {
                    Some(d) => d.to_string(),
                    None => return,
                };
                self.start("loading containers...", false, |s| async move {
                    s.list_containers(&db).await.map(Done::Containers)
                });
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

    fn load_rows(&mut self) {
        self.fetch_rows(false);
    }

    fn fetch_rows(&mut self, focus_rows: bool) {
        let db = match self.current_db() {
            Some(d) => d.to_string(),
            None => return,
        };
        let container = match self.current_container() {
            Some(c) => c.to_string(),
            None => return,
        };
        let (offset, sort) = (self.offset, self.sort.clone());
        self.start("loading rows...", false, move |s| async move {
            let res = match &sort {
                Some(o) => {
                    s.fetch_sorted(&db, &container, PAGE_SIZE, offset, o)
                        .await?
                }
                None => s.fetch_rows(&db, &container, PAGE_SIZE, offset).await?,
            };
            // a failed count only hides paging, it doesn't fail the page
            let total = s.count_rows(&db, &container).await.ok().flatten();
            Ok(Done::Rows(res, focus_rows, total))
        });
    }

    /// `200 rows (offset 0)`, plus `of 1234, page 1/7` and the sort when known.
    fn rows_status(&self, shown: usize) -> String {
        let mut s = format!("{shown} rows (offset {})", self.offset);
        if let Some(total) = self.total {
            let pages = total.div_ceil(PAGE_SIZE as u64).max(1);
            let page = self.offset as u64 / PAGE_SIZE as u64 + 1;
            s += &format!(" of {total}, page {page}/{pages}");
        }
        if let Some(o) = &self.sort {
            s += &format!(
                ", sorted by {} {}",
                o.column,
                if o.desc { "↓" } else { "↑" }
            );
        }
        s
    }

    /// Reorders the rows pane server-side and reloads it from the first page; None
    /// back to the natural order. Used by `:sort` and the GUI's header clicks.
    pub fn set_sort(&mut self, sort: Option<Sort>) {
        if self.screen != Screen::Browse || self.result.is_none() {
            self.status = "error: open a table first".into();
            return;
        }
        if sort.is_some() && !self.current_source().is_some_and(|s| s.can_sort()) {
            self.status = "error: this backend can't sort".into();
            return;
        }
        self.sort = sort;
        self.offset = 0;
        self.load_rows();
    }

    /// `:sort`, `:sort <column>`, `:sort <column> desc`.
    fn sort_command(&mut self, args: &str) {
        let mut words = args.split_whitespace();
        let Some(column) = words.next() else {
            return self.set_sort(None);
        };
        let desc = match words.next() {
            None | Some("asc") => false,
            Some("desc") => true,
            Some(other) => {
                self.status = format!("error: sort direction '{other}' is not asc or desc");
                return;
            }
        };
        let known = self
            .result
            .as_ref()
            .is_some_and(|r| r.columns.iter().any(|c| c == column));
        if !known {
            self.status = format!("error: no column '{column}' here");
            return;
        }
        self.set_sort(Some(Sort {
            column: column.to_string(),
            desc,
        }));
    }

    async fn handle_query_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
            }
            KeyCode::Up | KeyCode::Down => self.recall(key.code == KeyCode::Up),
            KeyCode::Enter => {
                let db = self.current_db().unwrap_or_default().to_string();
                let container = self.current_container().unwrap_or_default().to_string();
                let raw = self.query_input.value().to_string();
                self.mode = Mode::Normal;
                if let Some(conn) = self.conns.get(self.conn_idx) {
                    let queries = self.history.entry(conn.name.clone()).or_default();
                    crate::history::push(queries, &raw);
                    // losing history isn't worth interrupting the query for
                    let _ = crate::history::save(&self.history);
                }
                // a raw query may write, so it's never aborted
                self.start("running query...", true, move |s| async move {
                    s.run_query(&db, &container, &raw).await.map(Done::Query)
                });
            }
            _ => {
                self.query_input.handle(key);
            }
        }
    }

    /// ↑ older / ↓ newer query of this connection into the prompt; past the newest,
    /// back to an empty prompt.
    fn recall(&mut self, older: bool) {
        let Some(queries) = self
            .conns
            .get(self.conn_idx)
            .and_then(|c| self.history.get(&c.name))
            .filter(|q| !q.is_empty())
        else {
            return;
        };
        self.history_idx = match (self.history_idx, older) {
            (None, true) => Some(queries.len() - 1),
            (Some(i), true) => Some(i.saturating_sub(1)),
            (Some(i), false) if i + 1 < queries.len() => Some(i + 1),
            (_, false) => None,
        };
        self.query_input = Input::new(self.history_idx.map_or("", |i| &queries[i]));
    }

    fn save_row(&mut self, original: &Row, edited: &Row) {
        let db = self.current_db().unwrap_or_default().to_string();
        let container = self.current_container().unwrap_or_default().to_string();
        let (original, edited) = (original.clone(), edited.clone());
        self.start("saving...", true, move |s| async move {
            s.update_row(&db, &container, &original, &edited)
                .await
                .map(|_| Done::Saved)
        });
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
            insert: false,
        });
        self.mode = Mode::FieldEdit;
    }

    /// A blank row with the page's columns, edited like any row and inserted on save.
    fn start_insert(&mut self) {
        let Some(res) = &self.result else {
            return;
        };
        let blank: Row = res
            .columns
            .iter()
            .map(|c| (c.clone(), serde_json::Value::Null))
            .collect();
        if blank.is_empty() {
            self.status = "error: no columns to fill in (empty table?), use / to insert".into();
            return;
        }
        self.field_edit = Some(FieldEdit {
            original: blank.clone(),
            edited: blank,
            idx: 0,
            input: None,
            insert: true,
        });
        self.mode = Mode::FieldEdit;
    }

    fn insert_row(&mut self, row: Row) {
        let db = self.current_db().unwrap_or_default().to_string();
        let container = self.current_container().unwrap_or_default().to_string();
        // null = not set: let the database fill ids and defaults
        let row: Row = row.into_iter().filter(|(_, v)| !v.is_null()).collect();
        self.start("inserting...", true, move |s| async move {
            s.insert_row(&db, &container, &row)
                .await
                .map(|_| Done::Inserted)
        });
    }

    /// Asks the backend, in the background, what the open confirm would run.
    fn preview<F>(
        &mut self,
        op: impl FnOnce(Arc<dyn DataSource>, String, String) -> F + Send + 'static,
    ) where
        F: Future<Output = Result<Option<String>>> + Send + 'static,
    {
        self.confirm_preview = None;
        let db = self.current_db().unwrap_or_default().to_string();
        let container = self.current_container().unwrap_or_default().to_string();
        let status = std::mem::take(&mut self.status);
        self.start(&status, false, move |s| async move {
            // a preview that fails (no primary key...) just isn't shown
            Ok(Done::Preview(op(s, db, container).await.ok().flatten()))
        });
    }

    fn load_schema(&mut self) {
        let db = self.current_db().unwrap_or_default().to_string();
        let Some(container) = self.current_container().map(str::to_string) else {
            return;
        };
        self.start("loading schema...", false, move |s| async move {
            s.describe(&db, &container).await.map(Done::Schema)
        });
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
                    // untouched text keeps its original type (string "42" stays a string);
                    // on a new row, a field left empty stays null (= database default)
                    if input.value() != before && !(fe.insert && input.value().is_empty()) {
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
                let v = &fe.edited[&field];
                // a new row's unset field starts empty, not as the text "null"
                let text = if fe.insert && v.is_null() {
                    String::new()
                } else {
                    crate::value::value_to_input(v)
                };
                fe.input = Some(Input::new(text))
            }
            KeyCode::Char('s') if key.ctrl => {
                let fe = self.field_edit.take().unwrap();
                self.mode = Mode::Normal;
                if fe.edited == fe.original {
                    self.status = "no changes".into();
                } else if fe.insert {
                    self.insert_row(fe.edited);
                } else {
                    self.save_row(&fe.original, &fe.edited);
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
        self.pending_edit = Some((original.clone(), edited.clone()));
        self.mode = Mode::ConfirmEditSave;
        self.preview(
            move |s, db, c| async move { s.preview_update(&db, &c, &original, &edited).await },
        );
    }

    async fn handle_confirm_edit_save_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') => {
                self.mode = Mode::Normal;
                if let Some((original, edited)) = self.pending_edit.take() {
                    self.save_row(&original, &edited);
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
                self.start("deleting...", true, move |s| async move {
                    s.delete_row(&db, &container, &row)
                        .await
                        .map(|_| Done::Deleted)
                });
            }
            _ => self.mode = Mode::Normal,
        }
    }

    fn start_settings(&mut self) {
        self.settings_editor_input = Input::new(self.editor_cmd.clone());
        self.settings_error = None;
        self.settings_on_theme = false;
        self.settings_theme = self.theme_name.clone();
        self.settings_theme_before = self.theme_name.clone();
        self.mode = Mode::Settings;
    }

    fn handle_settings_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                // undo the live preview
                let before = self.settings_theme_before.clone();
                self.apply_theme(&before, false);
                self.mode = Mode::Normal;
            }
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                self.settings_on_theme = !self.settings_on_theme;
            }
            KeyCode::Left | KeyCode::Right if self.settings_on_theme => {
                let names = crate::theme::names();
                let i = names
                    .iter()
                    .position(|n| *n == self.settings_theme)
                    .unwrap_or(0);
                let next = if key.code == KeyCode::Right {
                    (i + 1) % names.len()
                } else {
                    (i + names.len() - 1) % names.len()
                };
                self.settings_theme = names[next].clone();
                let name = self.settings_theme.clone();
                self.settings_error =
                    (!self.apply_theme(&name, false)).then(|| std::mem::take(&mut self.status));
            }
            KeyCode::Enter => {
                let value = self.settings_editor_input.value().trim().to_string();
                if value.is_empty() {
                    self.settings_error = Some("editor command cannot be empty".into());
                    return;
                }
                let saved = crate::config::set_editor(&value)
                    .and_then(|_| crate::config::set_theme(&self.settings_theme));
                match saved {
                    Ok(_) => {
                        self.editor_cmd = value;
                        self.theme_name = self.settings_theme.clone();
                        self.status = "settings saved".into();
                        self.mode = Mode::Normal;
                    }
                    Err(e) => self.settings_error = Some(format!("failed to save: {e}")),
                }
            }
            _ if !self.settings_on_theme => {
                self.settings_editor_input.handle(key);
            }
            _ => {}
        }
    }

    fn start_add_conn(&mut self) {
        self.new_conn_name = Input::default();
        self.new_conn_url = Input::default();
        self.new_conn_kind = Kind::Postgres;
        self.new_conn_read_only = false;
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
        self.new_conn_read_only = entry.read_only;
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
            KeyCode::Char('t') if key.ctrl => self.start_test_conn(),
            KeyCode::Tab | KeyCode::Down => {
                self.new_conn_field = match self.new_conn_field {
                    AddField::Name => AddField::Url,
                    AddField::Url => AddField::Kind,
                    AddField::Kind => AddField::ReadOnly,
                    AddField::ReadOnly => AddField::Name,
                };
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.new_conn_field = match self.new_conn_field {
                    AddField::Name => AddField::ReadOnly,
                    AddField::Url => AddField::Name,
                    AddField::Kind => AddField::Url,
                    AddField::ReadOnly => AddField::Kind,
                };
            }
            KeyCode::Left | KeyCode::Right if self.new_conn_field == AddField::Kind => {
                self.new_conn_kind = self.new_conn_kind.next();
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                if self.new_conn_field == AddField::ReadOnly =>
            {
                self.new_conn_read_only = !self.new_conn_read_only;
            }
            KeyCode::Enter => match self.new_conn_field {
                AddField::Name => self.new_conn_field = AddField::Url,
                AddField::Url => self.new_conn_field = AddField::Kind,
                AddField::Kind | AddField::ReadOnly => self.start_submit_conn(),
            },
            _ => match self.new_conn_field {
                AddField::Name => {
                    self.new_conn_name.handle(key);
                }
                AddField::Url => {
                    self.new_conn_url.handle(key);
                }
                AddField::Kind | AddField::ReadOnly => {}
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
            handle: spawn_connect(kind, url, self.new_conn_read_only || self.force_read_only),
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
            handle: spawn_connect(kind, url, self.new_conn_read_only || self.force_read_only),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{complete, mask_password};
    use super::{App, BrowseFocus, ConnEntry, Mode, Screen};
    use crate::config::Kind;
    use crate::datasource::{DataSource, QueryResult};
    use crate::key::{KeyCode, KeyEvent};
    use crate::value::Row;
    use anyhow::Result;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// Answers after `delay`; records whether a delete went through and what was inserted.
    struct Slow {
        delay: Duration,
        deleted: Arc<AtomicBool>,
        inserted: Arc<std::sync::Mutex<Vec<Row>>>,
    }

    #[async_trait::async_trait]
    impl DataSource for Slow {
        async fn list_databases(&self) -> Result<Vec<String>> {
            tokio::time::sleep(self.delay).await;
            Ok(vec!["db".into()])
        }
        async fn list_containers(&self, _: &str) -> Result<Vec<String>> {
            Ok(vec!["t".into()])
        }
        async fn fetch_rows(&self, _: &str, _: &str, _: i64, _: i64) -> Result<QueryResult> {
            Ok(QueryResult {
                columns: vec!["id".into(), "name".into()],
                rows: vec![serde_json::from_str(r#"{"id": 1, "name": "a"}"#).unwrap()],
                affected: None,
            })
        }
        async fn run_query(&self, _: &str, _: &str, _: &str) -> Result<QueryResult> {
            unimplemented!()
        }
        fn can_sort(&self) -> bool {
            true
        }
        async fn count_rows(&self, _: &str, _: &str) -> Result<Option<u64>> {
            Ok(Some(450))
        }
        async fn delete_row(&self, _: &str, _: &str, _: &Row) -> Result<()> {
            tokio::time::sleep(self.delay).await;
            self.deleted.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn update_row(&self, _: &str, _: &str, _: &Row, _: &Row) -> Result<()> {
            Ok(())
        }
        async fn insert_row(&self, _: &str, _: &str, row: &Row) -> Result<()> {
            self.inserted.lock().unwrap().push(row.clone());
            Ok(())
        }
        async fn preview_delete(&self, _: &str, c: &str, _: &Row) -> Result<Option<String>> {
            Ok(Some(format!("DELETE FROM {c} WHERE id = 1")))
        }
        async fn metrics(&self, _: &str, _: &str) -> Result<Vec<(String, f64)>> {
            Ok(vec![("depth".into(), 3.)])
        }
        async fn describe(&self, _: &str, _: &str) -> Result<QueryResult> {
            Ok(QueryResult {
                columns: vec!["column".into()],
                rows: vec![serde_json::from_str(r#"{"column": "id"}"#).unwrap()],
                affected: None,
            })
        }
    }

    thread_local! {
        /// What the current test's `Slow` source inserted.
        static INSERTED: Arc<std::sync::Mutex<Vec<Row>>> = Arc::default();
    }

    async fn app_with(delay: Duration) -> (App, Arc<AtomicBool>) {
        let mut app = App::new(Default::default(), false).await;
        let deleted = Arc::new(AtomicBool::new(false));
        app.conns.push(ConnEntry {
            name: "slow".into(),
            kind: Kind::Postgres,
            url: String::new(),
            read_only: false,
            source: Ok(Arc::new(Slow {
                delay,
                deleted: deleted.clone(),
                inserted: INSERTED.with(|i| i.clone()),
            })),
        });
        app.mode = Mode::Normal;
        (app, deleted)
    }

    async fn press(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code)).await;
    }

    async fn settle(app: &mut App) {
        while app.is_busy() {
            tokio::time::sleep(Duration::from_millis(5)).await;
            app.poll().await;
        }
    }

    #[tokio::test]
    async fn database_calls_run_in_the_background() {
        let (mut app, _) = app_with(Duration::from_millis(50)).await;
        press(&mut app, KeyCode::Enter).await;
        // handle_key came back before the server answered
        assert!(app.is_busy());
        assert_eq!(app.screen, Screen::Connections);
        assert_eq!(app.status, "loading databases...");
        settle(&mut app).await;
        assert_eq!(app.screen, Screen::Databases);
        assert_eq!(app.databases, ["db"]);
        // containers then the first container's rows chain on their own
        press(&mut app, KeyCode::Enter).await;
        settle(&mut app).await;
        assert_eq!(app.screen, Screen::Browse);
        assert_eq!(app.result.as_ref().map(|r| r.rows.len()), Some(1));
    }

    #[tokio::test]
    async fn esc_cancels_a_slow_read() {
        let (mut app, _) = app_with(Duration::from_secs(30)).await;
        press(&mut app, KeyCode::Enter).await;
        press(&mut app, KeyCode::Esc).await;
        assert!(!app.is_busy());
        assert_eq!(app.status, "cancelled");
        assert_eq!(app.screen, Screen::Connections);
    }

    #[tokio::test]
    async fn a_write_is_never_aborted() {
        let (mut app, deleted) = app_with(Duration::from_millis(50)).await;
        press(&mut app, KeyCode::Enter).await;
        settle(&mut app).await;
        press(&mut app, KeyCode::Enter).await;
        settle(&mut app).await;
        app.browse_focus = BrowseFocus::Rows;
        press(&mut app, KeyCode::Char('d')).await;
        press(&mut app, KeyCode::Char('y')).await;
        assert_eq!(app.status, "deleting...");
        // neither esc nor a new read stops it
        press(&mut app, KeyCode::Esc).await;
        press(&mut app, KeyCode::Char('j')).await;
        assert!(app.is_busy());
        settle(&mut app).await;
        assert!(deleted.load(Ordering::SeqCst));
        assert_eq!(app.status, "1 rows (offset 0) of 450, page 1/3");
    }

    async fn command(app: &mut App, text: &str) {
        press(app, KeyCode::Char(':')).await;
        for c in text.chars() {
            press(app, KeyCode::Char(c)).await;
        }
        press(app, KeyCode::Enter).await;
        settle(app).await;
    }

    async fn open_table(app: &mut App) {
        press(app, KeyCode::Enter).await;
        settle(app).await;
        press(app, KeyCode::Enter).await;
        settle(app).await;
        app.browse_focus = BrowseFocus::Rows;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_inserts_only_the_fields_that_were_set() {
        let (mut app, _) = app_with(Duration::ZERO).await;
        open_table(&mut app).await;
        press(&mut app, KeyCode::Char('a')).await;
        assert!(app.field_edit.as_ref().is_some_and(|fe| fe.insert));
        for k in [KeyCode::Enter, KeyCode::Char('7'), KeyCode::Enter] {
            press(&mut app, k).await;
        }
        app.handle_key(KeyEvent::ctrl(KeyCode::Char('s'))).await;
        settle(&mut app).await;
        let inserted = INSERTED.with(|i| i.lock().unwrap().clone());
        assert_eq!(
            inserted,
            [serde_json::from_str::<Row>(r#"{"id": 7}"#).unwrap()]
        );
        assert!(app.status.starts_with("1 rows"), "reloaded: {}", app.status);
    }

    #[tokio::test]
    async fn delete_confirm_shows_the_statement() {
        let (mut app, _) = app_with(Duration::ZERO).await;
        open_table(&mut app).await;
        press(&mut app, KeyCode::Char('d')).await;
        settle(&mut app).await;
        assert!(matches!(app.mode, Mode::ConfirmDelete));
        assert_eq!(
            app.confirm_preview.as_deref(),
            Some("DELETE FROM t WHERE id = 1")
        );
        press(&mut app, KeyCode::Char('n')).await;
        assert!(matches!(app.mode, Mode::Normal));
    }

    #[tokio::test]
    async fn up_and_down_walk_the_query_history() {
        let (mut app, _) = app_with(Duration::ZERO).await;
        open_table(&mut app).await;
        app.history
            .insert("slow".into(), vec!["select 1".into(), "select 2".into()]);
        press(&mut app, KeyCode::Char('/')).await;
        let shown = |app: &App| app.query_input.value().to_string();
        press(&mut app, KeyCode::Up).await;
        assert_eq!(shown(&app), "select 2");
        press(&mut app, KeyCode::Up).await;
        press(&mut app, KeyCode::Up).await;
        assert_eq!(shown(&app), "select 1");
        press(&mut app, KeyCode::Down).await;
        assert_eq!(shown(&app), "select 2");
        press(&mut app, KeyCode::Down).await;
        assert_eq!(shown(&app), "");
    }

    #[tokio::test]
    async fn metrics_are_sampled_for_the_open_container() {
        let (mut app, _) = app_with(Duration::ZERO).await;
        open_table(&mut app).await;
        for _ in 0..20 {
            app.poll().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        // sampled once (then every METRICS_EVERY), as a named series
        assert_eq!(app.metrics.len(), 1);
        assert_eq!(app.metrics[0].0, "depth");
        assert_eq!(app.metrics[0].1, [3.]);
        // another screen: series dropped
        press(&mut app, KeyCode::Esc).await;
        press(&mut app, KeyCode::Esc).await;
        app.poll().await;
        assert!(app.metrics.is_empty());
    }

    #[tokio::test]
    async fn i_shows_the_schema_and_esc_closes_it() {
        let (mut app, _) = app_with(Duration::ZERO).await;
        open_table(&mut app).await;
        press(&mut app, KeyCode::Char('i')).await;
        settle(&mut app).await;
        assert!(matches!(app.mode, Mode::Schema));
        assert_eq!(app.schema.as_ref().unwrap().rows.len(), 1);
        press(&mut app, KeyCode::Esc).await;
        assert!(matches!(app.mode, Mode::Normal));
        assert!(app.schema.is_none());
    }

    #[tokio::test]
    async fn sort_command_orders_rows_server_side() {
        let (mut app, _) = app_with(Duration::ZERO).await;
        press(&mut app, KeyCode::Enter).await;
        settle(&mut app).await;
        press(&mut app, KeyCode::Enter).await;
        settle(&mut app).await;
        press(&mut app, KeyCode::Char(':')).await;
        press(&mut app, KeyCode::Char('s')).await;
        assert!(app.command_matches().contains(&"sort id desc".to_string()));
        press(&mut app, KeyCode::Esc).await;

        command(&mut app, "sort id desc").await;
        assert_eq!(
            app.sort.as_ref().map(|s| (s.column.as_str(), s.desc)),
            Some(("id", true))
        );
        assert_eq!(
            app.status,
            "1 rows (offset 0) of 450, page 1/3, sorted by id ↓"
        );
        command(&mut app, "sort nope").await;
        assert_eq!(app.status, "error: no column 'nope' here");
        command(&mut app, "sort id sideways").await;
        assert!(app.status.contains("not asc or desc"));
        command(&mut app, "sort").await;
        assert_eq!(app.sort, None);
    }

    #[test]
    fn completes_fish_style() {
        let c: Vec<String> = ["users", "user_roles", "orders", "Users", "u"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(complete(&c, "us"), vec!["Users", "users", "user_roles"]);
        assert_eq!(complete(&c, "user_"), vec!["user_roles"]);
        assert!(complete(&c, "users").is_empty());
        assert!(complete(&c, "").is_empty());
        assert!(complete(&c, "zzz").is_empty());
    }

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
