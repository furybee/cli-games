//! Core contract shared by every game in the workspace.
//!
//! A game crate depends *only* on this crate (plus `ratatui` for drawing).
//! It implements the [`Game`] trait and self-registers with [`register_game!`].
//! The launcher discovers everything through [`registered_games`] — no game ever
//! edits a shared file, which is what lets several be built in parallel.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::Rect;

// Re-exported so game crates don't need their own `crossterm` / `inventory`
// dependencies (and can't pick a mismatched version).
pub use inventory;
pub use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// What the runner should do after a call to [`Game::update`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// Keep the current game running.
    Stay,
    /// Quit the current game and return to the launcher menu.
    Exit,
}

/// Everything a game needs to know about the current tick.
///
/// Handed to [`Game::update`] each frame. Input is already collected for you;
/// timing is exposed as a delta so games can run at their own pace regardless
/// of the runner's poll rate (accumulate `dt` to drive movement).
pub struct GameContext {
    keys: Vec<KeyEvent>,
    /// Time elapsed since the previous update.
    pub dt: Duration,
    /// Time elapsed since the game started.
    pub elapsed: Duration,
}

impl GameContext {
    /// Build a context. Called by the runner, not by games.
    pub fn new(keys: Vec<KeyEvent>, dt: Duration, elapsed: Duration) -> Self {
        Self { keys, dt, elapsed }
    }

    /// Every key event received since the last update.
    pub fn keys(&self) -> &[KeyEvent] {
        &self.keys
    }

    /// `true` if `code` was pressed during this tick.
    pub fn pressed(&self, code: KeyCode) -> bool {
        self.keys
            .iter()
            .any(|k| k.kind == KeyEventKind::Press && k.code == code)
    }

    /// `true` if the user asked to force-quit (Ctrl+C). The runner handles this
    /// globally, but games may inspect it too.
    pub fn quit_requested(&self) -> bool {
        self.keys.iter().any(|k| {
            k.kind == KeyEventKind::Press
                && k.code == KeyCode::Char('c')
                && k.modifiers.contains(KeyModifiers::CONTROL)
        })
    }
}

/// The contract every game implements.
///
/// Convention: return [`Transition::Exit`] on `q` / `Esc` to go back to the menu.
pub trait Game {
    /// Construct a fresh instance. Used by the registry factory.
    fn new() -> Self
    where
        Self: Sized;

    /// Advance game state by one tick using the collected input.
    fn update(&mut self, ctx: &GameContext) -> Transition;

    /// Draw the current state into `area` (usually the full screen).
    fn render(&mut self, frame: &mut Frame, area: Rect);

    /// How often `update` + `render` run. Override for faster/slower games.
    fn tick_rate(&self) -> Duration {
        Duration::from_millis(50)
    }
}

/// A registered game, collected at link time via [`inventory`].
pub struct GameEntry {
    /// Stable identifier, used to launch directly (`cli-games <id>`).
    pub id: &'static str,
    /// Display name shown in the menu.
    pub name: &'static str,
    /// One-line description shown in the menu.
    pub description: &'static str,
    /// Whoever built the game.
    pub author: &'static str,
    /// Builds a boxed instance of the game.
    pub factory: fn() -> Box<dyn Game>,
}

inventory::collect!(GameEntry);

/// All games registered across the workspace, sorted by name.
pub fn registered_games() -> Vec<&'static GameEntry> {
    let mut games: Vec<&'static GameEntry> = inventory::iter::<GameEntry>.into_iter().collect();
    games.sort_by_key(|e| e.name);
    games
}

/// Register a [`Game`] implementor with the launcher.
///
/// Place this once in your game crate's `lib.rs`. It plugs the game into the
/// registry with no edits to any shared file.
///
/// ```ignore
/// register_game! {
///     Snake,
///     id: "snake",
///     name: "Snake",
///     description: "Eat, grow, don't bite yourself.",
///     author: "you",
/// }
/// ```
#[macro_export]
macro_rules! register_game {
    (
        $ty:ty,
        id: $id:expr,
        name: $name:expr,
        description: $desc:expr,
        author: $author:expr $(,)?
    ) => {
        $crate::inventory::submit! {
            $crate::GameEntry {
                id: $id,
                name: $name,
                description: $desc,
                author: $author,
                factory: || ::std::boxed::Box::new(<$ty as $crate::Game>::new()),
            }
        }
    };
}
