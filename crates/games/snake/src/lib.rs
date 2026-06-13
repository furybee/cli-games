//! Snake — the reference implementation. Copy this crate as a starting point
//! for a new game (see `docs/ADD_A_GAME.md`).
//!
//! It demonstrates the full pattern: state, input handling, `dt`-based timing,
//! a grid render, a game-over overlay, and self-registration.

use std::collections::{HashSet, VecDeque};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

const WIDTH: u16 = 24;
const HEIGHT: u16 = 16;
/// The snake advances one cell every `STEP`.
const STEP: Duration = Duration::from_millis(120);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    fn opposite(self) -> Dir {
        match self {
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
            Dir::Left => Dir::Right,
            Dir::Right => Dir::Left,
        }
    }
}

pub struct Snake {
    /// Front is the head.
    body: VecDeque<(u16, u16)>,
    dir: Dir,
    /// Buffered direction so a key press never causes an instant reversal.
    next_dir: Dir,
    food: (u16, u16),
    accumulator: Duration,
    dead: bool,
    score: u32,
    rng: u64,
}

impl Game for Snake {
    fn new() -> Self {
        let mut body = VecDeque::new();
        body.push_back((WIDTH / 2, HEIGHT / 2));
        let mut game = Snake {
            body,
            dir: Dir::Right,
            next_dir: Dir::Right,
            food: (0, 0),
            accumulator: Duration::ZERO,
            dead: false,
            score: 0,
            rng: seed(),
        };
        game.place_food();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.dead {
            if ctx.pressed(KeyCode::Enter) {
                *self = Snake::new();
            }
            return Transition::Stay;
        }

        // Latest direction key wins, but never a 180° turn.
        for &want in &[
            (KeyCode::Up, Dir::Up),
            (KeyCode::Down, Dir::Down),
            (KeyCode::Left, Dir::Left),
            (KeyCode::Right, Dir::Right),
        ] {
            if ctx.pressed(want.0) && want.1 != self.dir.opposite() {
                self.next_dir = want.1;
            }
        }

        self.accumulator += ctx.dt;
        while self.accumulator >= STEP {
            self.accumulator -= STEP;
            self.step();
            if self.dead {
                break;
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(" Snake  ·  score {} ", self.score);
        let field = centered(WIDTH * 2 + 2, HEIGHT + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let head = *self.body.front().expect("snake always has a head");
        let occupied: HashSet<(u16, u16)> = self.body.iter().copied().collect();

        let mut lines = Vec::with_capacity(HEIGHT as usize);
        for y in 0..HEIGHT {
            let mut spans = Vec::with_capacity(WIDTH as usize);
            for x in 0..WIDTH {
                let cell = (x, y);
                let span = if cell == head {
                    Span::styled("██", Style::default().fg(Color::LightGreen))
                } else if occupied.contains(&cell) {
                    Span::styled("██", Style::default().fg(Color::Green))
                } else if cell == self.food {
                    Span::styled("██", Style::default().fg(Color::Red))
                } else {
                    Span::raw("  ")
                };
                spans.push(span);
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        if self.dead {
            let msg = format!(" GAME OVER · score {} · Enter: replay · q: menu ", self.score);
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

impl Snake {
    fn step(&mut self) {
        self.dir = self.next_dir;
        let (hx, hy) = *self.body.front().expect("head exists");

        // Move, treating walls as fatal.
        let next = match self.dir {
            Dir::Up if hy > 0 => (hx, hy - 1),
            Dir::Down if hy + 1 < HEIGHT => (hx, hy + 1),
            Dir::Left if hx > 0 => (hx - 1, hy),
            Dir::Right if hx + 1 < WIDTH => (hx + 1, hy),
            _ => {
                self.dead = true;
                return;
            }
        };

        // Self-collision (the tail tip will move away unless we just ate).
        let eats = next == self.food;
        let tail = *self.body.back().expect("tail exists");
        if self.body.iter().any(|&c| c == next) && !(next == tail && !eats) {
            self.dead = true;
            return;
        }

        self.body.push_front(next);
        if eats {
            self.score += 1;
            self.place_food();
        } else {
            self.body.pop_back();
        }
    }

    fn place_food(&mut self) {
        let free = (WIDTH as usize * HEIGHT as usize).saturating_sub(self.body.len());
        if free == 0 {
            return; // board full — you win, effectively
        }
        let occupied: HashSet<(u16, u16)> = self.body.iter().copied().collect();
        loop {
            let x = (self.next_rand() % WIDTH as u64) as u16;
            let y = (self.next_rand() % HEIGHT as u64) as u16;
            if !occupied.contains(&(x, y)) {
                self.food = (x, y);
                return;
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
    Snake,
    id: "snake",
    name: "Snake",
    description: "Eat, grow, don't bite yourself.",
    author: "furybee",
}
