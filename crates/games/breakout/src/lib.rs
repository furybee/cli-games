//! Breakout — a real-time paddle-and-ball brick smasher.
//!
//! Mirrors the Snake reference: `dt`-driven motion, a centred playfield, an
//! xorshift RNG seeded from the clock, and a `Clear` + bordered `Paragraph`
//! overlay for the win / game-over states. Move the paddle Left/Right; the ball
//! bounces off the walls, the paddle and the bricks. Clear every brick to win;
//! you have three lives and the ball speeds up the longer a life lasts.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Playfield size in cells. Each cell renders as two characters wide so the
/// aspect ratio stays roughly square in a terminal.
const WIDTH: u16 = 24;
const HEIGHT: u16 = 20;

/// Brick layout.
const BRICK_ROWS: u16 = 5;
const BRICK_COLS: u16 = WIDTH; // one brick per column

/// Paddle geometry.
const PADDLE_WIDTH: f32 = 5.0;
const PADDLE_ROW: u16 = HEIGHT - 1;
const PADDLE_SPEED: f32 = 28.0; // cells per second

/// Ball speed in cells per second, and how fast it ramps up per life.
const BALL_START_SPEED: f32 = 14.0;
const BALL_MAX_SPEED: f32 = 34.0;
const BALL_ACCEL: f32 = 1.2; // added speed per second of play

const START_LIVES: u32 = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Ball is sitting on the paddle; press Space to launch.
    Ready,
    Playing,
    Lost,
    Won,
}

pub struct Breakout {
    /// One flag per brick, row-major. `true` means the brick is still standing.
    bricks: Vec<bool>,
    bricks_left: u16,
    /// Paddle centre x, in cell units.
    paddle_x: f32,
    /// Ball position and velocity, in cell units / cells-per-second.
    ball_x: f32,
    ball_y: f32,
    vel_x: f32,
    vel_y: f32,
    speed: f32,
    phase: Phase,
    lives: u32,
    score: u32,
    rng: u64,
}

