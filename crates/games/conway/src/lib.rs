//! Game of Life — Conway's cellular automaton on a toroidal grid.
//!
//! Follows the same pattern as the Snake reference crate: state, input
//! handling, `dt`-based timing, a grid render, an overlay, and self-registration.
//!
//! The grid wraps at the edges (a torus), so gliders sail off one side and
//! reappear on the other. You edit with a cursor, then watch evolution unfold.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

const WIDTH: u16 = 32;
const HEIGHT: u16 = 20;

/// Speed presets, in milliseconds between generations. Index moves with `+`/`-`.
const SPEEDS: [u64; 6] = [400, 250, 150, 90, 50, 25];

pub struct Conway {
    /// Row-major occupancy, `WIDTH * HEIGHT` cells.
    cells: Vec<bool>,
    cursor: (u16, u16),
    running: bool,
    /// Index into [`SPEEDS`].
    speed: usize,
    accumulator: Duration,
    generation: u64,
    rng: u64,
}

impl Conway {
    fn idx(x: u16, y: u16) -> usize {
        y as usize * WIDTH as usize + x as usize
    }

    fn alive(&self, x: u16, y: u16) -> bool {
        self.cells.get(Self::idx(x, y)).copied().unwrap_or(false)
    }

    fn population(&self) -> usize {
        self.cells.iter().filter(|&&c| c).count()
    }

    fn step_ms(&self) -> u64 {
        SPEEDS.get(self.speed).copied().unwrap_or(150)
    }

    fn clear_grid(&mut self) {
        for c in &mut self.cells {
            *c = false;
        }
        self.generation = 0;
    }

    /// Advance one generation, treating the grid as a torus.
    fn step(&mut self) {
        let mut next = vec![false; self.cells.len()];
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let mut neighbours = 0u8;
                for dy in [HEIGHT - 1, 0, 1] {
                    for dx in [WIDTH - 1, 0, 1] {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = (x + dx) % WIDTH;
                        let ny = (y + dy) % HEIGHT;
                        if self.alive(nx, ny) {
                            neighbours += 1;
                        }
                    }
                }
                let here = self.alive(x, y);
                let lives = matches!((here, neighbours), (true, 2) | (true, 3) | (false, 3));
                if let Some(slot) = next.get_mut(Self::idx(x, y)) {
                    *slot = lives;
                }
            }
        }
        self.cells = next;
        self.generation += 1;
    }

    /// Stamp a glider with its top-left corner at the cursor (wrapping).
    fn load_glider(&mut self) {
        let pattern = [(1u16, 0u16), (2, 1), (0, 2), (1, 2), (2, 2)];
        let (cx, cy) = self.cursor;
        for (dx, dy) in pattern {
            let x = (cx + dx) % WIDTH;
            let y = (cy + dy) % HEIGHT;
            if let Some(slot) = self.cells.get_mut(Self::idx(x, y)) {
                *slot = true;
            }
        }
    }

    fn move_cursor(&mut self, dx: i16, dy: i16) {
        let nx = (self.cursor.0 as i16 + dx).rem_euclid(WIDTH as i16) as u16;
        let ny = (self.cursor.1 as i16 + dy).rem_euclid(HEIGHT as i16) as u16;
        self.cursor = (nx, ny);
    }

    fn toggle_cursor(&mut self) {
        let i = Self::idx(self.cursor.0, self.cursor.1);
        if let Some(slot) = self.cells.get_mut(i) {
            *slot = !*slot;
        }
    }

    /// Seed a scattering of live cells for an interesting starting screen.
    fn randomize(&mut self) {
        for c in &mut self.cells {
            *c = false;
        }
        let count = (WIDTH as u64 * HEIGHT as u64) / 4;
        for _ in 0..count {
            let x = (self.next_rand() % WIDTH as u64) as u16;
            let y = (self.next_rand() % HEIGHT as u64) as u16;
            if let Some(slot) = self.cells.get_mut(Self::idx(x, y)) {
                *slot = true;
            }
        }
        self.generation = 0;
    }

    /// xorshift64 — keeps the crate dependency-free.
    fn next_rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }
}

