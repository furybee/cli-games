//! cli-games launcher.
//!
//! With no argument it shows an interactive menu of every registered game.
//! With an id (`cli-games snake`) it launches that game directly.

mod menu;
mod runner;

use anyhow::Result;
use clap::Parser;
use game_core::registered_games;

// Force-link the umbrella crate so every game's registration is compiled in.
// This is the single line of "wiring" the binary needs; it never changes.
use games_all as _;

#[derive(Parser)]
#[command(
    name = "cli-games",
    version,
    about = "A collection of terminal mini-games"
)]
struct Cli {
    /// Launch a specific game directly by its id (skips the menu).
    game: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let games = registered_games();

    // Validate before touching the terminal so errors stay readable.
    if let Some(id) = &cli.game
        && !games.iter().any(|e| e.id == id)
    {
        eprintln!("Unknown game '{id}'.");
        if games.is_empty() {
            eprintln!("No games are registered yet.");
        } else {
            eprintln!("Available games:");
            for e in &games {
                eprintln!("  {:<12} {}", e.id, e.description);
            }
        }
        std::process::exit(1);
    }

    let mut term = runner::setup_terminal()?;
    let result = run(&mut term, cli.game.as_deref());
    runner::restore_terminal(&mut term)?;
    result
}

fn run(term: &mut runner::Term, direct: Option<&str>) -> Result<()> {
    if let Some(id) = direct {
        let entry = registered_games()
            .into_iter()
            .find(|e| e.id == id)
            .expect("validated in main");
        let mut game = (entry.factory)();
        return runner::run_game(term, game.as_mut());
    }

    while let Some(entry) = menu::run_menu(term)? {
        let mut game = (entry.factory)();
        runner::run_game(term, game.as_mut())?;
    }
    Ok(())
}
