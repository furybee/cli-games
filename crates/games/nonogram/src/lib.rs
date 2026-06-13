//! Nonogram — a picture-logic puzzle on a small fixed grid.
//!
//! Numbers around the grid (clues) describe runs of filled cells in each row and
//! column. Move the cursor, `Space` toggles a filled cell, `X` toggles an
//! "empty" mark. Solve the picture to win.
//!
//! Mirrors the snake reference: `dt`-driven blink animation, the `centered`
//! helper, the xorshift `next_rand`/`seed` pattern, and a `Clear` + bordered
//! `Paragraph` win overlay.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// All built-in puzzles are square and this big.
const SIZE: usize = 5;

/// State of a single cell.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    /// Untouched.
    Blank,
    /// Player believes this cell is part of the picture.
    Filled,
    /// Player marked this cell as definitely empty.
    Marked,
}

/// A built-in puzzle: `name` plus the solution grid (`true` = filled).
struct Puzzle {
    name: &'static str,
    solution: [[bool; SIZE]; SIZE],
}

/// Two hand-made 5x5 pictures.
const PUZZLES: [Puzzle; 2] = [
    Puzzle {
        // A heart.
        name: "Heart",
        solution: [
            [false, true, false, true, false],
            [true, true, true, true, true],
            [true, true, true, true, true],
            [false, true, true, true, false],
            [false, false, true, false, false],
        ],
    },
    Puzzle {
        // A smiley face.
        name: "Smiley",
        solution: [
            [false, true, false, true, false],
            [false, true, false, true, false],
            [false, false, false, false, false],
            [true, false, false, false, true],
            [false, true, true, true, false],
        ],
    },
];

pub struct Nonogram {
    /// Index into `PUZZLES`.
    puzzle: usize,
    /// Player's working grid.
    cells: [[Cell; SIZE]; SIZE],
    /// Cursor position (col, row).
    cursor: (usize, usize),
    /// Whether the picture is solved.
    won: bool,
    /// Drives the cursor blink.
    blink: Duration,
    rng: u64,
}