impl Game for Conway {
    fn new() -> Self {
        let mut game = Conway {
            cells: vec![false; WIDTH as usize * HEIGHT as usize],
            cursor: (WIDTH / 2, HEIGHT / 2),
            running: false,
            speed: 2,
            accumulator: Duration::ZERO,
            generation: 0,
            rng: seed(),
        };
        game.randomize();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        // Cursor movement (paused or running — editing on the fly is welcome).
        if ctx.pressed(KeyCode::Up) {
            self.move_cursor(0, -1);
        }
        if ctx.pressed(KeyCode::Down) {
            self.move_cursor(0, 1);
        }
        if ctx.pressed(KeyCode::Left) {
            self.move_cursor(-1, 0);
        }
        if ctx.pressed(KeyCode::Right) {
            self.move_cursor(1, 0);
        }

        if ctx.pressed(KeyCode::Char(' ')) {
            self.toggle_cursor();
        }

        // Play / pause.
        if ctx.pressed(KeyCode::Enter) || ctx.pressed(KeyCode::Char('p')) {
            self.running = !self.running;
            self.accumulator = Duration::ZERO;
        }

        // Single step (only meaningful while paused, but harmless otherwise).
        if ctx.pressed(KeyCode::Char('s')) {
            self.step();
        }

        if ctx.pressed(KeyCode::Char('c')) {
            self.clear_grid();
        }

        if ctx.pressed(KeyCode::Char('g')) {
            self.load_glider();
        }

        if ctx.pressed(KeyCode::Char('r')) {
            self.randomize();
        }

        // Speed control.
        if ctx.pressed(KeyCode::Char('+')) || ctx.pressed(KeyCode::Char('=')) {
            self.speed = (self.speed + 1).min(SPEEDS.len() - 1);
        }
        if ctx.pressed(KeyCode::Char('-')) || ctx.pressed(KeyCode::Char('_')) {
            self.speed = self.speed.saturating_sub(1);
        }

        // Real-time evolution: accumulate dt and step at the chosen cadence.
        if self.running {
            let period = Duration::from_millis(self.step_ms());
            self.accumulator += ctx.dt;
            while self.accumulator >= period {
                self.accumulator -= period;
                self.step();
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let state = if self.running { "running" } else { "paused" };
        let title = format!(
            " Game of Life  ·  gen {}  ·  pop {}  ·  {} ",
            self.generation,
            self.population(),
            state
        );
        let field = centered(WIDTH * 2 + 2, HEIGHT + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let mut lines = Vec::with_capacity(HEIGHT as usize);
        for y in 0..HEIGHT {
            let mut spans = Vec::with_capacity(WIDTH as usize);
            for x in 0..WIDTH {
                let is_cursor = (x, y) == self.cursor;
                let live = self.alive(x, y);
                let span = match (live, is_cursor) {
                    (true, true) => Span::styled("▓▓", Style::default().fg(Color::LightYellow)),
                    (true, false) => Span::styled("██", Style::default().fg(Color::LightGreen)),
                    (false, true) => Span::styled("[]", Style::default().fg(Color::Cyan)),
                    (false, false) => Span::raw("  "),
                };
                spans.push(span);
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        // Controls hint, drawn just below the field when there is room.
        let hint = " arrows: move · space: toggle · enter/p: play · s: step · +/-: speed · g: glider · c: clear · r: random · q: menu ";
        let hint_field = Rect {
            x: field.x,
            y: field.y.saturating_add(field.height),
            width: field.width,
            height: 1,
        };
        if hint_field.y < area.y.saturating_add(area.height) {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    hint,
                    Style::default().fg(Color::DarkGray),
                ))),
                hint_field,
            );
        }

        // Overlay when the colony has died out completely.
        if self.population() == 0 {
            let msg = " EXTINCT · space/g to seed life · r: random · q: menu ";
            let overlay = centered(msg.chars().count() as u16 + 2, 3, area);
            frame.render_widget(Clear, overlay);
            frame.render_widget(
                Paragraph::new(msg)
                    .block(Block::default().borders(Borders::ALL))
                    .style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                overlay,
            );
        }
    }

    fn tick_rate(&self) -> Duration {
        Duration::from_millis(30)
    }
}

fn seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x2545_F491_4F6C_DD1D)
        | 1
}

/// Centre a `w`×`h` rect inside `area`, clamped to its bounds.
fn centered(w: u16, h: u16, area: Rect) -> Rect {
    Rect {
        x: area.x + area.width.saturating_sub(w) / 2,
        y: area.y + area.height.saturating_sub(h) / 2,
        width: w.min(area.width),
        height: h.min(area.height),
    }
}

register_game! {
    Conway,
    id: "conway",
    name: "Game of Life",
    description: "Conway's cellular automaton on a toroidal grid.",
    author: "furybee",
}
