//! Flappy — tap to flap a bird through gaps in scrolling pipes.
//!
//! Built from the Snake template (see `docs/ADD_A_GAME.md`). It demonstrates
//! float-based physics driven by `ctx.dt`, procedurally spawned obstacles, a
//! grid render, a game-over overlay, and self-registration.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Playfield size in cells. Each cell is drawn two characters wide so the
/// field looks square in a typical terminal.
const WIDTH: u16 = 32;
const HEIGHT: u16 = 20;

/// Downward acceleration, in cells per second².
const GRAVITY: f32 = 38.0;
/// Upward velocity set on a flap, in cells per second (negative = up).
const FLAP_VELOCITY: f32 = -13.0;
/// How fast pipes scroll left, in cells per second.
const SCROLL_SPEED: f32 = 11.0;
/// Horizontal gap between successive pipes, in cells.
const PIPE_SPACING: f32 = 16.0;
/// Vertical opening each pipe leaves for the bird, in cells.
const GAP_HEIGHT: u16 = 6;
/// The bird sits at this fixed column; pipes flow toward it.
const BIRD_X: u16 = 7;

struct Pipe {
    /// Column of the pipe (float so it scrolls smoothly between cells).
    x: f32,
    /// Top of the gap (cells above are wall).
    gap_top: u16,
    /// Whether the bird has already cleared this pipe (for scoring once).
    scored: bool,
}

pub struct Flappy {
    /// Bird vertical position (0 = top), in cells.
    bird_y: f32,
    /// Bird vertical velocity, in cells per second.
    bird_vel: f32,
    pipes: Vec<Pipe>,
    /// Distance scrolled since the last pipe spawn.
    since_spawn: f32,
    dead: bool,
    started: bool,
    score: u32,
    rng: u64,
}

impl Game for Flappy {
    fn new() -> Self {
        let mut game = Flappy {
            bird_y: HEIGHT as f32 / 2.0,
            bird_vel: 0.0,
            pipes: Vec::new(),
            since_spawn: PIPE_SPACING,
            dead: false,
            started: false,
            score: 0,
            rng: seed(),
        };
        // Seed the field so the first pipe is already approaching.
        game.spawn_pipe(WIDTH as f32 + 4.0);
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        let flap = ctx.pressed(KeyCode::Char(' '))
            || ctx.pressed(KeyCode::Up)
            || ctx.pressed(KeyCode::Enter);

        if self.dead {
            if flap {
                *self = Flappy::new();
            }
            return Transition::Stay;
        }

        // The bird hovers until the first flap, so the player can get ready.
        if !self.started {
            if flap {
                self.started = true;
                self.bird_vel = FLAP_VELOCITY;
            }
            return Transition::Stay;
        }

        if flap {
            self.bird_vel = FLAP_VELOCITY;
        }

        let dt = ctx.dt.as_secs_f32();
        self.bird_vel += GRAVITY * dt;
        self.bird_y += self.bird_vel * dt;

        // Floor and ceiling are fatal.
        if self.bird_y <= 0.0 || self.bird_y >= (HEIGHT - 1) as f32 {
            self.bird_y = self.bird_y.clamp(0.0, (HEIGHT - 1) as f32);
            self.dead = true;
            return Transition::Stay;
        }

        let scroll = SCROLL_SPEED * dt;
        for pipe in &mut self.pipes {
            pipe.x -= scroll;
        }
        self.pipes.retain(|p| p.x > -1.0);

        self.since_spawn += scroll;
        if self.since_spawn >= PIPE_SPACING {
            self.since_spawn -= PIPE_SPACING;
            self.spawn_pipe(WIDTH as f32);
        }

        self.check_pipes();

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(" Flappy  ·  score {} ", self.score);
        let field = centered(WIDTH * 2 + 2, HEIGHT + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let bird_cell = self.bird_y.round() as u16;

        let mut lines = Vec::with_capacity(HEIGHT as usize);
        for y in 0..HEIGHT {
            let mut spans = Vec::with_capacity(WIDTH as usize);
            for x in 0..WIDTH {
                let span = if x == BIRD_X && y == bird_cell {
                    Span::styled("◆ ", Style::default().fg(Color::Yellow))
                } else if self.is_pipe(x, y) {
                    Span::styled("██", Style::default().fg(Color::Green))
                } else {
                    Span::raw("  ")
                };
                spans.push(span);
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        if !self.started && !self.dead {
            self.overlay(frame, area, " Space / ↑ to flap ", Color::Cyan);
        } else if self.dead {
            let msg = format!(
                " GAME OVER · score {} · Space: replay · q: menu ",
                self.score
            );
            self.overlay(frame, area, &msg, Color::Yellow);
        }
    }

    fn tick_rate(&self) -> Duration {
        Duration::from_millis(20)
    }
}

impl Flappy {
    /// Spawn a pipe at column `x` with a randomly placed gap.
    fn spawn_pipe(&mut self, x: f32) {
        // Keep the gap fully inside the field with a one-cell margin.
        let range = (HEIGHT - GAP_HEIGHT - 2) as u64;
        let gap_top = 1 + (self.next_rand() % (range + 1)) as u16;
        self.pipes.push(Pipe {
            x,
            gap_top,
            scored: false,
        });
    }

    /// Is cell `(x, y)` part of any pipe (wall, not gap)?
    fn is_pipe(&self, x: u16, y: u16) -> bool {
        self.pipes.iter().any(|p| {
            p.x.round() as i32 == x as i32 && (y < p.gap_top || y >= p.gap_top + GAP_HEIGHT)
        })
    }

    /// Award score for cleared pipes and detect collisions with pipe walls.
    fn check_pipes(&mut self) {
        let bird_cell = self.bird_y.round() as u16;
        for pipe in &mut self.pipes {
            let px = pipe.x.round() as i32;
            if px == BIRD_X as i32
                && (bird_cell < pipe.gap_top || bird_cell >= pipe.gap_top + GAP_HEIGHT)
            {
                self.dead = true;
            }
            // Score once the pipe has scrolled past the bird's column.
            if !pipe.scored && px < BIRD_X as i32 {
                pipe.scored = true;
                self.score += 1;
            }
        }
    }

    /// Draw a centred single-line overlay box.
    fn overlay(&self, frame: &mut Frame, area: Rect, msg: &str, color: Color) {
        let overlay = centered(msg.chars().count() as u16 + 2, 3, area);
        frame.render_widget(Clear, overlay);
        frame.render_widget(
            Paragraph::new(msg)
                .block(Block::default().borders(Borders::ALL))
                .style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
            overlay,
        );
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
    Flappy,
    id: "flappy",
    name: "Flappy",
    description: "Tap to flap through the pipes.",
    author: "furybee",
}