impl Game for Nonogram {
    fn new() -> Self {
        let mut game = Nonogram {
            puzzle: 0,
            cells: [[Cell::Blank; SIZE]; SIZE],
            cursor: (0, 0),
            won: false,
            blink: Duration::ZERO,
            rng: seed(),
        };
        // Start on a random one of the built-in puzzles for variety.
        game.puzzle = (game.next_rand() % PUZZLES.len() as u64) as usize;
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        self.blink += ctx.dt;

        if self.won {
            if ctx.pressed(KeyCode::Enter) {
                // Advance to the next puzzle and reset the board.
                let next = (self.puzzle + 1) % PUZZLES.len();
                *self = Nonogram::new();
                self.puzzle = next;
            }
            return Transition::Stay;
        }

        let (mut cx, mut cy) = self.cursor;
        if ctx.pressed(KeyCode::Left) {
            cx = cx.saturating_sub(1);
        }
        if ctx.pressed(KeyCode::Right) && cx + 1 < SIZE {
            cx += 1;
        }
        if ctx.pressed(KeyCode::Up) {
            cy = cy.saturating_sub(1);
        }
        if ctx.pressed(KeyCode::Down) && cy + 1 < SIZE {
            cy += 1;
        }
        self.cursor = (cx, cy);

        if ctx.pressed(KeyCode::Char(' ')) {
            let c = &mut self.cells[cy][cx];
            *c = if *c == Cell::Filled {
                Cell::Blank
            } else {
                Cell::Filled
            };
        }
        if ctx.pressed(KeyCode::Char('x')) || ctx.pressed(KeyCode::Char('X')) {
            let c = &mut self.cells[cy][cx];
            *c = if *c == Cell::Marked {
                Cell::Blank
            } else {
                Cell::Marked
            };
        }
        // Clear the whole board.
        if ctx.pressed(KeyCode::Char('r')) || ctx.pressed(KeyCode::Char('R')) {
            self.cells = [[Cell::Blank; SIZE]; SIZE];
        }

        if self.is_solved() {
            self.won = true;
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let puzzle = &PUZZLES[self.puzzle];

        let row_clues = self.row_clues();
        let col_clues = self.col_clues();

        // Widest row-clue label decides the left gutter; tallest column clue
        // decides the top gutter.
        let row_label_w = row_clues
            .iter()
            .map(|s| s.chars().count())
            .max()
            .unwrap_or(0)
            .max(1) as u16;
        let col_depth = col_clues.iter().map(|c| c.len()).max().unwrap_or(1).max(1) as u16;

        // Each cell is two columns wide (like snake's "██").
        let grid_w = SIZE as u16 * 2;
        let inner_w = row_label_w + 1 + grid_w;
        let inner_h = col_depth + SIZE as u16;

        let title = format!(" Nonogram · {} ", puzzle.name);
        let field = centered(inner_w + 2, inner_h + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let blink_on = (self.blink.as_millis() / 400).is_multiple_of(2);
        let mut lines: Vec<Line> = Vec::with_capacity(inner_h as usize);

        // Column-clue header rows, bottom-aligned within `col_depth` lines.
        for depth in 0..col_depth as usize {
            let mut spans = Vec::with_capacity(SIZE + 1);
            spans.push(Span::raw(" ".repeat((row_label_w + 1) as usize)));
            for clue in col_clues.iter() {
                let pad = col_depth as usize - clue.len();
                let text = if depth >= pad {
                    format!("{:>2}", clue[depth - pad])
                } else {
                    "  ".to_string()
                };
                spans.push(Span::styled(text, Style::default().fg(Color::Cyan)));
            }
            lines.push(Line::from(spans));
        }

        // Grid rows, each prefixed by its right-aligned row clue.
        #[allow(clippy::needless_range_loop)]
        for y in 0..SIZE {
            let mut spans = Vec::with_capacity(SIZE + 1);
            spans.push(Span::styled(
                format!("{:>width$} ", row_clues[y], width = row_label_w as usize),
                Style::default().fg(Color::Cyan),
            ));
            for x in 0..SIZE {
                let is_cursor = self.cursor == (x, y);
                let span = match self.cells[y][x] {
                    Cell::Filled => {
                        let style = Style::default().fg(if is_cursor && blink_on {
                            Color::White
                        } else {
                            Color::LightGreen
                        });
                        Span::styled("██", style)
                    }
                    Cell::Marked => Span::styled(
                        if is_cursor && blink_on { "><" } else { "··" },
                        Style::default().fg(Color::DarkGray),
                    ),
                    Cell::Blank => {
                        if is_cursor && blink_on {
                            Span::styled("[]", Style::default().fg(Color::Yellow))
                        } else {
                            Span::styled("░░", Style::default().fg(Color::DarkGray))
                        }
                    }
                };
                spans.push(span);
            }
            lines.push(Line::from(spans));
        }

        frame.render_widget(Paragraph::new(lines), inner);

        // Controls hint just under the field.
        let hint = " ←↑↓→ move · Space fill · X mark · R clear · q menu ";
        let hint_area = Rect {
            x: area.x,
            y: field.y.saturating_add(field.height),
            width: area.width,
            height: 1,
        };
        if hint_area.y < area.y + area.height {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    hint,
                    Style::default().fg(Color::DarkGray),
                ))),
                hint_area,
            );
        }

        if self.won {
            let msg = format!(" SOLVED · {} · Enter: next puzzle · q: menu ", puzzle.name);
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

impl Nonogram {
    /// The picture is solved when every filled cell matches the solution and no
    /// extra cells are filled. Marks are ignored — only fills matter.
    fn is_solved(&self) -> bool {
        let solution = &PUZZLES[self.puzzle].solution;
        self.cells.iter().zip(solution.iter()).all(|(crow, srow)| {
            crow.iter()
                .zip(srow.iter())
                .all(|(&cell, &want)| (cell == Cell::Filled) == want)
        })
    }

    /// Run lengths for each row, as a display string (e.g. "2 1").
    fn row_clues(&self) -> Vec<String> {
        let solution = &PUZZLES[self.puzzle].solution;
        (0..SIZE)
            .map(|y| {
                let runs = runs(&(0..SIZE).map(|x| solution[y][x]).collect::<Vec<_>>());
                clue_string(&runs)
            })
            .collect()
    }

    /// Run lengths for each column, as a vector of numbers per column.
    fn col_clues(&self) -> Vec<Vec<usize>> {
        let solution = &PUZZLES[self.puzzle].solution;
        (0..SIZE)
            .map(|x| runs(&(0..SIZE).map(|y| solution[y][x]).collect::<Vec<_>>()))
            .collect()
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

/// Lengths of consecutive `true` runs in a line.
fn runs(line: &[bool]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut count = 0usize;
    for &b in line {
        if b {
            count += 1;
        } else if count > 0 {
            out.push(count);
            count = 0;
        }
    }
    if count > 0 {
        out.push(count);
    }
    out
}

/// Render a run list as a space-separated clue ("0" if the line is empty).
fn clue_string(runs: &[usize]) -> String {
    if runs.is_empty() {
        return "0".to_string();
    }
    runs.iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(" ")
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
    Nonogram,
    id: "nonogram",
    name: "Nonogram",
    description: "Solve the picture from row and column number clues.",
    author: "furybee",
}
