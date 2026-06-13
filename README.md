<div align="center">

# 🎮 cli-games

**A collection of polished terminal mini-games, in one binary.**

[![CI](https://github.com/furybee/cli-games/actions/workflows/ci.yml/badge.svg)](https://github.com/furybee/cli-games/actions/workflows/ci.yml)
[![Release](https://github.com/furybee/cli-games/actions/workflows/release.yml/badge.svg)](https://github.com/furybee/cli-games/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 2024](https://img.shields.io/badge/rust-2024-orange.svg)](https://www.rust-lang.org)

**35 games** — Snake · Tetris · 2048 · Minesweeper · Pong · Wordle · Chess · Sokoban ·
Breakout · Solitaire · and many more, all in your terminal, built on [ratatui](https://ratatui.rs).

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
| **Filter the menu** | `f` (or `/`) then type | `Esc` clears the filter |
| **In a game** | `q` / `Esc` back to menu | `Ctrl-C` quit |

> Controls are shown in-game. Most games also accept `w`/`a`/`s`/`d`.

## The games

All 35 games, launched from the menu or directly with `cli-games <id>`:

| Game | Description |
|------|-------------|
| **2048** | Slide tiles, merge equal numbers, reach 2048. |
| **anagram** | Unscramble the word before the timer runs out. |
| **blackjack** | Beat the dealer at 21: bet, hit, stand, double. |
| **boggle** | Trace adjacent letters to spell words against the clock. |
| **breakout** | Bounce a ball, smash every brick, don't drop it. |
| **chess** | Full legal chess versus a simple minimax AI. |
| **conway** | Conway's cellular automaton on a toroidal grid. |
| **dinorun** | Jump the cacti in an endless desert dash. |
| **flappy** | Tap to flap through the pipes. |
| **freecell** | Classic FreeCell solitaire: free cells, foundations, eight cascades. |
| **hangman** | Guess the word before the gallows fill up. |
| **lightsout** | Flip crosses of lights until the board goes dark. |
| **mastermind** | Crack the hidden 4-colour code in ten guesses. |
| **maze** | Find the exit of a freshly generated perfect maze. |
| **memory** | Flip cards two at a time and match the pairs. |
| **minesweeper** | Clear the field without detonating a mine. |
| **nonogram** | Solve the picture from row and column number clues. |
| **peg** | Jump pegs across the cross until one remains. |
| **pong** | Volley past a chasing CPU paddle — first to 7 wins. |
| **racer** | Dodge traffic, ramp up speed, don't crash. |
| **roguelike** | Explore a dungeon, fight monsters, reach the stairs. |
| **simon** | Repeat the growing colour sequence. |
| **slidepuzzle** | Slide the numbered tiles back into order. |
| **snake** | Eat, grow, don't bite yourself. |
| **sokoban** | Push every crate onto its target. |
| **solitaire** | Klondike patience — clear the tableau to the foundations. |
| **spaceinvaders** | Defend Earth from a descending alien armada. |
| **sudoku** | Fill the grid so every row, column, and box holds 1–9. |
| **tetris** | Stack falling tetrominoes and clear lines. |
| **tictactoe** | Outsmart an unbeatable AI — or settle for a draw. |
| **tron** | Light-cycle duel — out-survive the AI. |
| **typing** | Type the sentence — measure your WPM and accuracy. |
| **videopoker** | Jacks-or-Better: hold, draw, and chase the royal flush. |
| **wordle** | Guess the hidden five-letter word in six tries. |
| **yahtzee** | Roll five dice, fill the 13-category scorecard. |

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
