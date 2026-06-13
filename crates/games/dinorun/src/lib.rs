//! Dino Run — an endless terminal runner in the spirit of Chrome's offline dino.
//!
//! A dino jogs in place on the left while cacti scroll in from the right at an
//! ever-increasing pace. Tap Space / Up / W to jump; clip a cactus and it's over.
//! Mirrors the Snake template: float physics driven by `ctx.dt`, a grid render,
//! a game-over overlay, and self-registration.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Playfield size, in cells. Each cell renders two chars wide for a square look.
const WIDTH: u16 = 50;
const HEIGHT: u16 = 14;
/// Row the ground sits on; everything stands with its base here.
const GROUND_ROW: u16 = HEIGHT - 1;
/// The dino holds this column and the next one (it's two cells wide / tall).
const DINO_X: u16 = 6;

/// Vertical physics, in cells and cells/second. Tuned so a jump peaks around
/// 4.4 cells (clears the tallest cactus) with ~0.77s of airtime.
const GRAVITY: f32 = 60.0;
const JUMP_V: f32 = 23.0;

/// Scroll speed ramps from `BASE_SPEED` up to `MAX_SPEED` over time.
const BASE_SPEED: f32 = 16.0;
const MAX_SPEED: f32 = 40.0;
const ACCEL: f32 = 1.0; // cells/second added per second survived

/// A cactus: a single column, 1–2 cells tall, drifting left.
struct Obstacle {
    x: f32,
    height: u16,
}

pub struct DinoRun {
    /// Feet height above the ground, in cells (0 = standing).
    dino_y: f32,
    dino_vy: f32,
    on_ground: bool,
    obstacles: Vec<Obstacle>,
    /// Seconds until the next cactus spawns.
    spawn_timer: f32,
    /// Distance scrolled, doubles as the score (in "metres").
    distance: f32,
    /// Run time, drives the speed ramp.
    run_time: f32,
    dead: bool,
    best: u32,
    rng: u64,
}

impl Game for DinoRun {
    fn new() -> Self {
        let mut game = DinoRun {
            dino_y: 0.0,
            dino_vy: 0.0,
            on_ground: true,
            obstacles: Vec::new(),
            spawn_timer: 0.0,
            distance: 0.0,
            run_time: 0.0,
            dead: false,
            best: 0,
            rng: seed(),
        };
        game.restart();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.dead {
            if ctx.pressed(KeyCode::Enter) {
                self.restart();
            }
            return Transition::Stay;
        }

        let jump = ctx.pressed(KeyCode::Char(' '))
            || ctx.pressed(KeyCode::Up)
            || ctx.pressed(KeyCode::Char('w'));
        if jump && self.on_ground {
            self.dino_vy = JUMP_V;
            self.on_ground = false;
        }

        let dt = ctx.dt.as_secs_f32();
        self.run_time += dt;
        let speed = (BASE_SPEED + self.run_time * ACCEL).min(MAX_SPEED);
        self.distance += speed * dt;

        // Vertical integration; clamp back onto the ground.
        if !self.on_ground {
            self.dino_y += self.dino_vy * dt;
            self.dino_vy -= GRAVITY * dt;
            if self.dino_y <= 0.0 {
                self.dino_y = 0.0;
                self.dino_vy = 0.0;
                self.on_ground = true;
            }
        }

        // Scroll cacti left; drop any that have left the field.
        for o in &mut self.obstacles {
            o.x -= speed * dt;
        }
        self.obstacles.retain(|o| o.x > -2.0);

        // Spawn on a time gap so spacing stays fair as the speed climbs.
        self.spawn_timer -= dt;
        if self.spawn_timer <= 0.0 {
            let height = if self.rand_frac() < 0.4 { 2 } else { 1 };
            self.obstacles.push(Obstacle {
                x: WIDTH as f32,
                height,
            });
            // 1.2–2.4s between cacti — always longer than a jump's airtime.
            self.spawn_timer = 1.2 + self.rand_frac() * 1.2;
        }

        if self.collides() {
            self.dead = true;
            self.best = self.best.max(self.distance as u32);
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let score = self.distance as u32;
        let title = format!(" Dino Run  ·  {score:04}  ·  best {:04} ", self.best);
        let field = centered(WIDTH * 2 + 2, HEIGHT + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        // Dino's occupied cells: two wide, two tall, lifted by the jump offset.
        let offset = self.dino_y.round() as u16;
        let dino_base = GROUND_ROW.saturating_sub(offset);
        let dino_top = dino_base.saturating_sub(1);

        let mut lines = Vec::with_capacity(HEIGHT as usize);
        for y in 0..HEIGHT {
            let mut spans = Vec::with_capacity(WIDTH as usize);
            for x in 0..WIDTH {
                let span = if (x == DINO_X || x == DINO_X + 1) && (y == dino_base || y == dino_top)
                {
                    // Lighter top row reads as the head, darker bottom as the body.
                    let shade = if y == dino_top {
                        Color::White
                    } else {
                        Color::Gray
                    };
                    Span::styled("██", Style::default().fg(shade))
                } else if self.obstacle_at(x, y) {
                    Span::styled("██", Style::default().fg(Color::Green))
                } else if y == GROUND_ROW {
                    Span::styled("──", Style::default().fg(Color::DarkGray))
                } else {
                    Span::raw("  ")
                };
                spans.push(span);
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        if self.dead {
            let msg = format!(" GAME OVER · {score:04} · Enter: replay · q: menu ");
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

impl DinoRun {
    /// Reset the run while keeping the session's best score.
    fn restart(&mut self) {
        self.dino_y = 0.0;
        self.dino_vy = 0.0;
        self.on_ground = true;
        self.obstacles.clear();
        // A little breathing room before the first cactus.
        self.spawn_timer = 1.5;
        self.distance = 0.0;
        self.run_time = 0.0;
        self.dead = false;
    }

    /// Does the dino's footprint overlap any cactus? Both share the ground
    /// baseline, so the dino is clear whenever its feet are above the cactus top.
    fn collides(&self) -> bool {
        let dino_lo = DINO_X as f32;
        let dino_hi = (DINO_X + 2) as f32; // exclusive right edge
        self.obstacles.iter().any(|o| {
            let x_overlap = o.x < dino_hi && o.x + 1.0 > dino_lo;
            x_overlap && self.dino_y < o.height as f32
        })
    }

    /// Whether a cactus paints cell `(x, y)` — a single column, `height` tall.
    fn obstacle_at(&self, x: u16, y: u16) -> bool {
        self.obstacles.iter().any(|o| {
            let col = o.x.round();
            col >= 0.0
                && col as u16 == x
                && y <= GROUND_ROW
                && y > GROUND_ROW.saturating_sub(o.height)
        })
    }

    /// A pseudo-random fraction in `[0, 1)`, dependency-free.
    fn rand_frac(&mut self) -> f32 {
        (self.next_rand() % 10_000) as f32 / 10_000.0
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
    DinoRun,
    id: "dinorun",
    name: "Dino Run",
    description: "Jump the cacti in an endless desert dash.",
    author: "furybee",
}
