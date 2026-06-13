//! Tetris — stack falling tetrominoes and clear full lines.
//!
//! Follows the same pattern as the Snake reference crate: state, input,
//! `dt`-based gravity, a grid render with a side panel, a game-over overlay,
//! and self-registration. Dependency-free (xorshift RNG, 7-bag randomizer).

use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Playfield dimensions, in cells.
const WIDTH: i32 = 10;
const HEIGHT: i32 = 20;

/// The seven tetrominoes, each as four rotation states. A state is four cell
/// offsets `(x, y)` inside a 4×4 bounding box (guideline spawn orientations).
const SHAPES: [[[(i8, i8); 4]; 4]; 7] = [
    // I
    [
        [(0, 1), (1, 1), (2, 1), (3, 1)],
        [(2, 0), (2, 1), (2, 2), (2, 3)],
        [(0, 2), (1, 2), (2, 2), (3, 2)],
        [(1, 0), (1, 1), (1, 2), (1, 3)],
    ],
    // O
    [
        [(1, 0), (2, 0), (1, 1), (2, 1)],
        [(1, 0), (2, 0), (1, 1), (2, 1)],
        [(1, 0), (2, 0), (1, 1), (2, 1)],
        [(1, 0), (2, 0), (1, 1), (2, 1)],
    ],
    // T
    [
        [(1, 0), (0, 1), (1, 1), (2, 1)],
        [(1, 0), (1, 1), (2, 1), (1, 2)],
        [(0, 1), (1, 1), (2, 1), (1, 2)],
        [(1, 0), (0, 1), (1, 1), (1, 2)],
    ],
    // S
    [
        [(1, 0), (2, 0), (0, 1), (1, 1)],
        [(1, 0), (1, 1), (2, 1), (2, 2)],
        [(1, 1), (2, 1), (0, 2), (1, 2)],
        [(0, 0), (0, 1), (1, 1), (1, 2)],
    ],
    // Z
    [
        [(0, 0), (1, 0), (1, 1), (2, 1)],
        [(2, 0), (1, 1), (2, 1), (1, 2)],
        [(0, 1), (1, 1), (1, 2), (2, 2)],
        [(1, 0), (0, 1), (1, 1), (0, 2)],
    ],
    // J
    [
        [(0, 0), (0, 1), (1, 1), (2, 1)],
        [(1, 0), (2, 0), (1, 1), (1, 2)],
        [(0, 1), (1, 1), (2, 1), (2, 2)],
        [(1, 0), (1, 1), (0, 2), (1, 2)],
    ],
    // L
    [
        [(2, 0), (0, 1), (1, 1), (2, 1)],
        [(1, 0), (1, 1), (1, 2), (2, 2)],
        [(0, 1), (1, 1), (2, 1), (0, 2)],
        [(0, 0), (1, 0), (1, 1), (1, 2)],
    ],
];

/// Per-tetromino colour, indexed like [`SHAPES`].
const COLORS: [Color; 7] = [
    Color::Cyan,             // I
    Color::Yellow,           // O
    Color::Magenta,          // T
    Color::Green,            // S
    Color::Red,              // Z
    Color::Blue,             // J
    Color::Rgb(255, 165, 0), // L (orange)
];

/// Score awarded for clearing 0–4 lines at once (before the level multiplier).
const LINE_SCORES: [u32; 5] = [0, 100, 300, 500, 800];

pub struct Tetris {
    /// Settled blocks. `None` is empty; `Some(color)` is a locked cell.
    board: [[Option<Color>; WIDTH as usize]; HEIGHT as usize],
    /// Active piece: which tetromino, rotation, and 4×4-box top-left position.
    kind: usize,
    rot: usize,
    x: i32,
    y: i32,
    /// Next tetromino, previewed in the side panel.
    next: usize,
    /// 7-bag randomizer — every piece appears once before any repeats.
    bag: Vec<usize>,
    accumulator: Duration,
    score: u32,
    lines: u32,
    level: u32,
    dead: bool,
    rng: u64,
}

