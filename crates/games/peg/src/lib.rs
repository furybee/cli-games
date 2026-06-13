//! Peg Solitaire — the classic cross-shaped (English) board.
//!
//! Mirrors the structure of the `snake` reference crate: explicit state,
//! `ctx.pressed` input handling, a centred grid render, a win/lose overlay, and
//! self-registration. The board is mostly turn-based; we still accumulate
//! `ctx.dt` to blink the cursor so the selection is easy to spot.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// The board is a 7×7 grid; the four 2×2 corners are off-board.
const SIZE: usize = 7;
/// Cursor blink half-period.
const BLINK: Duration = Duration::from_millis(450);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    /// Not part of the cross — never drawn, never reachable.
    Invalid,
    /// A playable hole that currently holds a peg.
    Peg,
    /// A playable hole that is empty.
    Empty,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    /// Step `(dx, dy)` for this direction (y grows downward).
    fn delta(self) -> (i32, i32) {
        match self {
            Dir::Up => (0, -1),
            Dir::Down => (0, 1),
            Dir::Left => (-1, 0),
            Dir::Right => (1, 0),
        }
    }
}

pub struct Peg {
    board: [[Cell; SIZE]; SIZE],
    /// Cursor position over a playable cell.
    cursor: (usize, usize),
    /// The currently selected peg, if the player has picked one to move.
    selected: Option<(usize, usize)>,
    /// Pegs still on the board.
    pegs: u32,
    /// `true` once no legal move remains and more than one peg is left.
    stuck: bool,
    /// Blink accumulator for the cursor highlight.
    blink: Duration,
    blink_on: bool,
    rng: u64,
}

