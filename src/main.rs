mod app;
mod config;
mod datasource;
mod theme;
mod ui;
mod value;

use anyhow::Result;
use app::App;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "db9s",
    about = "k9s-style TUI browser for Postgres, MongoDB, RabbitMQ and Redis"
)]
struct Args {
    /// One-off postgres connection url, added as a connection named "postgres"
    #[arg(long)]
    postgres: Option<String>,
    /// One-off mongo connection url, added as a connection named "mongo"
    #[arg(long)]
    mongo: Option<String>,
    /// One-off RabbitMQ management API url (e.g. http://guest:guest@localhost:15672),
    /// added as a connection named "rabbitmq"
    #[arg(long)]
    rabbitmq: Option<String>,
    /// One-off Redis url (redis:// or rediss://), added as a connection named "redis"
    #[arg(long)]
    redis: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let mut cfg = config::load().unwrap_or_default();

    if let Some(url) = args.postgres {
        cfg.connections.push(config::Connection {
            name: "postgres".into(),
            kind: config::Kind::Postgres,
            url,
        });
    }
    if let Some(url) = args.mongo {
        cfg.connections.push(config::Connection {
            name: "mongo".into(),
            kind: config::Kind::Mongo,
            url,
        });
    }
    if let Some(url) = args.rabbitmq {
        cfg.connections.push(config::Connection {
            name: "rabbitmq".into(),
            kind: config::Kind::RabbitMq,
            url,
        });
    }
    if let Some(url) = args.redis {
        cfg.connections.push(config::Connection {
            name: "redis".into(),
            kind: config::Kind::Redis,
            url,
        });
    }

    let app = App::new(cfg).await;
    run(app).await
}

async fn run(mut app: App) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;
        app.poll_connecting().await;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == crossterm::event::KeyEventKind::Press {
                    app.handle_key(key).await;
                }
            }
        }

        if let Some(req) = app.editor_request.take() {
            run_external_editor(terminal, app, req).await?;
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

/// Suspends the TUI, runs $EDITOR (falling back to vi) on the temp file, then
/// restores the TUI and feeds the result back into the app.
async fn run_external_editor(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    req: app::EditorRequest,
) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;

    let editor_cmd = app.editor_cmd.clone();
    let path = req.tmp_path.clone();
    let status = tokio::task::spawn_blocking(move || {
        std::process::Command::new(&editor_cmd).arg(&path).status()
    })
    .await?;

    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;
    terminal.clear()?;

    let status = match status {
        Ok(s) => s,
        Err(e) => {
            app.status = format!("failed to launch editor: {e}");
            let _ = std::fs::remove_file(&req.tmp_path);
            return Ok(());
        }
    };
    if !status.success() {
        app.status = "editor exited with an error, edit discarded".into();
        let _ = std::fs::remove_file(&req.tmp_path);
        return Ok(());
    }

    let content = std::fs::read_to_string(&req.tmp_path).unwrap_or_default();
    let _ = std::fs::remove_file(&req.tmp_path);
    app.finish_external_edit(&content, req.original);

    Ok(())
}
