mod app;

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use rstools_core::db;
use rstools_http::HttpTool;
use rstools_keepass::KeePassTool;
use rstools_todo::TodoTool;

use app::App;

fn main() -> Result<()> {
    // Open the shared database
    let conn = db::open_db()?;

    // Create tools
    // Each tool gets its own connection to avoid borrow issues
    let todo_conn = db::open_db()?;
    let todo = TodoTool::new(todo_conn)?;

    let http_conn = db::open_db()?;
    let http = HttpTool::new(http_conn)?;

    let keepass_conn = db::open_db()?;
    let keepass = KeePassTool::new(keepass_conn)?;

    // Build the app
    let mut app = App::new(vec![Box::new(todo), Box::new(http), Box::new(keepass)]);
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
