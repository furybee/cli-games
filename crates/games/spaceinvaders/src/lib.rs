//! Space Invaders — a real-time arcade defence game.
//!
//! Mirrors the structure of the `snake` reference crate: a single struct holds
//! all state, `update` reads input and advances motion by accumulating
//! `ctx.dt`, `render` draws a centred playfield with a controls hint and a
//! game-over / win overlay, and the file ends with `register_game!`.
//!
//! Move the cannon Left/Right, fire with Space. Aliens march side-to-side,
//! drop down and speed up as their ranks thin, and fire back occasionally.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Playfield dimensions in cells. Each cell renders as one character.
const WIDTH: u16 = 40;
const HEIGHT: u16 = 24;

/// Alien grid layout.
const COLS: u16 = 8;
const ROWS: u16 = 5;
const ALIEN_SPACING_X: u16 = 4;
const ALIEN_SPACING_Y: u16 = 2;

/// Vertical row the cannon lives on.
const PLAYER_ROW: u16 = HEIGHT - 1;

/// How fast things move (cells per second).
const PLAYER_SPEED: f32 = 22.0;
const PLAYER_BULLET_SPEED: f32 = 38.0;
const ALIEN_BULLET_SPEED: f32 = 16.0;

/// Starting lives.
const START_LIVES: u8 = 3;

#[derive(Clone, Copy)]
struct Bullet {
    x: u16,
    /// Fractional row position so motion is smooth across frames.
    y: f32,
}

pub struct Spaceinvaders {
    /// One flag per alien slot; `true` means still alive.
    aliens: Vec<bool>,
    /// Top-left origin of the alien block, in fractional cells.
    block_x: f32,
    block_y: f32,
    /// Marching direction: +1 right, -1 left.
    dir: f32,
    /// Pending downward drop in whole cells, applied next move tick.
    drop_pending: u16,
    /// Cannon column (fractional for smooth movement).
    player_x: f32,
    player_bullet: Option<Bullet>,
    alien_bullets: Vec<Bullet>,
    /// Time since last alien firing attempt.
    fire_timer: f32,
    lives: u8,
    score: u32,
    wave: u32,
    dead: bool,
    won: bool,
    rng: u64,
}

