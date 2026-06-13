//! Minesweeper — clear every safe cell without uncovering a mine.
//!
//! Follows the same pattern as the Snake reference game: self-contained state,
//! input via `ctx.pressed`, `dt`-accumulated timing for the clock, a grid
//! render with a game-over overlay, and self-registration.
//!
//! Controls: arrows / WASD move the cursor, Space or Enter reveals a cell,
//! `f` toggles a flag, Enter replays after the game ends, `q` / `Esc` quits.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

const WIDTH: u16 = 16;
const HEIGHT: u16 = 16;
const MINES: u32 = 40;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Playing,
    Won,
    Lost,
}

#[derive(Clone, Copy, Default)]
struct Cell {
    mine: bool,
    revealed: bool,
    flagged: bool,
    /// Number of mines in the eight neighbours.
    adjacent: u8,
}

pub struct Minesweeper {
    cells: Vec<Cell>,
    cursor: (u16, u16),
    phase: Phase,
    /// Mines are placed on the first reveal so the first click is always safe.
    seeded: bool,
    flags: u32,
    /// Clock, frozen once the game ends; starts on the first reveal.
    clock: Duration,
    rng: u64,
}

impl Game for Minesweeper {
    fn new() -> Self {
        Minesweeper {
            cells: vec![Cell::default(); WIDTH as usize * HEIGHT as usize],
            cursor: (WIDTH / 2, HEIGHT / 2),
            phase: Phase::Playing,
            seeded: false,
            flags: 0,
            clock: Duration::ZERO,
            rng: seed(),
        }
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.phase != Phase::Playing {
            if ctx.pressed(KeyCode::Enter) {
                *self = Minesweeper::new();
            }
            return Transition::Stay;
        }

        // The clock runs once the player has made their first reveal.
        if self.seeded {
            self.clock += ctx.dt;
        }

        let (mut cx, mut cy) = self.cursor;
        if ctx.pressed(KeyCode::Up) || ctx.pressed(KeyCode::Char('w')) {
            cy = cy.saturating_sub(1);
        }
        if ctx.pressed(KeyCode::Down) || ctx.pressed(KeyCode::Char('s')) {
            cy = (cy + 1).min(HEIGHT - 1);
        }
        if ctx.pressed(KeyCode::Left) || ctx.pressed(KeyCode::Char('a')) {
            cx = cx.saturating_sub(1);
        }
        if ctx.pressed(KeyCode::Right) || ctx.pressed(KeyCode::Char('d')) {
            cx = (cx + 1).min(WIDTH - 1);
        }
        self.cursor = (cx, cy);

        if ctx.pressed(KeyCode::Char('f')) {
            self.toggle_flag(cx, cy);
        }

        if ctx.pressed(KeyCode::Char(' ')) || ctx.pressed(KeyCode::Enter) {
            self.reveal(cx, cy);
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let remaining = MINES as i64 - self.flags as i64;
        let secs = self.clock.as_secs();
        let title = format!(
            " Minesweeper  ·  mines {remaining:>3}  ·  {:02}:{:02} ",
            secs / 60,
            secs % 60,
        );
        let field = centered(WIDTH * 2 + 2, HEIGHT + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let mut lines = Vec::with_capacity(HEIGHT as usize);
        for y in 0..HEIGHT {
            let mut spans = Vec::with_capacity(WIDTH as usize);
            for x in 0..WIDTH {
                spans.push(self.cell_span(x, y));
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        if self.phase != Phase::Playing {
            let msg = match self.phase {
                Phase::Won => format!(
                    " YOU WIN · {:02}:{:02} · Enter: replay · q: menu ",
                    secs / 60,
                    secs % 60
                ),
                _ => " BOOM! · Enter: replay · q: menu ".to_string(),
            };
            let color = if self.phase == Phase::Won {
                Color::LightGreen
            } else {
                Color::Red
            };
            let overlay = centered(msg.chars().count() as u16 + 2, 3, area);
            frame.render_widget(Clear, overlay);
            frame.render_widget(
                Paragraph::new(msg)
                    .block(Block::default().borders(Borders::ALL))
                    .style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
                overlay,
            );
        }
    }

    fn tick_rate(&self) -> Duration {
        Duration::from_millis(30)
    }
}

impl Minesweeper {
    fn idx(x: u16, y: u16) -> usize {
        y as usize * WIDTH as usize + x as usize
    }

    fn cell(&self, x: u16, y: u16) -> &Cell {
        &self.cells[Self::idx(x, y)]
    }

    /// The two-character glyph plus styling for one board cell.
    fn cell_span(&self, x: u16, y: u16) -> Span<'static> {
        let cell = self.cell(x, y);
        let is_cursor = self.cursor == (x, y);

        let (text, mut style): (String, Style) = if cell.flagged {
            ("⚑ ".to_string(), Style::default().fg(Color::Red))
        } else if !cell.revealed {
            // After a loss, expose every mine that wasn't flagged.
            if self.phase == Phase::Lost && cell.mine {
                ("✸ ".to_string(), Style::default().fg(Color::Red))
            } else {
                ("░░".to_string(), Style::default().fg(Color::DarkGray))
            }
        } else if cell.mine {
            ("✸ ".to_string(), Style::default().fg(Color::Red))
        } else if cell.adjacent == 0 {
            ("  ".to_string(), Style::default())
        } else {
            (
                format!("{} ", cell.adjacent),
                Style::default().fg(number_color(cell.adjacent)),
            )
        };

        if is_cursor {
            style = style.add_modifier(Modifier::REVERSED);
        }
        Span::styled(text, style)
    }

    fn toggle_flag(&mut self, x: u16, y: u16) {
        let cell = &mut self.cells[Self::idx(x, y)];
        if cell.revealed {
            return;
        }
        cell.flagged = !cell.flagged;
        if cell.flagged {
            self.flags += 1;
        } else {
            self.flags -= 1;
        }
    }

    fn reveal(&mut self, x: u16, y: u16) {
        if !self.seeded {
            self.place_mines(x, y);
            self.seeded = true;
        }

        if self.cell(x, y).revealed || self.cell(x, y).flagged {
            return;
        }

        if self.cell(x, y).mine {
            self.cells[Self::idx(x, y)].revealed = true;
            self.phase = Phase::Lost;
            return;
        }

        // Flood fill from the clicked cell, opening up empty regions.
        let mut stack = vec![(x, y)];
        while let Some((cx, cy)) = stack.pop() {
            let i = Self::idx(cx, cy);
            if self.cells[i].revealed || self.cells[i].flagged {
                continue;
            }
            self.cells[i].revealed = true;
            if self.cells[i].adjacent == 0 {
                for (nx, ny) in neighbours(cx, cy) {
                    if !self.cells[Self::idx(nx, ny)].revealed {
                        stack.push((nx, ny));
                    }
                }
            }
        }

        if self.won() {
            self.phase = Phase::Won;
        }
    }

    /// Place `MINES` mines uniformly, never on the first-clicked cell, then
    /// compute each safe cell's adjacency count.
    fn place_mines(&mut self, safe_x: u16, safe_y: u16) {
        let total = WIDTH as u32 * HEIGHT as u32;
        let mines = MINES.min(total - 1);
        let safe = Self::idx(safe_x, safe_y);
        let mut placed = 0;
        while placed < mines {
            let i = (self.next_rand() % total as u64) as usize;
            if i == safe || self.cells[i].mine {
                continue;
            }
            self.cells[i].mine = true;
            placed += 1;
        }

        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                if self.cell(x, y).mine {
                    continue;
                }
                let count = neighbours(x, y)
                    .filter(|&(nx, ny)| self.cell(nx, ny).mine)
                    .count() as u8;
                self.cells[Self::idx(x, y)].adjacent = count;
            }
        }
    }

    /// Won when every non-mine cell has been revealed.
    fn won(&self) -> bool {
        self.cells.iter().all(|c| c.mine || c.revealed)
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

/// The up-to-eight in-bounds neighbours of `(x, y)`.
fn neighbours(x: u16, y: u16) -> impl Iterator<Item = (u16, u16)> {
    const OFFSETS: [(i32, i32); 8] = [
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ];
    OFFSETS.iter().filter_map(move |&(dx, dy)| {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx >= 0 && nx < WIDTH as i32 && ny >= 0 && ny < HEIGHT as i32 {
            Some((nx as u16, ny as u16))
        } else {
            None
        }
    })
}

/// Classic Minesweeper colour per adjacency count.
fn number_color(n: u8) -> Color {
    match n {
        1 => Color::LightBlue,
        2 => Color::LightGreen,
        3 => Color::LightRed,
        4 => Color::Blue,
        5 => Color::Red,
        6 => Color::Cyan,
        7 => Color::Magenta,
        _ => Color::White,
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
    Minesweeper,
    id: "minesweeper",
    name: "Minesweeper",
    description: "Clear the field without detonating a mine.",
    author: "furybee",
}
