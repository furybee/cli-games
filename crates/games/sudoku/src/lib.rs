//! Sudoku — fill the 9×9 grid so every row, column, and 3×3 box contains 1–9.
//!
//! Move the cursor with the arrow keys (or `hjkl`), type `1`–`9` to place a
//! digit, and `0` / Backspace to clear. Given clues are fixed; cells that
//! clash with another entry are flagged in red. Solve it and the board locks
//! in green — press Enter for a fresh puzzle.

use std::time::{SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// How many of the 81 cells start empty. The rest are revealed clues.
const HOLES: usize = 45;

pub struct Sudoku {
    /// Current board; 0 means empty.
    grid: [[u8; 9]; 9],
    /// `true` where the cell is a fixed clue the player can't change.
    given: [[bool; 9]; 9],
    /// Cursor position as `(col, row)`.
    cursor: (usize, usize),
    solved: bool,
    rng: u64,
}

impl Game for Sudoku {
    fn new() -> Self {
        let mut game = Sudoku {
            grid: [[0; 9]; 9],
            given: [[false; 9]; 9],
            cursor: (0, 0),
            solved: false,
            rng: seed(),
        };
        game.generate();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.solved {
            if ctx.pressed(KeyCode::Enter) {
                *self = Sudoku::new();
            }
            return Transition::Stay;
        }

        let (cx, cy) = self.cursor;
        if ctx.pressed(KeyCode::Up) || ctx.pressed(KeyCode::Char('k')) {
            self.cursor.1 = cy.saturating_sub(1);
        }
        if ctx.pressed(KeyCode::Down) || ctx.pressed(KeyCode::Char('j')) {
            self.cursor.1 = (cy + 1).min(8);
        }
        if ctx.pressed(KeyCode::Left) || ctx.pressed(KeyCode::Char('h')) {
            self.cursor.0 = cx.saturating_sub(1);
        }
        if ctx.pressed(KeyCode::Right) || ctx.pressed(KeyCode::Char('l')) {
            self.cursor.0 = (cx + 1).min(8);
        }

        if !self.given[cy][cx] {
            for n in 1..=9u8 {
                if ctx.pressed(KeyCode::Char((b'0' + n) as char)) {
                    self.grid[cy][cx] = n;
                }
            }
            if ctx.pressed(KeyCode::Char('0'))
                || ctx.pressed(KeyCode::Backspace)
                || ctx.pressed(KeyCode::Delete)
            {
                self.grid[cy][cx] = 0;
            }
        }

        if self.is_solved() {
            self.solved = true;
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        // 9 cells × 3 chars + 10 separators = 37 wide; 9 rows + 10 rules = 19 tall.
        let board = centered(37 + 2, 19 + 2, area);
        let title = if self.solved {
            " Sudoku  ·  solved! ".to_string()
        } else {
            " Sudoku  ·  arrows/hjkl move · 1-9 place · 0 clear ".to_string()
        };
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(board);
        frame.render_widget(block, board);

        let conflicts = self.conflicts();
        let (cx, cy) = self.cursor;

        let mut lines: Vec<Line> = Vec::with_capacity(19);
        lines.push(rule_line(0));
        for (y, conflict_row) in conflicts.iter().enumerate() {
            let mut spans = vec![heavy_bar()];
            for (x, &conflict) in conflict_row.iter().enumerate() {
                let value = self.grid[y][x];
                let text = if value == 0 {
                    "   ".to_string()
                } else {
                    format!(" {value} ")
                };

                let mut style = if self.given[y][x] {
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else if conflict {
                    Style::default().fg(Color::Red)
                } else if self.solved {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Cyan)
                };
                if (x, y) == (cx, cy) && !self.solved {
                    style = style.bg(Color::DarkGray).add_modifier(Modifier::BOLD);
                }

                spans.push(Span::styled(text, style));
                spans.push(if x % 3 == 2 { heavy_bar() } else { light_bar() });
            }
            lines.push(Line::from(spans));
            lines.push(rule_line(y + 1));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        if self.solved {
            let msg = " SOLVED · Enter: new puzzle · q: menu ";
            let overlay = centered(msg.chars().count() as u16 + 2, 3, area);
            frame.render_widget(Clear, overlay);
            frame.render_widget(
                Paragraph::new(msg)
                    .block(Block::default().borders(Borders::ALL))
                    .style(
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                overlay,
            );
        }
    }
}

impl Sudoku {
    /// Build a fresh puzzle: a full solved grid with `HOLES` cells removed.
    fn generate(&mut self) {
        self.grid = [[0; 9]; 9];
        self.fill(0);

        let mut removed = 0;
        while removed < HOLES {
            let x = (self.next_rand() % 9) as usize;
            let y = (self.next_rand() % 9) as usize;
            if self.grid[y][x] != 0 {
                self.grid[y][x] = 0;
                removed += 1;
            }
        }

        for y in 0..9 {
            for x in 0..9 {
                self.given[y][x] = self.grid[y][x] != 0;
            }
        }
    }

    /// Recursively fill `self.grid` with a valid solution, trying digits in a
    /// random order so each puzzle differs. `idx` walks cells 0..81 row-major.
    fn fill(&mut self, idx: usize) -> bool {
        if idx == 81 {
            return true;
        }
        let (x, y) = (idx % 9, idx / 9);
        if self.grid[y][x] != 0 {
            return self.fill(idx + 1);
        }

        let mut nums = [1u8, 2, 3, 4, 5, 6, 7, 8, 9];
        self.shuffle(&mut nums);
        for &n in &nums {
            if self.fits(x, y, n) {
                self.grid[y][x] = n;
                if self.fill(idx + 1) {
                    return true;
                }
                self.grid[y][x] = 0;
            }
        }
        false
    }

    /// `true` if placing `n` at `(x, y)` breaks no row/column/box rule. Ignores
    /// the target cell itself so it serves both filling and validation.
    fn fits(&self, x: usize, y: usize, n: u8) -> bool {
        for i in 0..9 {
            if i != x && self.grid[y][i] == n {
                return false;
            }
            if i != y && self.grid[i][x] == n {
                return false;
            }
        }
        let (bx, by) = (x / 3 * 3, y / 3 * 3);
        for cy in by..by + 3 {
            for cx in bx..bx + 3 {
                if (cx, cy) != (x, y) && self.grid[cy][cx] == n {
                    return false;
                }
            }
        }
        true
    }

    /// Per-cell flags for filled cells that clash with another entry.
    fn conflicts(&self) -> [[bool; 9]; 9] {
        let mut out = [[false; 9]; 9];
        for (y, row) in out.iter_mut().enumerate() {
            for (x, cell) in row.iter_mut().enumerate() {
                let n = self.grid[y][x];
                *cell = n != 0 && !self.fits(x, y, n);
            }
        }
        out
    }

    fn is_solved(&self) -> bool {
        for y in 0..9 {
            for x in 0..9 {
                let n = self.grid[y][x];
                if n == 0 || !self.fits(x, y, n) {
                    return false;
                }
            }
        }
        true
    }

    /// Fisher–Yates shuffle driven by the crate-local RNG.
    fn shuffle(&mut self, nums: &mut [u8; 9]) {
        for i in (1..9).rev() {
            let j = (self.next_rand() % (i as u64 + 1)) as usize;
            nums.swap(i, j);
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

/// Vertical separator between 3×3 boxes.
fn heavy_bar() -> Span<'static> {
    Span::styled("┃", Style::default().fg(Color::Gray))
}

/// Vertical separator between cells inside a box.
fn light_bar() -> Span<'static> {
    Span::styled("│", Style::default().fg(Color::DarkGray))
}

/// A horizontal rule below row `y` (0 = top border). Heavier on box boundaries.
fn rule_line(y: usize) -> Line<'static> {
    let heavy = y.is_multiple_of(3);
    let (cell, joint) = if heavy {
        ("━━━", "╋")
    } else {
        ("───", "┼")
    };
    let mut s = String::from("┃");
    for x in 0..9 {
        s.push_str(cell);
        s.push_str(if x == 8 { "┃" } else { joint });
    }
    let color = if heavy { Color::Gray } else { Color::DarkGray };
    Line::from(Span::styled(s, Style::default().fg(color)))
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
    Sudoku,
    id: "sudoku",
    name: "Sudoku",
    description: "Fill the grid so every row, column, and box holds 1–9.",
    author: "furybee",
}
