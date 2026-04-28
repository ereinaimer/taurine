// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.

mod app;
mod event;
mod status;
mod ui;

use std::io;
use std::time::{Duration, Instant};

use app::App;
use crossterm::{
    cursor::Show,
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use event::{Event, EventHandler};
use ratatui::{Terminal, backend::CrosstermBackend};

const EVENT_TICK_RATE: Duration = Duration::from_millis(250);
const STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

pub fn run() -> taurine_core::Result<()> {
    let mut app = App::default();
    app.set_daemon_status(status::probe_daemon_status());

    let mut terminal = TerminalGuard::new()?;
    let mut events = EventHandler::new(EVENT_TICK_RATE);
    let mut last_status_refresh = Instant::now();

    loop {
        terminal.terminal.draw(|frame| ui::render(frame, &app))?;

        match events.next()? {
            Event::Key(key) => app.handle_key_event(key),
            Event::Tick => {
                if last_status_refresh.elapsed() >= STATUS_REFRESH_INTERVAL {
                    app.set_daemon_status(status::probe_daemon_status());
                    last_status_refresh = Instant::now();
                }
            }
        }

        if app.should_quit() {
            break;
        }
    }

    Ok(())
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;

        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            restore_terminal();
            return Err(error);
        }

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(error) => {
                restore_terminal();
                return Err(error);
            }
        };

        if let Err(error) = terminal.hide_cursor() {
            restore_terminal();
            return Err(error);
        }

        if let Err(error) = terminal.clear() {
            restore_terminal();
            return Err(error);
        }

        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
        let _ = execute!(self.terminal.backend_mut(), Show);
    }
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
}
