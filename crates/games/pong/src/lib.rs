//! Pong — you (left paddle) versus a CPU paddle that chases the ball.
//!
//! Follows the same pattern as the Snake reference: `dt`-accumulated physics,
//! a grid render, a game-over overlay, and self-registration. First player to
//! [`WIN_SCORE`] points wins the rally.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Playfield size in cells. Each cell renders two characters wide so the field
/// looks roughly square in a terminal (same trick Snake uses).
const WIDTH: u16 = 40;
const HEIGHT: u16 = 22;
const PADDLE_H: u16 = 5;
/// First to this many points wins.
const WIN_SCORE: u32 = 7;

/// Physics advances in fixed sub-steps for stable bounces regardless of frame rate.
const STEP: Duration = Duration::from_millis(16);
const STEP_SECS: f32 = 0.016;

/// Vertical speeds, in cells per second.
const PADDLE_SPEED: f32 = 26.0;
/// The CPU is deliberately a touch slower than the player so it's beatable.
const CPU_SPEED: f32 = 19.0;
/// Initial horizontal ball speed; it ramps up slightly on every paddle hit.
const BALL_SPEED_X: f32 = 24.0;
const BALL_SPEED_MAX: f32 = 46.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Move {
    Up,
    Down,
    None,
}

pub struct Pong {
    /// Top edge of each paddle (left = player, right = CPU).
    player_y: f32,
    cpu_y: f32,
    /// What the player asked for this tick.
    player_move: Move,
    ball_x: f32,
    ball_y: f32,
    ball_vx: f32,
    ball_vy: f32,
    player_score: u32,
    cpu_score: u32,
    /// `Some(player_won)` once someone reaches [`WIN_SCORE`].
    winner: Option<bool>,
    accumulator: Duration,
    rng: u64,
}

