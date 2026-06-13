//! Asteroids — a real-time space shooter built on the same pattern as `snake`.
//!
//! The ship rotates (Left/Right), thrusts with momentum (Up) and fires (Space).
//! Asteroids drift and wrap around the playfield edges; shooting a large one
//! splits it into smaller, faster fragments. The ship wraps too, and a
//! collision costs a life. All motion is driven by `ctx.dt` so the feel stays
//! constant regardless of the runner's poll rate.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Logical playfield size in cells. Each cell renders as two characters wide so
/// the field looks square on a typical terminal (mirrors snake's `WIDTH * 2`).
const WIDTH: f32 = 40.0;
const HEIGHT: f32 = 24.0;

/// Ship handling.
const ROT_SPEED: f32 = 3.4; // radians per second
const THRUST: f32 = 26.0; // cells per second^2
const DRAG: f32 = 0.6; // velocity retained-per-second factor applied smoothly
const MAX_SPEED: f32 = 22.0;

/// Bullets.
const BULLET_SPEED: f32 = 34.0;
const BULLET_LIFE: f32 = 1.2; // seconds
const FIRE_COOLDOWN: f32 = 0.18; // seconds between shots
const MAX_BULLETS: usize = 8;

/// Asteroid sizing. Index 0 = large, 1 = medium, 2 = small.
const AST_RADII: [f32; 3] = [3.4, 2.1, 1.1];
const AST_SCORES: [u32; 3] = [20, 50, 100];
const START_ASTEROIDS: usize = 4;
const RESPAWN_INVULN: f32 = 1.6; // seconds of invulnerability after a hit

#[derive(Clone, Copy)]
struct Ship {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    /// Heading in radians; 0 points up the screen.
    angle: f32,
}

#[derive(Clone, Copy)]
struct Bullet {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
}

#[derive(Clone, Copy)]
struct Asteroid {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    /// 0 = large, 1 = medium, 2 = small.
    size: usize,
}

pub struct Asteroids {
    ship: Ship,
    bullets: Vec<Bullet>,
    asteroids: Vec<Asteroid>,
    cooldown: f32,
    invuln: f32,
    lives: u32,
    score: u32,
    level: u32,
    dead: bool,
    rng: u64,
}

