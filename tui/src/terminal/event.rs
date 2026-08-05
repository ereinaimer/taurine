use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, KeyEventKind};

pub(crate) enum Event {
    Key(KeyEvent),
    Tick,
}

pub(crate) struct EventHandler {
    tick_rate: Duration,
    last_tick: Instant,
}

impl EventHandler {
    pub(crate) fn new(tick_rate: Duration) -> Self {
        Self {
            tick_rate,
            last_tick: Instant::now(),
        }
    }

    pub(crate) fn next(&mut self) -> io::Result<Event> {
        let timeout = self.tick_rate.saturating_sub(self.last_tick.elapsed());

        if event::poll(timeout)? {
            match event::read()? {
                CrosstermEvent::Key(key)
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                {
                    return Ok(Event::Key(key));
                }
                _ => {
                    self.last_tick = Instant::now();
                    return Ok(Event::Tick);
                }
            }
        }

        self.last_tick = Instant::now();
        Ok(Event::Tick)
    }
}