impl Game for Pong {
    fn new() -> Self {
        let mut game = Pong {
            player_y: (HEIGHT - PADDLE_H) as f32 / 2.0,
            cpu_y: (HEIGHT - PADDLE_H) as f32 / 2.0,
            player_move: Move::None,
            ball_x: 0.0,
            ball_y: 0.0,
            ball_vx: 0.0,
            ball_vy: 0.0,
            player_score: 0,
            cpu_score: 0,
            winner: None,
            accumulator: Duration::ZERO,
            rng: seed(),
        };
        game.serve(true);
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.winner.is_some() {
            if ctx.pressed(KeyCode::Enter) {
                *self = Pong::new();
            }
            return Transition::Stay;
        }

        // Continuous movement: terminal key-repeat re-fires Press while held.
        self.player_move = if ctx.pressed(KeyCode::Up) || ctx.pressed(KeyCode::Char('w')) {
            Move::Up
        } else if ctx.pressed(KeyCode::Down) || ctx.pressed(KeyCode::Char('s')) {
            Move::Down
        } else {
            Move::None
        };

        self.accumulator += ctx.dt;
        while self.accumulator >= STEP {
            self.accumulator -= STEP;
            self.physics();
            if self.winner.is_some() {
                break;
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(
            " Pong  ·  you {}  —  cpu {} ",
            self.player_score, self.cpu_score
        );
        let field = centered(WIDTH * 2 + 2, HEIGHT + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let ball = (self.ball_x.round() as i32, self.ball_y.round() as i32);
        let player_top = self.player_y.round() as i32;
        let cpu_top = self.cpu_y.round() as i32;

        let mut lines = Vec::with_capacity(HEIGHT as usize);
        for y in 0..HEIGHT as i32 {
            let mut spans = Vec::with_capacity(WIDTH as usize);
            for x in 0..WIDTH as i32 {
                let on_player = x == 0 && y >= player_top && y < player_top + PADDLE_H as i32;
                let on_cpu = x == WIDTH as i32 - 1 && y >= cpu_top && y < cpu_top + PADDLE_H as i32;
                let span = if (x, y) == ball {
                    Span::styled("██", Style::default().fg(Color::LightYellow))
                } else if on_player {
                    Span::styled("██", Style::default().fg(Color::LightCyan))
                } else if on_cpu {
                    Span::styled("██", Style::default().fg(Color::LightMagenta))
                } else if x == WIDTH as i32 / 2 && y % 2 == 0 {
                    // Dashed centre net.
                    Span::styled("┃ ", Style::default().fg(Color::DarkGray))
                } else {
                    Span::raw("  ")
                };
                spans.push(span);
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        if let Some(player_won) = self.winner {
            let who = if player_won { "YOU WIN" } else { "CPU WINS" };
            let msg = format!(" {who} · Enter: replay · q: menu ");
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
        Duration::from_millis(16)
    }
}

impl Pong {
    /// Advance physics by one fixed [`STEP`].
    fn physics(&mut self) {
        // Player paddle.
        match self.player_move {
            Move::Up => self.player_y -= PADDLE_SPEED * STEP_SECS,
            Move::Down => self.player_y += PADDLE_SPEED * STEP_SECS,
            Move::None => {}
        }
        self.player_y = clamp_paddle(self.player_y);

        // CPU tracks the ball's centre with a capped speed.
        let target = self.ball_y - (PADDLE_H as f32 - 1.0) / 2.0;
        let max = CPU_SPEED * STEP_SECS;
        let delta = (target - self.cpu_y).clamp(-max, max);
        self.cpu_y = clamp_paddle(self.cpu_y + delta);

        // Ball.
        self.ball_x += self.ball_vx * STEP_SECS;
        self.ball_y += self.ball_vy * STEP_SECS;

        // Bounce off top and bottom walls.
        if self.ball_y < 0.0 {
            self.ball_y = -self.ball_y;
            self.ball_vy = self.ball_vy.abs();
        } else if self.ball_y > (HEIGHT - 1) as f32 {
            self.ball_y = 2.0 * (HEIGHT - 1) as f32 - self.ball_y;
            self.ball_vy = -self.ball_vy.abs();
        }

        // Left paddle (player).
        if self.ball_x <= 0.0 && self.ball_vx < 0.0 {
            if let Some(offset) = self.paddle_hit(self.player_y) {
                self.ball_x = -self.ball_x;
                self.bounce(offset, true);
            } else {
                self.cpu_score += 1;
                self.finish_point(false);
            }
        }

        // Right paddle (CPU).
        if self.ball_x >= (WIDTH - 1) as f32 && self.ball_vx > 0.0 {
            if let Some(offset) = self.paddle_hit(self.cpu_y) {
                self.ball_x = 2.0 * (WIDTH - 1) as f32 - self.ball_x;
                self.bounce(offset, false);
            } else {
                self.player_score += 1;
                self.finish_point(true);
            }
        }
    }

    /// If the ball's row overlaps a paddle whose top is `paddle_top`, return the
    /// hit offset from the paddle centre in `-1.0..=1.0`.
    fn paddle_hit(&self, paddle_top: f32) -> Option<f32> {
        let top = paddle_top;
        let bottom = paddle_top + PADDLE_H as f32;
        if self.ball_y >= top - 0.5 && self.ball_y <= bottom - 0.5 {
            let center = paddle_top + (PADDLE_H as f32 - 1.0) / 2.0;
            Some(((self.ball_y - center) / (PADDLE_H as f32 / 2.0)).clamp(-1.0, 1.0))
        } else {
            None
        }
    }

    /// Reflect the ball off a paddle, steering vertically by the hit `offset`
    /// and nudging the speed up a little each rally.
    fn bounce(&mut self, offset: f32, going_right: bool) {
        let speed = (self.ball_vx.abs() + 1.5).min(BALL_SPEED_MAX);
        self.ball_vx = if going_right { speed } else { -speed };
        self.ball_vy = offset * speed * 0.75;
    }

    fn finish_point(&mut self, player_scored: bool) {
        if self.player_score >= WIN_SCORE {
            self.winner = Some(true);
        } else if self.cpu_score >= WIN_SCORE {
            self.winner = Some(false);
        } else {
            // Serve toward whoever just lost the point.
            self.serve(!player_scored);
        }
    }

    /// Centre the ball and launch it toward the player (`true`) or CPU.
    fn serve(&mut self, toward_player: bool) {
        self.ball_x = (WIDTH - 1) as f32 / 2.0;
        self.ball_y = (HEIGHT - 1) as f32 / 2.0;
        self.ball_vx = if toward_player {
            -BALL_SPEED_X
        } else {
            BALL_SPEED_X
        };
        // A small, randomised vertical kick so serves aren't identical.
        let r = (self.next_rand() % 1000) as f32 / 1000.0; // 0.0..1.0
        self.ball_vy = (r - 0.5) * BALL_SPEED_X;
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

fn clamp_paddle(y: f32) -> f32 {
    y.clamp(0.0, (HEIGHT - PADDLE_H) as f32)
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
    Pong,
    id: "pong",
    name: "Pong",
    description: "Volley past a chasing CPU paddle — first to 7 wins.",
    author: "furybee",
}