impl Game for Tetris {
    fn new() -> Self {
        let mut game = Tetris {
            board: [[None; WIDTH as usize]; HEIGHT as usize],
            kind: 0,
            rot: 0,
            x: 0,
            y: 0,
            next: 0,
            bag: Vec::new(),
            accumulator: Duration::ZERO,
            score: 0,
            lines: 0,
            level: 1,
            dead: false,
            rng: seed(),
        };
        game.next = game.draw_from_bag();
        game.spawn();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.dead {
            if ctx.pressed(KeyCode::Enter) {
                *self = Tetris::new();
            }
            return Transition::Stay;
        }

        if ctx.pressed(KeyCode::Left) {
            self.try_move(-1, 0);
        }
        if ctx.pressed(KeyCode::Right) {
            self.try_move(1, 0);
        }
        if ctx.pressed(KeyCode::Up) || ctx.pressed(KeyCode::Char('x')) {
            self.rotate();
        }
        if ctx.pressed(KeyCode::Down) {
            // Soft drop: nudge down a row and reward it slightly.
            if self.try_move(0, 1) {
                self.score += 1;
                self.accumulator = Duration::ZERO;
            }
        }
        if ctx.pressed(KeyCode::Char(' ')) {
            self.hard_drop();
        }

        // Gravity: drop one row each interval, scaled by level.
        let step = self.fall_interval();
        self.accumulator += ctx.dt;
        while self.accumulator >= step {
            self.accumulator -= step;
            if !self.try_move(0, 1) {
                self.lock();
                if self.dead {
                    break;
                }
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let board_w = (WIDTH * 2) as u16 + 2;
        let board_h = HEIGHT as u16 + 2;
        let panel_w = 16u16;
        let outer = centered(board_w + panel_w, board_h, area);

        let field = Rect {
            x: outer.x,
            y: outer.y,
            width: board_w,
            height: board_h,
        };
        let panel = Rect {
            x: outer.x + board_w,
            y: outer.y,
            width: panel_w.min(outer.width.saturating_sub(board_w)),
            height: board_h,
        };

        // Playfield.
        let block = Block::default().borders(Borders::ALL).title(" Tetris ");
        let inner = block.inner(field);
        frame.render_widget(block, field);

        // Overlay the active piece and its landing ghost onto the board.
        let piece: HashSet<(i32, i32)> = self
            .cells(self.kind, self.rot, self.x, self.y)
            .into_iter()
            .filter(|&(_, y)| y >= 0)
            .collect();
        let ghost_y = self.ghost_y();
        let ghost: HashSet<(i32, i32)> = self
            .cells(self.kind, self.rot, self.x, ghost_y)
            .into_iter()
            .filter(|&(_, y)| y >= 0)
            .collect();
        let color = COLORS[self.kind];

        let mut lines = Vec::with_capacity(HEIGHT as usize);
        for y in 0..HEIGHT {
            let mut spans = Vec::with_capacity(WIDTH as usize);
            for x in 0..WIDTH {
                let span = if piece.contains(&(x, y)) {
                    Span::styled("██", Style::default().fg(color))
                } else if let Some(c) = self.board[y as usize][x as usize] {
                    Span::styled("██", Style::default().fg(c))
                } else if ghost.contains(&(x, y)) {
                    Span::styled("▕▏", Style::default().fg(color))
                } else {
                    Span::raw(" ·")
                };
                spans.push(span);
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        // Side panel: stats + next-piece preview.
        self.render_panel(frame, panel);

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
        Duration::from_millis(16)
    }
}

impl Tetris {
    /// Draw the stats/next panel to the right of the playfield.
    fn render_panel(&self, frame: &mut Frame, panel: Rect) {
        if panel.width == 0 {
            return;
        }
        let block = Block::default().borders(Borders::ALL).title(" Info ");
        let inner = block.inner(panel);
        frame.render_widget(block, panel);

        let mut lines = vec![
            Line::from(format!("Score {}", self.score)),
            Line::from(format!("Lines {}", self.lines)),
            Line::from(format!("Level {}", self.level)),
            Line::from(""),
            Line::from("Next"),
        ];

        // Mini 4×4 preview of the next piece.
        let cells: HashSet<(i8, i8)> = SHAPES[self.next][0].iter().copied().collect();
        let color = COLORS[self.next];
        for py in 0..4 {
            let mut spans = Vec::with_capacity(4);
            for px in 0..4 {
                if cells.contains(&(px as i8, py as i8)) {
                    spans.push(Span::styled("██", Style::default().fg(color)));
                } else {
                    spans.push(Span::raw("  "));
                }
            }
            lines.push(Line::from(spans));
        }

        lines.push(Line::from(""));
        lines.push(Line::from("← → move"));
        lines.push(Line::from("↑ rotate"));
        lines.push(Line::from("↓ soft drop"));
        lines.push(Line::from("space drop"));

        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// Absolute board cells of a piece at `(x, y)`.
    fn cells(&self, kind: usize, rot: usize, x: i32, y: i32) -> [(i32, i32); 4] {
        let mut out = [(0, 0); 4];
        for (i, &(cx, cy)) in SHAPES[kind][rot].iter().enumerate() {
            out[i] = (x + cx as i32, y + cy as i32);
        }
        out
    }

    /// `true` if the piece would overlap a wall, the floor, or a settled cell.
    fn collides(&self, kind: usize, rot: usize, x: i32, y: i32) -> bool {
        self.cells(kind, rot, x, y).iter().any(|&(bx, by)| {
            !(0..WIDTH).contains(&bx)
                || by >= HEIGHT
                || (by >= 0 && self.board[by as usize][bx as usize].is_some())
        })
    }

    fn try_move(&mut self, dx: i32, dy: i32) -> bool {
        if self.collides(self.kind, self.rot, self.x + dx, self.y + dy) {
            return false;
        }
        self.x += dx;
        self.y += dy;
        true
    }

    /// Rotate clockwise, trying small horizontal kicks if the turn is blocked.
    fn rotate(&mut self) {
        let nr = (self.rot + 1) % 4;
        for kick in [0, -1, 1, -2, 2] {
            if !self.collides(self.kind, nr, self.x + kick, self.y) {
                self.x += kick;
                self.rot = nr;
                return;
            }
        }
    }

    /// The lowest `y` the current piece can occupy (for the ghost / hard drop).
    fn ghost_y(&self) -> i32 {
        let mut gy = self.y;
        while !self.collides(self.kind, self.rot, self.x, gy + 1) {
            gy += 1;
        }
        gy
    }

    fn hard_drop(&mut self) {
        let target = self.ghost_y();
        self.score += 2 * (target - self.y).max(0) as u32;
        self.y = target;
        self.lock();
    }

    /// Settle the active piece, clear filled rows, and spawn the next piece.
    fn lock(&mut self) {
        let color = COLORS[self.kind];
        for &(bx, by) in self.cells(self.kind, self.rot, self.x, self.y).iter() {
            if by < 0 {
                // Locked above the top of the well — the stack has topped out.
                self.dead = true;
                continue;
            }
            self.board[by as usize][bx as usize] = Some(color);
        }
        if self.dead {
            return;
        }
        self.clear_lines();
        self.spawn();
    }

    fn clear_lines(&mut self) {
        let mut cleared = 0u32;
        let mut write = HEIGHT - 1;
        // Compact surviving rows downward, top to bottom.
        for read in (0..HEIGHT).rev() {
            let full = self.board[read as usize].iter().all(Option::is_some);
            if full {
                cleared += 1;
            } else {
                self.board[write as usize] = self.board[read as usize];
                write -= 1;
            }
        }
        // Blank out the rows left at the top.
        for y in 0..=write {
            self.board[y as usize] = [None; WIDTH as usize];
        }

        if cleared > 0 {
            self.score += LINE_SCORES[cleared as usize] * self.level;
            self.lines += cleared;
            self.level = 1 + self.lines / 10;
        }
    }

    /// Promote `next` to active, draw a new `next`, and check for top-out.
    fn spawn(&mut self) {
        self.kind = self.next;
        self.next = self.draw_from_bag();
        self.rot = 0;
        self.x = 3;
        self.y = 0;
        if self.collides(self.kind, self.rot, self.x, self.y) {
            self.dead = true;
        }
    }

    /// 7-bag: refill and shuffle when empty so droughts can't happen.
    fn draw_from_bag(&mut self) -> usize {
        if self.bag.is_empty() {
            self.bag = (0..7).collect();
            // Fisher–Yates shuffle.
            for i in (1..self.bag.len()).rev() {
                let j = (self.next_rand() % (i as u64 + 1)) as usize;
                self.bag.swap(i, j);
            }
        }
        self.bag.pop().expect("bag refilled above")
    }

    /// Gravity interval, shrinking as the level climbs.
    fn fall_interval(&self) -> Duration {
        let speedup = self.level.saturating_sub(1).min(14) as u64 * 50;
        Duration::from_millis(800u64.saturating_sub(speedup).max(60))
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
    Tetris,
    id: "tetris",
    name: "Tetris",
    description: "Stack falling tetrominoes and clear lines.",
    author: "furybee",
}
