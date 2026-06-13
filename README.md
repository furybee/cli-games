<div align="center">

# 🎮 cli-games

**A collection of polished terminal mini-games, in one binary.**

[![CI](https://github.com/furybee/cli-games/actions/workflows/ci.yml/badge.svg)](https://github.com/furybee/cli-games/actions/workflows/ci.yml)
[![Release](https://github.com/furybee/cli-games/actions/workflows/release.yml/badge.svg)](https://github.com/furybee/cli-games/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 2024](https://img.shields.io/badge/rust-2024-orange.svg)](https://www.rust-lang.org)

Snake · Tetris · 2048 · Minesweeper · Pong · Wordle · Sudoku · Solitaire · and more —
all running in your terminal, built on [ratatui](https://ratatui.rs).

</div>

---

## Install

### Homebrew

```bash
brew install furybee/homebrew-tap/cli-games
```

### Shell installer (macOS & Linux)

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/furybee/cli-games/releases/latest/download/cli-games-installer.sh | sh
```

### From source

```bash
git clone https://github.com/furybee/cli-games
cd cli-games
cargo install --path crates/app
```

## Play

```bash
cli-games            # open the menu and pick a game
cli-games snake      # launch a game directly by id
```

| | | |
|---|---|---|
| **Navigate the menu** | `↑` / `↓` (or `j` / `k`) | `Enter` to play |
| **In a game** | `q` / `Esc` back to menu | `Ctrl-C` quit |

> Controls are shown in-game. Most games also accept `w`/`a`/`s`/`d`.

## The games

| Game | Description |
|------|-------------|
| 🐍 **snake** | Eat, grow, don't bite yourself. |
| 🟦 **tetris** | Stack falling tetrominoes and clear lines. |
| 🔢 **2048** | Slide tiles, merge equal numbers, reach 2048. |
| 💣 **minesweeper** | Clear the field without detonating a mine. |
| 🏓 **pong** | Volley past a chasing CPU paddle — first to 7 wins. |
| 🟩 **wordle** | Guess the hidden five-letter word in six tries. |
| 🔡 **hangman** | Guess the word before the gallows fill up. |
| 🃏 **solitaire** | Klondike patience — clear the tableau to the foundations. |
| 🧩 **sudoku** | Fill the grid so every row, column, and box holds 1–9. |
| 🧠 **memory** | Flip cards two at a time and match the pairs. |
| ⭕ **tictactoe** | Outsmart an unbeatable AI — or settle for a draw. |
| 🦖 **dinorun** | Jump the cacti in an endless desert dash. |
| 🐤 **flappy** | Tap to flap through the pipes. |

## Architecture

`cli-games` is a Cargo workspace built so **many games can be developed in parallel
without ever colliding** — each game is a self-contained crate that registers
itself with the launcher.

```
crates/
  core/        game_core  — the Game trait, runtime context, registry
  app/         cli-games  — launcher TUI (menu + frame loop)
  games/
    _registry/ games_all  — links every game so it's discovered at runtime
    snake/     game_snake — reference game / template
    <game>/    one crate per game
```

- A game depends only on `game_core` (+ `ratatui`) and implements one small trait.
- It self-registers with the `register_game!` macro — there's **no central list to edit**.
- The launcher discovers games at link time via [`inventory`](https://docs.rs/inventory).
- Adding a game means a new crate plus two append-only lines — so parallel work
  merges without conflicts.

## Contributing a game

See **[docs/ADD_A_GAME.md](docs/ADD_A_GAME.md)** for the full walkthrough, and
**[CLAUDE.md](CLAUDE.md)** for the conventions. In short:

```bash
cp -r crates/games/snake crates/games/<game>   # start from the template
# implement the Game trait, then register it
cargo run -p cli-games -- <game>                # try it
```

Spin up isolated sessions for several games at once:

```bash
./scripts/spawn-games.sh tetris 2048 minesweeper   # prints a `claude --worktree` command per game
```

## Development

```bash
cargo run -p cli-games        # run the launcher
cargo build                   # build everything
cargo test --workspace        # run tests
cargo clippy --workspace      # lint (CI runs this with -D warnings)
cargo fmt --all               # format
```

CI runs fmt, clippy (`-D warnings`), build, and tests on every push and PR.

## License

[MIT](LICENSE) © furybee