impl Game for Peg {
    fn new() -> Self {
        let mut board = [[Cell::Invalid; SIZE]; SIZE];
        let mut pegs = 0;
        for (y, row) in board.iter_mut().enumerate() {
            for (x, cell) in row.iter_mut().enumerate() {
                if on_cross(x, y) {
                    *cell = Cell::Peg;
                    pegs += 1;
                }
            }
        }
        // Hollow out the centre — the standard starting position.
        board[SIZE / 2][SIZE / 2] = Cell::Empty;
        pegs -= 1;

        Peg {
            board,
            cursor: (SIZE / 2, SIZE / 2),
            selected: None,
            pegs,
            stuck: false,
            blink: Duration::ZERO,
            blink_on: true,
            rng: seed(),
        }
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        // Cursor blink — purely cosmetic, driven by dt like snake's timing.
        self.blink += ctx.dt;
        while self.blink >= BLINK {
            self.blink -= BLINK;
            self.blink_on = !self.blink_on;
        }

        let finished = self.pegs == 1 || self.stuck;
        if finished {
            if ctx.pressed(KeyCode::Enter) {
                let rng = self.rng;
                *self = Peg::new();
                self.rng = rng;
            }
            return Transition::Stay;
        }

        // Enter / space restarts at any time as a quick reset, but only when no
        // peg is currently selected (so it doesn't fight the move flow).
        if self.selected.is_none() && ctx.pressed(KeyCode::Enter) {
            let rng = self.rng;
            *self = Peg::new();
            self.rng = rng;
            return Transition::Stay;
        }

        // Space toggles selection of the peg under the cursor.
        if ctx.pressed(KeyCode::Char(' ')) {
            let (cx, cy) = self.cursor;
            match self.selected {
                Some(sel) if sel == self.cursor => self.selected = None,
                _ if self.board[cy][cx] == Cell::Peg => self.selected = Some(self.cursor),
                _ => {}
            }
        }

        // Arrow keys either move the cursor, or — with a peg selected — attempt a
        // jump in that direction.
        for &(key, dir) in &[
            (KeyCode::Up, Dir::Up),
            (KeyCode::Down, Dir::Down),
            (KeyCode::Left, Dir::Left),
            (KeyCode::Right, Dir::Right),
        ] {
            if !ctx.pressed(key) {
                continue;
            }
            if let Some(from) = self.selected {
                if self.try_jump(from, dir) {
                    self.selected = None;
                    if self.pegs > 1 && !self.any_move() {
                        self.stuck = true;
                    }
                }
            } else {
                self.move_cursor(dir);
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(" Peg Solitaire  ·  pegs {} ", self.pegs);
        // Each cell renders as two columns; the cross is 7 wide.
        let field = centered(SIZE as u16 * 2 + 2, SIZE as u16 + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let mut lines = Vec::with_capacity(SIZE);
        for y in 0..SIZE {
            let mut spans = Vec::with_capacity(SIZE);
            for x in 0..SIZE {
                spans.push(self.cell_span(x, y));
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        // Controls hint just below the board.
        let hint = if self.selected.is_some() {
            " arrows: jump · space: cancel · q: menu "
        } else {
            " arrows: move · space: select · Enter: restart · q: menu "
        };
        let hint_rect = Rect {
            x: field.x,
            y: field.y.saturating_add(field.height),
            width: field.width.max(hint.len() as u16).min(area.width),
            height: 1,
        };
        if hint_rect.y < area.y.saturating_add(area.height) {
            frame.render_widget(
                Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray))),
                hint_rect,
            );
        }

        let won = self.pegs == 1;
        if won || self.stuck {
            let msg = if won {
                " YOU WIN! · one peg left · Enter: replay · q: menu ".to_string()
            } else {
                format!(
                    " NO MOVES · {} pegs left · Enter: replay · q: menu ",
                    self.pegs
                )
            };
            let overlay = centered(msg.chars().count() as u16 + 2, 3, area);
            frame.render_widget(Clear, overlay);
            frame.render_widget(
                Paragraph::new(msg)
                    .block(Block::default().borders(Borders::ALL))
                    .style(
                        Style::default()
                            .fg(if won {
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

impl Peg {
    /// Render a single board cell as a two-column span.
    fn cell_span(&self, x: usize, y: usize) -> Span<'static> {
        let cell = self.board[y][x];
        if cell == Cell::Invalid {
            return Span::raw("  ");
        }

        let is_cursor = self.cursor == (x, y);
        let is_selected = self.selected == Some((x, y));

        match cell {
            Cell::Peg => {
                let color = if is_selected {
                    Color::LightCyan
                } else if is_cursor && self.blink_on {
                    Color::LightYellow
                } else {
                    Color::Green
                };
                let glyph = if is_selected { "◉ " } else { "● " };
                Span::styled(glyph, Style::default().fg(color))
            }
            Cell::Empty => {
                if is_cursor && self.blink_on {
                    Span::styled("□ ", Style::default().fg(Color::LightYellow))
                } else {
                    Span::styled("· ", Style::default().fg(Color::DarkGray))
                }
            }
            Cell::Invalid => Span::raw("  "),
        }
    }

    /// Move the cursor one playable cell in `dir`, skipping off-board cells and
    /// staying put if there is nowhere valid to land.
    fn move_cursor(&mut self, dir: Dir) {
        let (dx, dy) = dir.delta();
        let (mut nx, mut ny) = (self.cursor.0 as i32, self.cursor.1 as i32);
        loop {
            nx += dx;
            ny += dy;
            if !in_bounds(nx, ny) {
                return; // ran off the board — keep the cursor where it was
            }
            let (ux, uy) = (nx as usize, ny as usize);
            if self.board[uy][ux] != Cell::Invalid {
                self.cursor = (ux, uy);
                return;
            }
            // Otherwise we landed on an off-board corner cell; keep scanning so
            // the cursor can hop across the notches of the cross.
        }
    }

    /// Attempt the standard solitaire jump: peg at `from` hops over an adjacent
    /// peg into the empty hole two steps away in `dir`. Returns `true` on success.
    fn try_jump(&mut self, from: (usize, usize), dir: Dir) -> bool {
        let (dx, dy) = dir.delta();
        let (fx, fy) = (from.0 as i32, from.1 as i32);
        let (mx, my) = (fx + dx, fy + dy); // jumped peg
        let (tx, ty) = (fx + 2 * dx, fy + 2 * dy); // destination

        if !in_bounds(mx, my) || !in_bounds(tx, ty) {
            return false;
        }
        let (mx, my, tx, ty) = (mx as usize, my as usize, tx as usize, ty as usize);

        if self.board[fy as usize][fx as usize] == Cell::Peg
            && self.board[my][mx] == Cell::Peg
            && self.board[ty][tx] == Cell::Empty
        {
            self.board[fy as usize][fx as usize] = Cell::Empty;
            self.board[my][mx] = Cell::Empty;
            self.board[ty][tx] = Cell::Peg;
            self.pegs -= 1;
            self.cursor = (tx, ty);
            true
        } else {
            false
        }
    }

    /// Is any legal jump still available anywhere on the board?
    fn any_move(&self) -> bool {
        for y in 0..SIZE {
            for x in 0..SIZE {
                if self.board[y][x] != Cell::Peg {
                    continue;
                }
                for dir in [Dir::Up, Dir::Down, Dir::Left, Dir::Right] {
                    let (dx, dy) = dir.delta();
                    let (mx, my) = (x as i32 + dx, y as i32 + dy);
                    let (tx, ty) = (x as i32 + 2 * dx, y as i32 + 2 * dy);
                    if in_bounds(mx, my)
                        && in_bounds(tx, ty)
                        && self.board[my as usize][mx as usize] == Cell::Peg
                        && self.board[ty as usize][tx as usize] == Cell::Empty
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// xorshift64 — kept for parity with the snake template; used only to carry
    /// a stable seed across restarts (the board itself is deterministic).
    #[allow(dead_code)]
    fn next_rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }
}

/// `true` if `(x, y)` lies on the English cross (all but the 2×2 corners).
fn on_cross(x: usize, y: usize) -> bool {
    let corner = !(2..SIZE - 2).contains(&x) && !(2..SIZE - 2).contains(&y);
    !corner
}

/// Bounds check for signed grid coordinates.
fn in_bounds(x: i32, y: i32) -> bool {
    x >= 0 && y >= 0 && (x as usize) < SIZE && (y as usize) < SIZE
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
    Peg,
    id: "peg",
    name: "Peg Solitaire",
    description: "Jump pegs across the cross until one remains.",
    author: "furybee",
}
