//! Terminal setup/teardown and the per-game frame loop.

use std::collections::HashMap;
use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use anyhow::Result;
use game_core::{Game, GameContext, KeyCode, KeyEventKind, Transition};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

pub type Term = Terminal<CrosstermBackend<Stdout>>;

/// Enter raw mode + alternate screen and hand back a ready terminal.
pub fn setup_terminal() -> Result<Term> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    drain_startup_input()?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

/// Some terminals emit an escape sequence right after we switch to raw mode
/// (query responses, focus-in events, …). Discard anything that arrives in a
/// short window so it isn't misread as a keypress — otherwise a stray `Esc`
/// can instantly quit a menu or game that treats `Esc` as "back".
fn drain_startup_input() -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(150);
    while Instant::now() < deadline {
        if event::poll(Duration::from_millis(15))? {
            let _ = event::read()?;
        }
    }
    Ok(())
}

/// Restore the terminal to its original state. Always call this, even on error.
pub fn restore_terminal(term: &mut Term) -> Result<()> {
    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    term.show_cursor()?;
    Ok(())
}

/// A key stays "held" for this long after its last press/repeat. Long enough to
/// bridge the terminal's auto-repeat gap (so holding feels continuous), short
/// enough that a single tap only nudges.
const HELD_WINDOW: Duration = Duration::from_millis(150);

/// Drive a single game until it returns [`Transition::Exit`] (or Ctrl+C).
pub fn run_game(term: &mut Term, game: &mut dyn Game) -> Result<()> {
    let tick = game.tick_rate();
    let start = Instant::now();
    let mut last = Instant::now();
    // Last time each key was seen down, used to synthesise a "held" state since
    // terminals never send key-release events.
    let mut last_press: HashMap<KeyCode, Instant> = HashMap::new();

    loop {
        term.draw(|f| {
            let area = f.area();
            game.render(f, area);
        })?;

        // Collect every key event that arrives within this tick.
        let mut keys = Vec::new();
        if event::poll(tick)? {
            while event::poll(Duration::ZERO)? {
                if let Event::Key(key) = event::read()? {
                    keys.push(key);
                }
            }
        }

        let now = Instant::now();
        let dt = now.duration_since(last);
        last = now;

        // Refresh held-key timestamps, then expire anything past the window.
        for key in &keys {
            if key.kind == KeyEventKind::Press {
                last_press.insert(key.code, now);
            }
        }
        last_press.retain(|_, t| now.duration_since(*t) < HELD_WINDOW);
        let held: Vec<KeyCode> = last_press.keys().copied().collect();

        let ctx = GameContext::new(keys, held, dt, start.elapsed());
        if ctx.quit_requested() {
            break;
        }
        if game.update(&ctx) == Transition::Exit {
            break;
        }
    }
    Ok(())
}