impl Game for Spaceinvaders {
    fn new() -> Self {
        let mut game = Spaceinvaders {
            aliens: vec![true; (COLS * ROWS) as usize],
            block_x: 2.0,
            block_y: 2.0,
            dir: 1.0,
            drop_pending: 0,
            player_x: (WIDTH / 2) as f32,
            player_bullet: None,
            alien_bullets: Vec::new(),
            fire_timer: 0.0,
            lives: START_LIVES,
            score: 0,
            wave: 1,
            dead: false,
            won: false,
            rng: seed(),
        };
        game.reset_wave();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.dead || self.won {
            if ctx.pressed(KeyCode::Enter) {
                *self = Spaceinvaders::new();
            }
            return Transition::Stay;
        }

        let dt = ctx.dt.as_secs_f32();

        // Cannon movement.
        if ctx.pressed(KeyCode::Left) {
            self.player_x -= PLAYER_SPEED * dt;
        }
        if ctx.pressed(KeyCode::Right) {
            self.player_x += PLAYER_SPEED * dt;
        }
        self.player_x = self.player_x.clamp(0.0, (WIDTH - 1) as f32);

        // Fire — one player bullet on screen at a time.
        if ctx.pressed(KeyCode::Char(' ')) && self.player_bullet.is_none() {
            self.player_bullet = Some(Bullet {
                x: self.player_x.round() as u16,
                y: (PLAYER_ROW - 1) as f32,
            });
        }

        self.advance_player_bullet(dt);
        self.advance_aliens(dt);
        self.advance_alien_bullets(dt);
        self.maybe_alien_fire(dt);

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(
            " Space Invaders  ·  score {}  ·  wave {}  ·  lives {} ",
            self.score, self.wave, self.lives
        );
        let field = centered(WIDTH + 2, HEIGHT + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        // Build the grid of characters row by row.
        let mut grid = vec![vec![Span::raw(" "); WIDTH as usize]; HEIGHT as usize];

        // Aliens.
        for row in 0..ROWS {
            for col in 0..COLS {
                if !self.alien_alive(col, row) {
                    continue;
                }
                let (ax, ay) = self.alien_cell(col, row);
                if ay < HEIGHT && ax < WIDTH {
                    let color = match row {
                        0 => Color::LightMagenta,
                        1 | 2 => Color::LightCyan,
                        _ => Color::LightGreen,
                    };
                    let glyph = if row == 0 { "W" } else { "M" };
                    grid[ay as usize][ax as usize] =
                        Span::styled(glyph, Style::default().fg(color));
                }
            }
        }

        // Alien bullets.
        for b in &self.alien_bullets {
            let by = b.y.round();
            if (0.0..HEIGHT as f32).contains(&by) && b.x < WIDTH {
                grid[by as usize][b.x as usize] =
                    Span::styled("!", Style::default().fg(Color::Red));
            }
        }

        // Player bullet.
        if let Some(b) = self.player_bullet {
            let by = b.y.round();
            if (0.0..HEIGHT as f32).contains(&by) && b.x < WIDTH {
                grid[by as usize][b.x as usize] =
                    Span::styled("|", Style::default().fg(Color::LightYellow));
            }
        }

        // Cannon.
        let px = self.player_x.round() as usize;
        if px < WIDTH as usize {
            grid[PLAYER_ROW as usize][px] = Span::styled(
                "A",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
        }

        let lines: Vec<Line> = grid.into_iter().map(Line::from).collect();
        frame.render_widget(Paragraph::new(lines), inner);

        // Controls hint just under the field.
        let hint = " ←/→ move · Space fire · q menu ";
        let hint_area = Rect {
            x: field.x,
            y: (field.y + field.height).min(area.y + area.height.saturating_sub(1)),
            width: field.width.min(area.width),
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
            hint_area,
        );

        if self.dead || self.won {
            let msg = if self.won {
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
            frame.render_widget(
                Paragraph::new(msg)
                    .block(Block::default().borders(Borders::ALL))
                    .style(
                        Style::default()
                            .fg(if self.won {
                                Color::LightGreen
                            } else {
                                Color::Yellow
                            })
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

impl Spaceinvaders {
    /// Reset the alien block for a fresh wave (keeps score and lives).
    fn reset_wave(&mut self) {
        for a in &mut self.aliens {
            *a = true;
        }
        self.block_x = 2.0;
        self.block_y = 2.0;
        self.dir = 1.0;
        self.drop_pending = 0;
        self.player_bullet = None;
        self.alien_bullets.clear();
        self.fire_timer = 0.0;
    }

    fn alien_index(col: u16, row: u16) -> usize {
        (row * COLS + col) as usize
    }

    fn alien_alive(&self, col: u16, row: u16) -> bool {
        self.aliens
            .get(Self::alien_index(col, row))
            .copied()
            .unwrap_or(false)
    }

    /// Screen cell of a given alien slot.
    fn alien_cell(&self, col: u16, row: u16) -> (u16, u16) {
        let x = self.block_x.round() as i32 + (col * ALIEN_SPACING_X) as i32;
        let y = self.block_y.round() as i32 + (row * ALIEN_SPACING_Y) as i32;
        (x.max(0) as u16, y.max(0) as u16)
    }

    fn alive_count(&self) -> usize {
        self.aliens.iter().filter(|&&a| a).count()
    }

    /// Marching: speed scales up as aliens are destroyed.
    fn advance_aliens(&mut self, dt: f32) {
        let total = (COLS * ROWS) as f32;
        let alive = self.alive_count() as f32;
        // Base speed grows with the wave and as the swarm thins.
        let thin_factor = 1.0 + (total - alive) / total * 3.0;
        let speed = (4.0 + self.wave as f32) * thin_factor;

        // Apply any pending drop smoothly before resuming horizontal march.
        if self.drop_pending > 0 {
            self.block_y += speed * dt;
            let _target = (self.drop_pending) as f32;
            // Snap once we've dropped enough this move.
            if self.block_y.fract() < 0.05 || speed * dt >= 1.0 {
                self.drop_pending = self.drop_pending.saturating_sub(1);
            }
            // Simpler robust approach: drop a whole cell then clear.
            return self.apply_drop();
        }

        self.block_x += self.dir * speed * dt;

        // Find the live column extents to know when to bounce.
        let mut min_x = WIDTH;
        let mut max_x = 0u16;
        let mut max_y = 0u16;
        let mut any = false;
        for row in 0..ROWS {
            for col in 0..COLS {
                if self.alien_alive(col, row) {
                    any = true;
                    let (ax, ay) = self.alien_cell(col, row);
                    min_x = min_x.min(ax);
                    max_x = max_x.max(ax);
                    max_y = max_y.max(ay);
                }
            }
        }

        if !any {
            // Wave cleared.
            self.wave += 1;
            self.score += 100;
            self.reset_wave();
            return;
        }

        // Bounce off the walls and queue a downward drop.
        if self.dir > 0.0 && max_x >= WIDTH - 1 {
            self.dir = -1.0;
            self.block_x = self
                .block_x
                .min((WIDTH - 1 - (COLS - 1) * ALIEN_SPACING_X) as f32);
            self.drop_pending = 1;
        } else if self.dir < 0.0 && min_x == 0 {
            self.dir = 1.0;
            self.block_x = self.block_x.max(0.0);
            self.drop_pending = 1;
        }

        // Reaching the cannon's row ends the game.
        if max_y >= PLAYER_ROW - 1 {
            self.dead = true;
        }
    }

    /// Move the swarm down by one whole cell, then resume the march.
    fn apply_drop(&mut self) {
        self.block_y = self.block_y.floor() + 1.0;
        self.drop_pending = 0;

        let mut max_y = 0u16;
        for row in 0..ROWS {
            for col in 0..COLS {
                if self.alien_alive(col, row) {
                    let (_, ay) = self.alien_cell(col, row);
                    max_y = max_y.max(ay);
                }
            }
        }
        if max_y >= PLAYER_ROW - 1 {
            self.dead = true;
        }
    }

    fn advance_player_bullet(&mut self, dt: f32) {
        let Some(mut b) = self.player_bullet else {
            return;
        };
        b.y -= PLAYER_BULLET_SPEED * dt;
        if b.y < 0.0 {
            self.player_bullet = None;
            return;
        }

        // Hit test against aliens.
        let by = b.y.round() as u16;
        for row in 0..ROWS {
            for col in 0..COLS {
                if !self.alien_alive(col, row) {
                    continue;
                }
                let (ax, ay) = self.alien_cell(col, row);
                if ax == b.x && ay == by {
                    self.aliens[Self::alien_index(col, row)] = false;
                    self.score += 10 + (ROWS - 1 - row) as u32 * 5;
                    self.player_bullet = None;
                    return;
                }
            }
        }

        self.player_bullet = Some(b);
    }

    fn advance_alien_bullets(&mut self, dt: f32) {
        let player_x = self.player_x.round() as u16;
        let mut hit = false;
        self.alien_bullets.retain_mut(|b| {
            b.y += ALIEN_BULLET_SPEED * dt;
            if b.y.round() as u16 >= PLAYER_ROW && b.x == player_x {
                hit = true;
                return false;
            }
            b.y < HEIGHT as f32
        });

        if hit {
            self.lives = self.lives.saturating_sub(1);
            if self.lives == 0 {
                self.dead = true;
            }
        }
    }

    /// Occasionally pick a random live column's lowest alien to fire.
    fn maybe_alien_fire(&mut self, dt: f32) {
        self.fire_timer += dt;
        // Higher waves fire more often.
        let interval = (1.2 - self.wave as f32 * 0.08).max(0.35);
        if self.fire_timer < interval {
            return;
        }
        self.fire_timer = 0.0;

        // Limit simultaneous alien bullets so it stays fair.
        if self.alien_bullets.len() >= 4 {
            return;
        }

        let col = (self.next_rand() % COLS as u64) as u16;
        // Lowest live alien in that column.
        let mut shooter: Option<(u16, u16)> = None;
        for row in (0..ROWS).rev() {
            if self.alien_alive(col, row) {
                shooter = Some(self.alien_cell(col, row));
                break;
            }
        }
        if let Some((ax, ay)) = shooter {
            self.alien_bullets.push(Bullet {
                x: ax,
                y: (ay + 1) as f32,
            });
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
    Spaceinvaders,
    id: "spaceinvaders",
    name: "Space Invaders",
    description: "Defend Earth from a descending alien armada.",
    author: "furybee",
}