impl Game for Breakout {
    fn new() -> Self {
        let mut game = Breakout {
            bricks: vec![true; (BRICK_ROWS * BRICK_COLS) as usize],
            bricks_left: BRICK_ROWS * BRICK_COLS,
            paddle_x: WIDTH as f32 / 2.0,
            ball_x: 0.0,
            ball_y: 0.0,
            vel_x: 0.0,
            vel_y: 0.0,
            speed: BALL_START_SPEED,
            phase: Phase::Ready,
            lives: START_LIVES,
            score: 0,
            rng: seed(),
        };
        game.reset_ball();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        match self.phase {
            Phase::Lost | Phase::Won => {
                if ctx.pressed(KeyCode::Enter) {
                    *self = Breakout::new();
                }
                return Transition::Stay;
            }
            _ => {}
        }

        let dt = ctx.dt.as_secs_f32();

        // Paddle movement (continuous while held — `pressed` reports any press
        // during the tick, which at this poll rate feels smooth).
        let half = PADDLE_WIDTH / 2.0;
        if ctx.pressed(KeyCode::Left) || ctx.pressed(KeyCode::Char('a')) {
            self.paddle_x -= PADDLE_SPEED * dt;
        }
        if ctx.pressed(KeyCode::Right) || ctx.pressed(KeyCode::Char('d')) {
            self.paddle_x += PADDLE_SPEED * dt;
        }
        self.paddle_x = self.paddle_x.clamp(half, WIDTH as f32 - half);

        if self.phase == Phase::Ready {
            // Glue the ball to the paddle until launch.
            self.ball_x = self.paddle_x;
            self.ball_y = PADDLE_ROW as f32 - 1.0;
            if ctx.pressed(KeyCode::Char(' ')) || ctx.pressed(KeyCode::Enter) {
                self.launch();
            }
            return Transition::Stay;
        }

        // Ball physics, sub-stepped so a fast ball can't tunnel through a brick.
        self.speed = (self.speed + BALL_ACCEL * dt).min(BALL_MAX_SPEED);
        let steps = 4;
        let sub = dt / steps as f32;
        for _ in 0..steps {
            self.advance_ball(sub);
            if self.phase != Phase::Playing {
                break;
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(
            " Breakout  ·  score {}  ·  lives {} ",
            self.score, self.lives
        );
        let field = centered(WIDTH * 2 + 2, HEIGHT + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let ball_cell = (self.ball_x as i32, self.ball_y as i32);
        let half = PADDLE_WIDTH / 2.0;
        let paddle_lo = (self.paddle_x - half).round() as i32;
        let paddle_hi = (self.paddle_x + half).round() as i32;

        let mut lines = Vec::with_capacity(HEIGHT as usize);
        for y in 0..HEIGHT {
            let mut spans = Vec::with_capacity(WIDTH as usize);
            for x in 0..WIDTH {
                let xi = x as i32;
                let yi = y as i32;
                let span = if (xi, yi) == ball_cell {
                    Span::styled("()", Style::default().fg(Color::White))
                } else if y == PADDLE_ROW && xi >= paddle_lo && xi <= paddle_hi {
                    Span::styled("▀▀", Style::default().fg(Color::Cyan))
                } else if y < BRICK_ROWS && self.brick_at(x, y) {
                    Span::styled("▐▌", Style::default().fg(row_color(y)))
                } else {
                    Span::raw("  ")
                };
                spans.push(span);
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        let hint = match self.phase {
            Phase::Ready => " ←/→ move · Space: launch · q: menu ".to_string(),
            _ => " ←/→ move · q: menu ".to_string(),
        };
        let hint_area = Rect {
            x: field.x,
            y: field.y.saturating_add(field.height),
            width: field.width,
            height: 1,
        };
        if hint_area.y < area.y + area.height {
            frame.render_widget(
                Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
                hint_area,
            );
        }

        if self.phase == Phase::Lost || self.phase == Phase::Won {
            let msg = if self.phase == Phase::Won {
                format!(
                    " YOU WIN! · score {} · Enter: replay · q: menu ",
                    self.score
                )
            } else {
                format!(
                    " GAME OVER · score {} · Enter: replay · q: menu ",
                    self.score
                )
            };
            let overlay = centered(msg.chars().count() as u16 + 2, 3, area);
            frame.render_widget(Clear, overlay);
            let color = if self.phase == Phase::Won {
                Color::LightGreen
            } else {
                Color::Yellow
            };
            frame.render_widget(
                Paragraph::new(msg)
                    .block(Block::default().borders(Borders::ALL))
                    .style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
                overlay,
            );
        }
    }

    fn tick_rate(&self) -> Duration {
        Duration::from_millis(30)
    }
}

impl Breakout {
    /// Whether the brick covering cell `(x, y)` is still standing.
    fn brick_at(&self, x: u16, y: u16) -> bool {
        if y >= BRICK_ROWS || x >= BRICK_COLS {
            return false;
        }
        self.bricks
            .get((y * BRICK_COLS + x) as usize)
            .copied()
            .unwrap_or(false)
    }

    /// Remove the brick at `(x, y)` if present; returns `true` on a hit.
    fn break_brick(&mut self, x: u16, y: u16) -> bool {
        if y >= BRICK_ROWS || x >= BRICK_COLS {
            return false;
        }
        let idx = (y * BRICK_COLS + x) as usize;
        if let Some(cell) = self.bricks.get_mut(idx)
            && *cell
        {
            *cell = false;
            self.bricks_left = self.bricks_left.saturating_sub(1);
            self.score += 10 + (BRICK_ROWS - 1 - y) as u32 * 5;
            return true;
        }
        false
    }

    /// Park the ball on the paddle and wait for a launch.
    fn reset_ball(&mut self) {
        self.phase = Phase::Ready;
        self.speed = BALL_START_SPEED;
        self.ball_x = self.paddle_x;
        self.ball_y = PADDLE_ROW as f32 - 1.0;
        self.vel_x = 0.0;
        self.vel_y = 0.0;
    }

    /// Send the ball upward with a slight random horizontal lean.
    fn launch(&mut self) {
        // Pick an angle leaning left or right but always upward.
        let lean = ((self.next_rand() % 1001) as f32 / 1000.0) - 0.5; // -0.5..0.5
        self.vel_x = lean;
        self.vel_y = -1.0;
        self.normalize_velocity();
        self.phase = Phase::Playing;
    }

    /// Rescale `(vel_x, vel_y)` to the current ball speed, keeping a sane angle.
    fn normalize_velocity(&mut self) {
        // Avoid a near-horizontal trajectory that would stall forever.
        if self.vel_y.abs() < 0.25 {
            self.vel_y = if self.vel_y < 0.0 { -0.25 } else { 0.25 };
        }
        let mag = (self.vel_x * self.vel_x + self.vel_y * self.vel_y).sqrt();
        if mag > f32::EPSILON {
            self.vel_x = self.vel_x / mag * self.speed;
            self.vel_y = self.vel_y / mag * self.speed;
        }
    }

    /// Move the ball by `dt` seconds and resolve every collision.
    fn advance_ball(&mut self, dt: f32) {
        // Velocity is stored as a direction during launch; ensure it carries the
        // speed magnitude here (idempotent once normalized).
        let mag = (self.vel_x * self.vel_x + self.vel_y * self.vel_y).sqrt();
        if (mag - self.speed).abs() > 0.01 && mag > f32::EPSILON {
            self.vel_x = self.vel_x / mag * self.speed;
            self.vel_y = self.vel_y / mag * self.speed;
        }

        self.ball_x += self.vel_x * dt;
        self.ball_y += self.vel_y * dt;

        // Side walls.
        if self.ball_x < 0.0 {
            self.ball_x = -self.ball_x;
            self.vel_x = self.vel_x.abs();
        } else if self.ball_x > (WIDTH - 1) as f32 {
            self.ball_x = 2.0 * (WIDTH - 1) as f32 - self.ball_x;
            self.vel_x = -self.vel_x.abs();
        }

        // Ceiling.
        if self.ball_y < 0.0 {
            self.ball_y = -self.ball_y;
            self.vel_y = self.vel_y.abs();
        }

        // Brick collision (bricks occupy the top rows).
        let bx = self.ball_x as i32;
        let by = self.ball_y as i32;
        if by >= 0
            && by < BRICK_ROWS as i32
            && bx >= 0
            && bx < BRICK_COLS as i32
            && self.break_brick(bx as u16, by as u16)
        {
            self.vel_y = -self.vel_y;
            if self.bricks_left == 0 {
                self.phase = Phase::Won;
            }
            return;
        }

        // Paddle collision: ball at the paddle row, moving down, within span.
        let half = PADDLE_WIDTH / 2.0;
        if self.vel_y > 0.0
            && self.ball_y >= PADDLE_ROW as f32 - 1.0
            && self.ball_y <= PADDLE_ROW as f32
            && self.ball_x >= self.paddle_x - half - 0.5
            && self.ball_x <= self.paddle_x + half + 0.5
        {
            // Reflect, steering by where it struck the paddle.
            let offset = (self.ball_x - self.paddle_x) / (half + 0.5); // -1..1
            self.vel_x = offset.clamp(-1.0, 1.0);
            self.vel_y = -1.0;
            self.ball_y = PADDLE_ROW as f32 - 1.0;
            self.normalize_velocity();
            return;
        }

        // Missed the paddle — fell off the bottom.
        if self.ball_y > HEIGHT as f32 {
            self.lives = self.lives.saturating_sub(1);
            if self.lives == 0 {
                self.phase = Phase::Lost;
            } else {
                self.reset_ball();
            }
        }
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

/// Colour for brick row `y` (top rows are worth more).
fn row_color(y: u16) -> Color {
    match y {
        0 => Color::Red,
        1 => Color::LightRed,
        2 => Color::Yellow,
        3 => Color::Green,
        _ => Color::Blue,
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
    Breakout,
    id: "breakout",
    name: "Breakout",
    description: "Bounce a ball, smash every brick, don't drop it.",
    author: "furybee",
}
