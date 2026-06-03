mod app;
mod cleanup;
mod input;
mod logging;
mod profile;
mod scanner;
mod size;
mod sources;
mod stats;
mod ui;

use std::{
    env,
    error::Error,
    io::{self, Stdout},
    time::Duration,
};

use app::App;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

type CrosstermTerminal = Terminal<CrosstermBackend<Stdout>>;

fn main() -> Result<(), Box<dyn Error>> {
    if env::args().any(|arg| arg == "-h" || arg == "--help") {
        print_help();
        return Ok(());
    }

    let mut app = App::new();
    let mut terminal = TerminalSession::enter()?;
    run_app(terminal.terminal_mut(), &mut app)?;
    Ok(())
}

fn print_help() {
    println!("Cleanroom");
    println!();
    println!("Safe Xcode cleanup preview and Trash-based cleanup for Friction Lab.");
    println!();
    println!("Usage:");
    println!("  clean");
    println!("  clean --help");
}

fn init_terminal() -> Result<CrosstermTerminal, Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut CrosstermTerminal) -> Result<(), Box<dyn Error>> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

struct TerminalSession {
    terminal: CrosstermTerminal,
}

impl TerminalSession {
    fn enter() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            terminal: init_terminal()?,
        })
    }

    fn terminal_mut(&mut self) -> &mut CrosstermTerminal {
        &mut self.terminal
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = restore_terminal(&mut self.terminal);
    }
}

fn run_app(terminal: &mut CrosstermTerminal, app: &mut App) -> Result<(), Box<dyn Error>> {
    loop {
        terminal.draw(|frame| ui::render(frame, app))?;

        if event::poll(Duration::from_millis(250))?
            && let Event::Key(key) = event::read()?
            && input::handle_key(app, key)
        {
            break;
        }
    }

    Ok(())
}
