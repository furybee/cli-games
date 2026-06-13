# cli-games

A collection of terminal mini-games, built as a Rust workspace designed for
**parallel development**: each game is an isolated crate that self-registers with
the launcher, so several games can be built at once without touching shared files.

## Run

```bash
cargo run -p cli-games               # interactive menu
cargo run -p cli-games -- snake      # launch a game directly by id
```

## Layout

```
crates/
  core/              game_core  — the Game trait, runtime context, registry
  app/               cli-games  — launcher TUI (menu + frame loop); never changes per game
  games/
    _registry/       games_all  — umbrella that links every game (2 append lines per game)
    snake/           game_snake — reference game / template
    <your-game>/     ...one crate per game
```

## How isolation works

- A game depends only on `game_core` (+ `ratatui`) and lives entirely in its own crate.
- It registers itself via the `register_game!` macro — no edit to any shared registry.
- The launcher discovers games at link time through the [`inventory`] crate.
- Adding a game = a new crate + two append-only lines in `crates/games/_registry`.

Add a game: see [`docs/ADD_A_GAME.md`](docs/ADD_A_GAME.md).

## Stack

ratatui · crossterm · clap · inventory · Rust 2024 edition.
