mod app;
mod demo_seed;

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use rstools_core::db;
use rstools_http::HttpTool;
use rstools_keepass::KeePassTool;
use rstools_notes::NotesTool;
use rstools_todo::TodoTool;

use app::App;

fn main() -> Result<()> {
    let demo_mode = std::env::args().any(|arg| arg == "--demo");

    // Open the shared database
    let conn = if demo_mode {
        let demo_path = demo_db_path()?;
        db::open_db_at(&demo_path)?
    } else {
        db::open_db()?
    };

    if demo_mode {
        demo_seed::seed_demo_data(&conn)?;
    }

    // Create tools
    // Each tool gets its own connection to avoid borrow issues
    let todo_conn = if demo_mode {
        db::open_db_at(&demo_db_path()?)?
    } else {
        db::open_db()?
    };
    let todo = TodoTool::new(todo_conn)?;

    let http_conn = if demo_mode {
        db::open_db_at(&demo_db_path()?)?
    } else {
        db::open_db()?
    };
    let http = HttpTool::new(http_conn)?;

    let keepass_conn = if demo_mode {
        db::open_db_at(&demo_db_path()?)?
    } else {
        db::open_db()?
    };
    let keepass = KeePassTool::new(keepass_conn)?;

    let notes_conn = if demo_mode {
        db::open_db_at(&demo_db_path()?)?
    } else {
        db::open_db()?
    };
    let notes = NotesTool::new(notes_conn)?;

    // Build the app
    let mut app = App::new(vec![
        Box::new(todo),
        Box::new(http),
        Box::new(keepass),
        Box::new(notes),
    ]);
    app.init_db(&conn)?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Main event loop
    let result = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        eprintln!("Error: {err:?}");
    }

    Ok(())
}

fn demo_db_path() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let demo_dir = cwd.join(".demo");
    std::fs::create_dir_all(&demo_dir)
        .with_context(|| format!("Failed to create demo directory: {}", demo_dir.display()))?;
    Ok(demo_dir.join("rstools-demo.db"))
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    const TICK_RATE: Duration = Duration::from_millis(50);

    loop {
        terminal.draw(|frame| {
            app.render(frame);
        })?;

        if app.should_quit {
            return Ok(());
        }

        // Poll with timeout so we can tick tools for async operations
        if event::poll(TICK_RATE)? {
            let ev = event::read()?;
            app.handle_event(ev);
        }

        // Tick the active tool (spinner animations, async polling, etc.)
        app.tick();
    }
}
