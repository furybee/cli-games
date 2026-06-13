//! 2048 — slide the tiles, merge equal numbers, try to reach 2048.
//!
//! Input-driven rather than time-driven: each arrow key performs one slide.
//! Follows the same pattern as the Snake reference (state, input, grid render,
//! overlay, self-registration), but with no `dt` accumulator since nothing
//! moves on its own.

use std::time::{SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// The board is always 4×4 — the classic size.
const SIZE: usize = 4;
/// Width of a rendered tile cell, in terminal columns.
const CELL_W: u16 = 6;
/// Height of a rendered tile cell, in terminal rows.
const CELL_H: u16 = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

pub struct Game2048 {
    /// Row-major grid; 0 means empty, otherwise the tile's face value.
    grid: [[u32; SIZE]; SIZE],
    score: u32,
    /// Set once a 2048 tile appears; the player may keep going.
    won: bool,
    /// No legal move remains.
    dead: bool,
    rng: u64,
}

impl Game for Game2048 {
    fn new() -> Self {
        let mut game = Game2048 {
            grid: [[0; SIZE]; SIZE],
            score: 0,
            won: false,
            dead: false,
            rng: seed(),
        };
        game.spawn_tile();
        game.spawn_tile();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.dead {
            if ctx.pressed(KeyCode::Enter) {
                *self = Game2048::new();
            }
            return Transition::Stay;
        }

        // One key press = one slide. Spawn a fresh tile only when the board
        // actually changed, then re-check whether any move is still possible.
        for &(key, dir) in &[
            (KeyCode::Up, Dir::Up),
            (KeyCode::Down, Dir::Down),
            (KeyCode::Left, Dir::Left),
            (KeyCode::Right, Dir::Right),
        ] {
            if ctx.pressed(key) && self.slide(dir) {
                self.spawn_tile();
                if !self.has_move() {
                    self.dead = true;
                }
                break;
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(" 2048  ·  score {} ", self.score);
        let board_w = CELL_W * SIZE as u16 + 2;
        let board_h = CELL_H * SIZE as u16 + 2;
        let field = centered(board_w, board_h, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        // Each tile occupies a CELL_W × CELL_H box; the value is centred
        // vertically and horizontally inside it.
        let mut lines = Vec::with_capacity((CELL_H * SIZE as u16) as usize);
        for row in 0..SIZE {
            for sub in 0..CELL_H {
                let mut spans = Vec::with_capacity(SIZE);
                for col in 0..SIZE {
                    let value = self.grid[row][col];
                    let style = tile_style(value);
                    let text = if sub == CELL_H / 2 {
                        let label = if value == 0 {
                            String::new()
                        } else {
                            value.to_string()
                        };
                        center_pad(&label, CELL_W as usize)
                    } else {
                        " ".repeat(CELL_W as usize)
                    };
                    spans.push(Span::styled(text, style));
                }
                lines.push(Line::from(spans));
            }
        }
        frame.render_widget(Paragraph::new(lines), inner);

        if self.dead {
            self.overlay(
                frame,
                area,
                &format!(
                    " GAME OVER · score {} · Enter: replay · q: menu ",
                    self.score
                ),
                Color::Red,
            );
        } else if self.won {
            self.overlay(
                frame,
                area,
                " 2048! · keep sliding · q: menu ",
                Color::Yellow,
            );
        }
    }
}

impl Game2048 {
    /// Slide and merge all tiles toward `dir`. Returns `true` if anything moved
    /// or merged, which is what decides whether to spawn a new tile.
    fn slide(&mut self, dir: Dir) -> bool {
        let before = self.grid;
        // Reduce every direction to "slide toward the front" by extracting each
        // line in travel order, collapsing it, then writing it back.
        for i in 0..SIZE {
            let mut line = self.read_line(dir, i);
            self.collapse(&mut line);
            self.write_line(dir, i, &line);
        }
        self.grid != before
    }

    /// Pull the `i`-th line out of the grid, ordered front-to-back along `dir`
    /// (front = the edge the tiles slide toward).
    fn read_line(&self, dir: Dir, i: usize) -> [u32; SIZE] {
        let mut line = [0; SIZE];
        for (j, slot) in line.iter_mut().enumerate() {
            *slot = match dir {
                Dir::Left => self.grid[i][j],
                Dir::Right => self.grid[i][SIZE - 1 - j],
                Dir::Up => self.grid[j][i],
                Dir::Down => self.grid[SIZE - 1 - j][i],
            };
        }
        line
    }

    /// Inverse of [`read_line`] — store a collapsed line back into the grid.
    fn write_line(&mut self, dir: Dir, i: usize, line: &[u32; SIZE]) {
        for (j, &v) in line.iter().enumerate() {
            match dir {
                Dir::Left => self.grid[i][j] = v,
                Dir::Right => self.grid[i][SIZE - 1 - j] = v,
                Dir::Up => self.grid[j][i] = v,
                Dir::Down => self.grid[SIZE - 1 - j][i] = v,
            }
        }
    }

    /// Collapse a line toward its front: drop gaps, then merge each adjacent
    /// equal pair once (front-to-back), accumulating score along the way.
    fn collapse(&mut self, line: &mut [u32; SIZE]) {
        let tiles: Vec<u32> = line.iter().copied().filter(|&v| v != 0).collect();
        let mut merged = Vec::with_capacity(SIZE);
        let mut k = 0;
        while k < tiles.len() {
            if k + 1 < tiles.len() && tiles[k] == tiles[k + 1] {
                let sum = tiles[k] * 2;
                merged.push(sum);
                self.score += sum;
                if sum >= 2048 {
                    self.won = true;
                }
                k += 2;
            } else {
                merged.push(tiles[k]);
                k += 1;
            }
        }
        merged.resize(SIZE, 0);
        line.copy_from_slice(&merged);
    }

    /// Put a new tile (2 with 90% chance, else 4) on a random empty cell.
    fn spawn_tile(&mut self) {
        let empties: Vec<(usize, usize)> = (0..SIZE)
            .flat_map(|r| (0..SIZE).map(move |c| (r, c)))
            .filter(|&(r, c)| self.grid[r][c] == 0)
            .collect();
        if empties.is_empty() {
            return;
        }
        let (r, c) = empties[(self.next_rand() as usize) % empties.len()];
        self.grid[r][c] = if self.next_rand().is_multiple_of(10) {
            4
        } else {
            2
        };
    }

    /// `true` if any slide would change the board (empty cell or mergeable pair).
    fn has_move(&self) -> bool {
        for r in 0..SIZE {
            for c in 0..SIZE {
                let v = self.grid[r][c];
                if v == 0 {
                    return true;
                }
                if c + 1 < SIZE && self.grid[r][c + 1] == v {
                    return true;
                }
                if r + 1 < SIZE && self.grid[r + 1][c] == v {
                    return true;
                }
            }
        }
        false
    }

    /// Draw a centred message box over the board.
    fn overlay(&self, frame: &mut Frame, area: Rect, msg: &str, color: Color) {
        let overlay = centered(msg.chars().count() as u16 + 2, 3, area);
        frame.render_widget(Clear, overlay);
        frame.render_widget(
            Paragraph::new(msg)
                .block(Block::default().borders(Borders::ALL))
                .style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
            overlay,
        );
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

/// Tile colours scale with the value so bigger numbers pop visually.
fn tile_style(value: u32) -> Style {
    let bg = match value {
        0 => Color::Rgb(40, 40, 40),
        2 => Color::Rgb(120, 110, 100),
        4 => Color::Rgb(130, 120, 90),
        8 => Color::Rgb(200, 140, 80),
        16 => Color::Rgb(210, 120, 70),
        32 => Color::Rgb(220, 100, 70),
        64 => Color::Rgb(230, 80, 60),
        128 => Color::Rgb(220, 200, 110),
        256 => Color::Rgb(225, 200, 90),
        512 => Color::Rgb(230, 200, 70),
        1024 => Color::Rgb(235, 200, 50),
        _ => Color::Rgb(240, 200, 30),
    };
    let fg = if value <= 4 {
        Color::Gray
    } else {
        Color::Black
    };
    Style::default().bg(bg).fg(fg).add_modifier(Modifier::BOLD)
}

/// Centre `s` inside a field `width` columns wide.
fn center_pad(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        return s.to_string();
    }
    let left = (width - len) / 2;
    let right = width - len - left;
    format!("{}{}{}", " ".repeat(left), s, " ".repeat(right))
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
    Game2048,
    id: "2048",
    name: "2048",
    description: "Slide tiles, merge equal numbers, reach 2048.",
    author: "furybee",
}
