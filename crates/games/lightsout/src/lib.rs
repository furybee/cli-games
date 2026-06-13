//! Lights Out — a 5×5 grid of lights. Toggling a cell flips it and its four
//! orthogonal neighbours; clear the whole board to win.
//!
//! Mirrors the Snake reference: `dt`-free turn-based input, a centred grid
//! render, a win overlay, and self-registration. The scramble is built by
//! applying random presses to a solved board, so every start is solvable.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Side length of the (square) board.
const SIZE: usize = 5;
/// Number of random presses used to scramble a solved board.
const SCRAMBLE_PRESSES: usize = 12;

pub struct Lightsout {
    /// `true` means the light at that cell is on.
    cells: [[bool; SIZE]; SIZE],
    /// Current cursor position as `(col, row)`.
    cursor: (usize, usize),
    moves: u32,
    won: bool,
    rng: u64,
}

impl Game for Lightsout {
    fn new() -> Self {
        let mut game = Lightsout {
            cells: [[false; SIZE]; SIZE],
            cursor: (SIZE / 2, SIZE / 2),
            moves: 0,
            won: false,
            rng: seed(),
        };
        game.scramble();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.won {
            if ctx.pressed(KeyCode::Enter) {
                *self = Lightsout::new();
            }
            return Transition::Stay;
        }

        let (cx, cy) = self.cursor;
        if ctx.pressed(KeyCode::Up) && cy > 0 {
            self.cursor.1 = cy - 1;
        }
        if ctx.pressed(KeyCode::Down) && cy + 1 < SIZE {
            self.cursor.1 = cy + 1;
        }
        if ctx.pressed(KeyCode::Left) && cx > 0 {
            self.cursor.0 = cx - 1;
        }
        if ctx.pressed(KeyCode::Right) && cx + 1 < SIZE {
            self.cursor.0 = cx + 1;
        }

        if ctx.pressed(KeyCode::Char(' ')) || ctx.pressed(KeyCode::Enter) {
            let (px, py) = self.cursor;
            self.press(px, py);
            self.moves += 1;
            if self.is_clear() {
                self.won = true;
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(" Lights Out  ·  moves {} ", self.moves);
        // Each cell renders as a 4-wide × 2-tall block, plus the border.
        let field = centered(SIZE as u16 * 4 + 2, SIZE as u16 * 2 + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let mut lines = Vec::with_capacity(SIZE * 2 + 1);
        for y in 0..SIZE {
            // Two rows of glyphs give each cell some height.
            for _ in 0..2 {
                let mut spans = Vec::with_capacity(SIZE);
                for x in 0..SIZE {
                    let on = self.cells[y][x];
                    let is_cursor = self.cursor == (x, y);
                    let glyph = if is_cursor { "[██]" } else { " ██ " };
                    let color = if on { Color::Yellow } else { Color::DarkGray };
                    let mut style = Style::default().fg(color);
                    if is_cursor {
                        style = style.add_modifier(Modifier::BOLD).fg(if on {
                            Color::LightYellow
                        } else {
                            Color::Gray
                        });
                    }
                    spans.push(Span::styled(glyph, style));
                }
                lines.push(Line::from(spans));
            }
        }
        lines.push(Line::from(Span::styled(
            "arrows: move · space: toggle · q: menu",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(Paragraph::new(lines), inner);

        if self.won {
            let msg = format!(" SOLVED in {} moves · Enter: replay · q: menu ", self.moves);
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

impl Lightsout {
    /// Toggle `(x, y)` and its orthogonal neighbours, staying in bounds.
    fn press(&mut self, x: usize, y: usize) {
        self.cells[y][x] = !self.cells[y][x];
        if y > 0 {
            self.cells[y - 1][x] = !self.cells[y - 1][x];
        }
        if y + 1 < SIZE {
            self.cells[y + 1][x] = !self.cells[y + 1][x];
        }
        if x > 0 {
            self.cells[y][x - 1] = !self.cells[y][x - 1];
        }
        if x + 1 < SIZE {
            self.cells[y][x + 1] = !self.cells[y][x + 1];
        }
    }

    /// Build a guaranteed-solvable board by pressing random cells from solved.
    fn scramble(&mut self) {
        self.cells = [[false; SIZE]; SIZE];
        // Keep scrambling until the board is not already solved.
        loop {
            for _ in 0..SCRAMBLE_PRESSES {
                let x = (self.next_rand() % SIZE as u64) as usize;
                let y = (self.next_rand() % SIZE as u64) as usize;
                self.press(x, y);
            }
            if !self.is_clear() {
                break;
            }
        }
    }

    fn is_clear(&self) -> bool {
        self.cells.iter().all(|row| row.iter().all(|&on| !on))
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
    Lightsout,
    id: "lightsout",
    name: "Lights Out",
    description: "Flip crosses of lights until the board goes dark.",
    author: "furybee",
}
