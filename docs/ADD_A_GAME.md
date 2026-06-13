# Adding a game

Each game is its own crate. Building one touches **only** your new directory plus
**two append-only lines** in the umbrella crate — so several games can be built in
parallel with no merge conflicts.

## 1. Create your crate

Copy the Snake crate as a starting point (it's a complete, minimal example):

```bash
cp -r crates/games/snake crates/games/<your-game>
```

Then edit `crates/games/<your-game>/Cargo.toml`:

```toml
[package]
name = "game_<your-game>"   # must start with `game_`
# ...everything else stays as-is (inherited from the workspace)
```

## 2. Implement the `Game` trait

In `crates/games/<your-game>/src/lib.rs`, implement [`game_core::Game`]:

```rust
use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::{Frame, layout::Rect};

pub struct MyGame { /* state */ }

impl Game for MyGame {
    fn new() -> Self { MyGame { /* ... */ } }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;          // back to the menu
        }
        // advance state using ctx.dt for timing
        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        // draw with ratatui widgets
    }

    // optional: fn tick_rate(&self) -> Duration { ... }
}

register_game! {
    MyGame,
    id: "<your-game>",
    name: "My Game",
    description: "One line shown in the menu.",
    author: "you",
}
```

Conventions:
- Return `Transition::Exit` on `q` / `Esc` to return to the launcher menu.
- Drive timing with `ctx.dt` (accumulate it) rather than assuming a fixed rate.

## 3. Register it with the launcher (the only shared edit)

Append, **alphabetically**, one line to each:

`crates/games/_registry/Cargo.toml` → `[dependencies]`:
```toml
game_<your-game> = { path = "../<your-game>" }
```

`crates/games/_registry/src/lib.rs`:
```rust
use game_<your_game> as _;
```

That's it — the workspace auto-discovers the crate (members glob `crates/games/*`),
the menu picks it up at runtime, and `cli-games <your-game>` launches it directly.

## 4. Run

```bash
cargo run -p cli-games               # menu
cargo run -p cli-games -- <your-game>  # launch directly
cargo build -p game_<your-game>      # build just your game while iterating
```
