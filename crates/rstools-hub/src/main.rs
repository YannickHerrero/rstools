mod app;

use std::io;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use rstools_core::db;
use rstools_http::HttpTool;
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

    // Build the app
    let mut app = App::new(vec![Box::new(todo), Box::new(http)]);
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
    loop {
        terminal.draw(|frame| {
            app.render(frame);
        })?;

        if app.should_quit {
            return Ok(());
        }

        // Block until an event arrives
        let ev = event::read()?;
        app.handle_event(ev);
    }
}