impl Game for Asteroids {
    fn new() -> Self {
        let mut game = Asteroids {
            ship: spawn_ship(),
            bullets: Vec::new(),
            asteroids: Vec::new(),
            cooldown: 0.0,
            invuln: RESPAWN_INVULN,
            lives: 3,
            score: 0,
            level: 1,
            dead: false,
            rng: seed(),
        };
        game.spawn_wave(START_ASTEROIDS);
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.dead {
            if ctx.pressed(KeyCode::Enter) {
                *self = Asteroids::new();
            }
            return Transition::Stay;
        }

        let dt = ctx.dt.as_secs_f32();
        // Guard against absurd frame gaps (e.g. after a stall) so physics stays sane.
        let dt = dt.min(0.1);

        self.handle_input(ctx, dt);
        self.advance(dt);
        self.collide();

        // Clear the field, advance to the next wave.
        if self.asteroids.is_empty() {
            self.level += 1;
            let count = START_ASTEROIDS + self.level as usize;
            self.spawn_wave(count);
            self.invuln = self.invuln.max(0.8);
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(
            " Asteroids  ·  score {}  ·  lives {}  ·  wave {} ",
            self.score, self.lives, self.level
        );
        let field = centered(WIDTH as u16 * 2 + 2, HEIGHT as u16 + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let cols = WIDTH as usize;
        let rows = HEIGHT as usize;
        // Build a glyph grid: each cell is (char, color). Default is empty space.
        let mut grid: Vec<Vec<(char, Color)>> = vec![vec![(' ', Color::Black); cols]; rows];

        // Asteroids — draw a filled disc of '#' so big ones read as big.
        for a in &self.asteroids {
            let glyph = match a.size {
                0 => '#',
                1 => 'o',
                _ => '.',
            };
            let color = match a.size {
                0 => Color::Gray,
                1 => Color::DarkGray,
                _ => Color::White,
            };
            let r = AST_RADII[a.size];
            stamp_disc(&mut grid, a.x, a.y, r, glyph, color);
        }

        // Bullets.
        for b in &self.bullets {
            put(&mut grid, b.x, b.y, '*', Color::LightYellow);
        }

        // Ship — a directional glyph that hints at the heading, blinking while invulnerable.
        let show_ship = self.invuln <= 0.0 || ((self.invuln * 8.0) as i32) % 2 == 0;
        if show_ship {
            let glyph = ship_glyph(self.ship.angle);
            let color = if self.invuln > 0.0 {
                Color::LightCyan
            } else {
                Color::Cyan
            };
            put(&mut grid, self.ship.x, self.ship.y, glyph, color);
        }

        // Emit one Line per row; each cell is two chars wide to keep the aspect square.
        let mut lines = Vec::with_capacity(rows);
        for row in &grid {
            let mut spans = Vec::with_capacity(cols);
            for &(ch, color) in row {
                if ch == ' ' {
                    spans.push(Span::raw("  "));
                } else {
                    let mut s = String::with_capacity(2);
                    s.push(ch);
                    s.push(' ');
                    spans.push(Span::styled(s, Style::default().fg(color)));
                }
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        // Controls hint, tucked just under the field if there is room.
        let hint = " ←/→ turn · ↑ thrust · Space fire · q menu ";
        let hint_area = Rect {
            x: field.x,
            y: field.y + field.height,
            width: hint.len().min(field.width as usize) as u16,
            height: 1,
        };
        if hint_area.y < area.y + area.height {
            frame.render_widget(
                Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
                hint_area,
            );
        }

        if self.dead {
            let msg = format!(
                " GAME OVER · score {} · Enter: replay · q: menu ",
                self.score
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

impl Asteroids {
    fn handle_input(&mut self, ctx: &GameContext, dt: f32) {
        if ctx.pressed(KeyCode::Left) {
            self.ship.angle -= ROT_SPEED * dt;
        }
        if ctx.pressed(KeyCode::Right) {
            self.ship.angle += ROT_SPEED * dt;
        }

        if ctx.pressed(KeyCode::Up) {
            // Heading 0 points up, so thrust is (sin, -cos).
            self.ship.vx += self.ship.angle.sin() * THRUST * dt;
            self.ship.vy += -self.ship.angle.cos() * THRUST * dt;
        }

        self.cooldown = (self.cooldown - dt).max(0.0);
        if ctx.pressed(KeyCode::Char(' '))
            && self.cooldown <= 0.0
            && self.bullets.len() < MAX_BULLETS
        {
            self.bullets.push(Bullet {
                x: self.ship.x,
                y: self.ship.y,
                vx: self.ship.angle.sin() * BULLET_SPEED + self.ship.vx,
                vy: -self.ship.angle.cos() * BULLET_SPEED + self.ship.vy,
                life: BULLET_LIFE,
            });
            self.cooldown = FIRE_COOLDOWN;
        }
    }

    fn advance(&mut self, dt: f32) {
        // Apply drag as exponential decay so it is frame-rate independent.
        let decay = DRAG.powf(dt);
        self.ship.vx *= decay;
        self.ship.vy *= decay;

        // Clamp top speed.
        let speed = (self.ship.vx * self.ship.vx + self.ship.vy * self.ship.vy).sqrt();
        if speed > MAX_SPEED {
            let scale = MAX_SPEED / speed;
            self.ship.vx *= scale;
            self.ship.vy *= scale;
        }

        self.ship.x = wrap(self.ship.x + self.ship.vx * dt, WIDTH);
        self.ship.y = wrap(self.ship.y + self.ship.vy * dt, HEIGHT);

        for a in &mut self.asteroids {
            a.x = wrap(a.x + a.vx * dt, WIDTH);
            a.y = wrap(a.y + a.vy * dt, HEIGHT);
        }

        for b in &mut self.bullets {
            b.x = wrap(b.x + b.vx * dt, WIDTH);
            b.y = wrap(b.y + b.vy * dt, HEIGHT);
            b.life -= dt;
        }
        self.bullets.retain(|b| b.life > 0.0);

        self.invuln = (self.invuln - dt).max(0.0);
    }

    fn collide(&mut self) {
        // Bullet vs asteroid. Collect fragments separately to avoid borrow clashes.
        let mut new_asteroids: Vec<Asteroid> = Vec::new();
        let mut parents_to_split: Vec<Asteroid> = Vec::new();
        let mut gained = 0u32;

        // Track which bullets and asteroids are consumed this tick.
        let mut bullet_hit = vec![false; self.bullets.len()];
        let mut ast_hit = vec![false; self.asteroids.len()];

        for (bi, b) in self.bullets.iter().enumerate() {
            for (ai, a) in self.asteroids.iter().enumerate() {
                if ast_hit[ai] {
                    continue;
                }
                if dist2_wrapped(b.x, b.y, a.x, a.y) <= AST_RADII[a.size] * AST_RADII[a.size] {
                    bullet_hit[bi] = true;
                    ast_hit[ai] = true;
                    gained += AST_SCORES[a.size];
                    // Split large/medium into two faster fragments (spawned
                    // after the loop, to avoid borrowing self both ways).
                    if a.size < 2 {
                        parents_to_split.push(*a);
                    }
                    break;
                }
            }
        }

        if gained > 0 {
            self.score += gained;
        }

        for parent in &parents_to_split {
            for _ in 0..2 {
                let frag = self.spawn_fragment(parent);
                new_asteroids.push(frag);
            }
        }

        // Remove consumed bullets and asteroids, keeping order stable.
        let mut bi = 0;
        self.bullets.retain(|_| {
            let keep = !bullet_hit[bi];
            bi += 1;
            keep
        });
        let mut ai = 0;
        self.asteroids.retain(|_| {
            let keep = !ast_hit[ai];
            ai += 1;
            keep
        });
        self.asteroids.append(&mut new_asteroids);

        // Ship vs asteroid (only when vulnerable).
        if self.invuln <= 0.0 {
            let ship = self.ship;
            let hit = self.asteroids.iter().any(|a| {
                let r = AST_RADII[a.size] + 0.6;
                dist2_wrapped(ship.x, ship.y, a.x, a.y) <= r * r
            });
            if hit {
                self.lose_life();
            }
        }
    }

    fn lose_life(&mut self) {
        if self.lives > 0 {
            self.lives -= 1;
        }
        if self.lives == 0 {
            self.dead = true;
            return;
        }
        self.ship = spawn_ship();
        self.invuln = RESPAWN_INVULN;
    }

    fn spawn_wave(&mut self, count: usize) {
        for _ in 0..count {
            // Spawn away from the ship so the player isn't instantly hit.
            let mut x;
            let mut y;
            loop {
                x = self.frand() * WIDTH;
                y = self.frand() * HEIGHT;
                if dist2_wrapped(x, y, self.ship.x, self.ship.y) > 8.0 * 8.0 {
                    break;
                }
            }
            let ang = self.frand() * std::f32::consts::TAU;
            let speed = 3.0 + self.frand() * 4.0;
            self.asteroids.push(Asteroid {
                x,
                y,
                vx: ang.cos() * speed,
                vy: ang.sin() * speed,
                size: 0,
            });
        }
    }

    fn spawn_fragment(&mut self, parent: &Asteroid) -> Asteroid {
        let ang = self.frand() * std::f32::consts::TAU;
        let speed = 5.0 + self.frand() * 5.0;
        Asteroid {
            x: parent.x,
            y: parent.y,
            vx: ang.cos() * speed,
            vy: ang.sin() * speed,
            size: parent.size + 1,
        }
    }

    /// xorshift64 — keeps the crate dependency-free (mirrors snake).
    fn next_rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    /// A float in [0, 1).
    fn frand(&mut self) -> f32 {
        // Use the top 24 bits for a clean mantissa.
        let bits = (self.next_rand() >> 40) as u32;
        bits as f32 / (1u32 << 24) as f32
    }
}

fn spawn_ship() -> Ship {
    Ship {
        x: WIDTH / 2.0,
        y: HEIGHT / 2.0,
        vx: 0.0,
        vy: 0.0,
        angle: 0.0,
    }
}

/// Pick a glyph that roughly points along the heading (0 = up, clockwise).
fn ship_glyph(angle: f32) -> char {
    let tau = std::f32::consts::TAU;
    // Normalise to [0, tau) then bucket into 8 directions.
    let mut a = angle % tau;
    if a < 0.0 {
        a += tau;
    }
    let sector = ((a / tau) * 8.0).round() as i32 % 8;
    match sector {
        0 => '^',
        1 => '/',
        2 => '>',
        3 => '\\',
        4 => 'v',
        5 => '/',
        6 => '<',
        _ => '\\',
    }
}

/// Wrap a coordinate into `[0, max)`.
fn wrap(v: f32, max: f32) -> f32 {
    let mut r = v % max;
    if r < 0.0 {
        r += max;
    }
    r
}

/// Squared distance between two points on a torus of size WIDTH×HEIGHT.
fn dist2_wrapped(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let mut dx = (ax - bx).abs();
    let mut dy = (ay - by).abs();
    if dx > WIDTH / 2.0 {
        dx = WIDTH - dx;
    }
    if dy > HEIGHT / 2.0 {
        dy = HEIGHT - dy;
    }
    dx * dx + dy * dy
}

/// Place a single glyph at a floating-point position, with wrapping.
fn put(grid: &mut [Vec<(char, Color)>], x: f32, y: f32, ch: char, color: Color) {
    let cols = grid.first().map(|r| r.len()).unwrap_or(0);
    let rows = grid.len();
    if cols == 0 || rows == 0 {
        return;
    }
    let cx = (wrap(x, WIDTH) as usize).min(cols - 1);
    let cy = (wrap(y, HEIGHT) as usize).min(rows - 1);
    grid[cy][cx] = (ch, color);
}

/// Stamp a filled disc of radius `r` centred at (cx, cy), with wrapping.
fn stamp_disc(grid: &mut [Vec<(char, Color)>], cx: f32, cy: f32, r: f32, ch: char, color: Color) {
    let cols = grid.first().map(|row| row.len()).unwrap_or(0);
    let rows = grid.len();
    if cols == 0 || rows == 0 {
        return;
    }
    let ri = r.ceil() as i32;
    let r2 = r * r;
    let base_x = wrap(cx, WIDTH);
    let base_y = wrap(cy, HEIGHT);
    for dy in -ri..=ri {
        for dx in -ri..=ri {
            let fx = dx as f32;
            let fy = dy as f32;
            if fx * fx + fy * fy > r2 {
                continue;
            }
            let gx = (wrap(base_x + fx, WIDTH) as usize).min(cols - 1);
            let gy = (wrap(base_y + fy, HEIGHT) as usize).min(rows - 1);
            // Outline reads a bit crisper: only fill the rim for the body glyph.
            grid[gy][gx] = (ch, color);
        }
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
    Asteroids,
    id: "asteroids",
    name: "Asteroids",
    description: "Rotate, thrust and shoot drifting space rocks.",
    author: "furybee",
}
