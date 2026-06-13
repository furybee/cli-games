//! 15-Puzzle — a 4x4 sliding tile game.
//!
//! Tiles 1..=15 plus one blank. The arrow keys slide the tile adjacent to the
//! blank into the gap. The board starts from a guaranteed-solvable shuffle; we
//! count moves and detect the solved ordering.
//!
//! Mirrors the structure of the `snake` reference crate: state struct, input
//! handling, the `centered` helper, the xorshift `next_rand`/`seed` pair, and a
//! win overlay (`Clear` + bordered `Paragraph`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Board side length (4 → the classic 15-puzzle).
const SIZE: usize = 4;
/// Number of cells on the board.
const CELLS: usize = SIZE * SIZE;
/// How many random legal slides to apply when shuffling. Starting from the
/// solved state and only making legal moves guarantees the result is solvable.
const SHUFFLE_MOVES: usize = 200;

/// Rendered width of one tile cell (including its leading pad), in columns.
const TILE_W: u16 = 5;
/// Rendered height of one tile cell, in rows.
const TILE_H: u16 = 2;

pub struct Slidepuzzle {
    /// Tiles row-major; `0` marks the blank. Solved state is `[1,2,..,15,0]`.
    board: [u8; CELLS],
    /// Index of the blank within `board`.
    blank: usize,
    moves: u32,
    won: bool,
    rng: u64,
}

impl Game for Slidepuzzle {
    fn new() -> Self {
        let mut game = Slidepuzzle {
            board: solved_board(),
            blank: CELLS - 1,
            moves: 0,
            won: false,
            rng: seed(),
        };
        game.shuffle();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.won {
            if ctx.pressed(KeyCode::Enter) {
                *self = Slidepuzzle::new();
            }
            return Transition::Stay;
        }

        // Arrow keys describe the direction the player wants a tile to travel
        // *into* the blank, which is the opposite of moving the blank itself.
        let (br, bc) = (self.blank / SIZE, self.blank % SIZE);
        let target = if ctx.pressed(KeyCode::Up) {
            // Tile below the blank slides up.
            (br + 1 < SIZE).then(|| (br + 1) * SIZE + bc)
        } else if ctx.pressed(KeyCode::Down) {
            // Tile above the blank slides down.
            (br > 0).then(|| (br - 1) * SIZE + bc)
        } else if ctx.pressed(KeyCode::Left) {
            // Tile to the right of the blank slides left.
            (bc + 1 < SIZE).then(|| br * SIZE + bc + 1)
        } else if ctx.pressed(KeyCode::Right) {
            // Tile to the left of the blank slides right.
            (bc > 0).then(|| br * SIZE + bc - 1)
        } else {
            None
        };

        if let Some(tile) = target {
            self.slide(tile);
            self.moves += 1;
            self.won = self.is_solved();
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(" 15-Puzzle  ·  moves {} ", self.moves);
        let field = centered(SIZE as u16 * TILE_W + 3, SIZE as u16 * TILE_H + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let mut lines = Vec::with_capacity(SIZE * TILE_H as usize);
        for row in 0..SIZE {
            // Each logical row spans `TILE_H` text rows; the label sits on the
            // second row so tiles read as small blocks rather than bare numbers.
            for sub in 0..TILE_H {
                let mut spans = Vec::with_capacity(SIZE);
                for col in 0..SIZE {
                    let value = self.board[row * SIZE + col];
                    spans.push(tile_span(value, sub));
                }
                lines.push(Line::from(spans));
            }
        }
        frame.render_widget(Paragraph::new(lines), inner);

        let hint = " ←↑↓→ slide · Enter restart · q menu ";
        let hint_rect = Rect {
            x: field.x,
            y: field.y.saturating_add(field.height),
            width: hint.len() as u16,
            height: 1,
        };
        if hint_rect.y < area.y.saturating_add(area.height) {
            frame.render_widget(
                Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
                hint_rect,
            );
        }

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

impl Slidepuzzle {
    /// Swap the tile at `idx` with the blank, then update the blank position.
    fn slide(&mut self, idx: usize) {
        self.board.swap(idx, self.blank);
        self.blank = idx;
    }

    /// Indices of the cells orthogonally adjacent to the blank.
    fn neighbours(&self) -> Vec<usize> {
        let (r, c) = (self.blank / SIZE, self.blank % SIZE);
        let mut out = Vec::with_capacity(4);
        if r > 0 {
            out.push((r - 1) * SIZE + c);
        }
        if r + 1 < SIZE {
            out.push((r + 1) * SIZE + c);
        }
        if c > 0 {
            out.push(r * SIZE + c - 1);
        }
        if c + 1 < SIZE {
            out.push(r * SIZE + c + 1);
        }
        out
    }

    /// Scramble by applying only legal slides, so the puzzle stays solvable.
    fn shuffle(&mut self) {
        let mut last = usize::MAX;
        let mut applied = 0;
        while applied < SHUFFLE_MOVES {
            let options: Vec<usize> = self
                .neighbours()
                .into_iter()
                .filter(|&n| n != last)
                .collect();
            if options.is_empty() {
                break;
            }
            let pick = options[(self.next_rand() as usize) % options.len()];
            last = self.blank;
            self.slide(pick);
            applied += 1;
        }
        // A scramble can coincidentally land on the solved board; nudge once if so.
        if self.is_solved() {
            let options = self.neighbours();
            if let Some(&pick) = options.first() {
                self.slide(pick);
            }
        }
    }

    /// `true` when tiles read `1..=15` followed by the blank.
    fn is_solved(&self) -> bool {
        self.board == solved_board()
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

/// The goal arrangement: `[1, 2, .., 15, 0]`.
fn solved_board() -> [u8; CELLS] {
    let mut board = [0u8; CELLS];
    for (i, slot) in board.iter_mut().enumerate().take(CELLS - 1) {
        *slot = i as u8 + 1;
    }
    board
}

/// Render one `TILE_W`-wide cell for `value` on text sub-row `sub`.
fn tile_span(value: u8, sub: u16) -> Span<'static> {
    if value == 0 {
        return Span::raw("     ");
    }
    if sub == 0 {
        // Top half of the tile: solid colour bar, no label.
        return Span::styled("████ ", Style::default().fg(Color::Blue));
    }
    // Bottom half carries the number, right-aligned in a 4-wide field.
    let label = format!("{value:>3} ");
    let colour = if value.is_multiple_of(2) {
        Color::LightCyan
    } else {
        Color::LightGreen
    };
    Span::styled(
        label,
        Style::default().fg(colour).add_modifier(Modifier::BOLD),
    )
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
    Slidepuzzle,
    id: "slidepuzzle",
    name: "15-Puzzle",
    description: "Slide the numbered tiles back into order.",
    author: "furybee",
}
