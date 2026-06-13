# cli-games — guide for agents

A Rust workspace of terminal mini-games, built for **parallel development**: each
game is an isolated crate. When you build a game, stay inside your crate and touch
**nothing else** except the two registry lines below. This keeps concurrent work
on other games conflict-free.

## Architecture (read before editing)

```
crates/
  core/        game_core  — the Game trait, GameContext, registry. DO NOT EDIT.
  app/         cli-games  — launcher TUI (menu + frame loop). DO NOT EDIT.
  games/
    _registry/ games_all  — umbrella; the ONLY shared file you append to.
    snake/     game_snake — reference game / template. Read it, don't change it.
    <game>/    one crate per game — your work lives here.
```

The launcher discovers games at link time via the `inventory` crate, so a game
registers itself with the `register_game!` macro — there is no central list to edit.

## Your task: add one game

1. **Scaffold** — copy the template:
   ```bash
   cp -r crates/games/snake crates/games/<game>
   ```
   In `crates/games/<game>/Cargo.toml` set `name = "game_<game>"`. Leave the
   inherited workspace fields as-is.

2. **Implement** `game_core::Game` in `crates/games/<game>/src/lib.rs`:
   - `fn new() -> Self` — fresh state.
   - `fn update(&mut self, ctx: &GameContext) -> Transition` — advance one tick.
   - `fn render(&mut self, frame: &mut Frame, area: Rect)` — draw with ratatui.
   - optional `fn tick_rate(&self) -> Duration` (default 50 ms).
   - End the file with a `register_game! { Type, id: "...", name: "...", description: "...", author: "..." }`.

3. **Register** (the only shared edit — append alphabetically, never reorder):
   - `crates/games/_registry/Cargo.toml` → `[dependencies]`: `game_<game> = { path = "../<game>" }`
   - `crates/games/_registry/src/lib.rs`: `use game_<game> as _;`

Full walkthrough with code: `docs/ADD_A_GAME.md`.

## Conventions

- **Exit to menu** on `q` / `Esc` by returning `Transition::Exit`. The runner
  already handles Ctrl+C globally.
- **Timing**: accumulate `ctx.dt` to drive game speed. Never assume a fixed tick.
- **Input**: read it via `ctx.pressed(KeyCode::...)` / `ctx.keys()`. KeyCode is
  re-exported from `game_core`.
- **Dependencies**: a game depends only on `game_core` and `ratatui` (both
  `.workspace = true`). Don't add `crossterm`/`inventory` directly — they're
  re-exported by `game_core` so versions stay unified. Adding any other dep
  needs a `[workspace.dependencies]` entry; flag it rather than diverging.
- **No `unsafe`, no panics in the game loop.** Use `Result`-free logic inside
  `update`/`render`; handle bad state gracefully (the runner can't recover a panic).
- Keep the game self-contained: no global state, no reading/writing files unless
  asked. Match the style and comment density of `crates/games/snake/src/lib.rs`.

## Build & verify

```bash
cargo build -p game_<game>          # fast iteration on just your crate
cargo build                         # whole workspace must compile
cargo run -p cli-games -- <game>    # launch your game directly
cargo run -p cli-games              # menu — confirm your game appears
cargo clippy -p game_<game>         # keep it warning-clean
cargo fmt
```

Your game is done when: it builds clean, appears in the menu, plays, and returns
to the menu on `q`/`Esc`.

## Parallel workflow (for whoever orchestrates)

One git worktree + branch per game so builds and commits don't collide:
```bash
git worktree add ../cli-games-<game> -b game/<game>
```
Point each agent at this file and `docs/ADD_A_GAME.md`, scoped to its one game.
Merges stay conflict-free because every agent only adds files plus append-only
registry lines.
