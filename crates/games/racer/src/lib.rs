//! Road Racer — a real-time top-down dodging game.
//!
//! Your car sits near the bottom of a multi-lane road and hops Left/Right
//! between lanes. Obstacle cars scroll toward you from the top; touch one and
//! the run ends. Distance and survival raise the score while the road speeds
//! up. Mirrors the structure of the `snake` reference crate: `dt`-based timing,
//! an xorshift RNG, a centred playfield, and a game-over overlay.

use std::collections::VecDeque;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Number of lanes across the road.
const LANES: u16 = 4;
/// Visible road height in cells (rows).
const HEIGHT: u16 = 18;
/// Width of a single lane, in screen columns (each "cell" renders as 3 chars).
const LANE_W: u16 = 3;
/// Row the player's car occupies (near the bottom).
const PLAYER_ROW: u16 = HEIGHT - 2;
/// Starting fall: rows the obstacles advance per second.
const BASE_SPEED: f32 = 6.0;
/// How much the speed climbs per second of survival.
const SPEED_RAMP: f32 = 0.45;
/// Average rows between spawned obstacle rows (scaled by speed).
const SPAWN_GAP: f32 = 3.0;

/// One obstacle car: a lane plus a fractional row position so motion is smooth.
#[derive(Clone, Copy)]
struct Car {
    lane: u16,
    /// Vertical position in rows, measured from the top of the road.
    y: f32,
}

pub struct Racer {
    /// Lane the player currently sits in (`0..LANES`).
    player: u16,
    /// Active obstacle cars, oldest (lowest) toward the front.
    cars: VecDeque<Car>,
    /// Current downward speed in rows per second.
    speed: f32,
    /// Distance (in rows) until the next obstacle row spawns.
    spawn_countdown: f32,
    /// Accumulated score (distance travelled, weighted by speed).
    score: u32,
    /// Best score across runs in this session.
    best: u32,
    dead: bool,
    rng: u64,
}

impl Game for Racer {
    fn new() -> Self {
        Racer {
            player: LANES / 2,
            cars: VecDeque::new(),
            speed: BASE_SPEED,
            spawn_countdown: SPAWN_GAP,
            score: 0,
            best: 0,
            dead: false,
            rng: seed(),
        }
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.dead {
            if ctx.pressed(KeyCode::Enter) {
                let best = self.best.max(self.score);
                *self = Racer::new();
                self.best = best;
            }
            return Transition::Stay;
        }

        // Lane changes — single-step per press, clamped to the road.
        if ctx.pressed(KeyCode::Left) && self.player > 0 {
            self.player -= 1;
        }
        if ctx.pressed(KeyCode::Right) && self.player + 1 < LANES {
            self.player += 1;
        }

        let dt = ctx.dt.as_secs_f32();
        // Speed ramps up the longer you survive.
        self.speed += SPEED_RAMP * dt;
        let advance = self.speed * dt;

        // Score grows with distance covered this frame.
        self.score = self.score.saturating_add((advance * 2.0) as u32 + 1);

        // Advance every obstacle downward.
        for car in &mut self.cars {
            car.y += advance;
        }

        // Spawn new obstacle rows as the road scrolls under us.
        self.spawn_countdown -= advance;
        while self.spawn_countdown <= 0.0 {
            // Gap shrinks a little as speed rises, keeping the pressure on.
            let gap = (SPAWN_GAP + self.speed * 0.18).max(2.0);
            self.spawn_countdown += gap;
            self.spawn_row();
        }

        // Retire obstacles that have scrolled off the bottom.
        while self.cars.front().is_some_and(|c| c.y > HEIGHT as f32 + 1.0) {
            self.cars.pop_front();
        }

        // Collision: any car overlapping the player's lane and row.
        for car in &self.cars {
            if car.lane == self.player && car.y.round() as i32 == PLAYER_ROW as i32 {
                self.dead = true;
                self.best = self.best.max(self.score);
                break;
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(
            " Road Racer  ·  score {}  ·  best {} ",
            self.score, self.best
        );
        let width = LANES * LANE_W + 2;
        let field = centered(width, HEIGHT + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        // Build a row-major grid of lane occupants for quick lookup.
        let mut grid = vec![None::<bool>; (LANES * HEIGHT) as usize];
        for car in &self.cars {
            let ry = car.y.round();
            if (0.0..HEIGHT as f32).contains(&ry) {
                let idx = (ry as u16 * LANES + car.lane) as usize;
                if let Some(slot) = grid.get_mut(idx) {
                    *slot = Some(false);
                }
            }
        }
        // Mark the player.
        if let Some(slot) = grid.get_mut((PLAYER_ROW * LANES + self.player) as usize) {
            *slot = Some(true);
        }

        let mut lines = Vec::with_capacity(HEIGHT as usize);
        for y in 0..HEIGHT {
            let mut spans = Vec::with_capacity((LANES + 1) as usize);
            for lane in 0..LANES {
                let cell = grid.get((y * LANES + lane) as usize).copied().flatten();
                let span = match cell {
                    Some(true) => Span::styled(
                        "[O]",
                        Style::default()
                            .fg(Color::LightGreen)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Some(false) => Span::styled("<#>", Style::default().fg(Color::LightRed)),
                    None => {
                        // Dashed centre line gives a sense of motion per row.
                        if y % 2 == 0 {
                            Span::styled(" ' ", Style::default().fg(Color::DarkGray))
                        } else {
                            Span::raw("   ")
                        }
                    }
                };
                spans.push(span);
                // Lane separators except after the last lane.
                if lane + 1 < LANES {
                    spans.push(Span::styled("|", Style::default().fg(Color::DarkGray)));
                }
            }
            lines.push(Line::from(spans));
        }

        let hint = " ←/→ change lane · q: menu ";
        frame.render_widget(Paragraph::new(lines), inner);

        // Controls hint along the bottom border line.
        let hint_area = Rect {
            x: field.x + 1,
            y: field.y + field.height.saturating_sub(1),
            width: field.width.saturating_sub(2),
            height: 1,
        };
        if hint_area.height > 0 {
            frame.render_widget(
                Paragraph::new(Span::styled(hint, Style::default().fg(Color::Gray))),
                hint_area,
            );
        }

        if self.dead {
            let msg = format!(
                " CRASH! · score {} · best {} · Enter: replay · q: menu ",
                self.score, self.best
            );
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

impl Racer {
    /// Spawn one row of obstacles at the top, leaving at least one open lane so
    /// the row is always passable.
    fn spawn_row(&mut self) {
        let open = (self.next_rand() % LANES as u64) as u16;
        // Occasionally leave a second lane open for breathing room.
        let extra_open = if self.next_rand().is_multiple_of(3) {
            (self.next_rand() % LANES as u64) as u16
        } else {
            open
        };
        for lane in 0..LANES {
            if lane == open || lane == extra_open {
                continue;
            }
            self.cars.push_back(Car { lane, y: 0.0 });
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
    Racer,
    id: "racer",
    name: "Road Racer",
    description: "Dodge traffic, ramp up speed, don't crash.",
    author: "furybee",
}
