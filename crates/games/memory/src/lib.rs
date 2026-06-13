//! Memory — flip cards two at a time and find the matching pairs.
//!
//! Follows the same shape as the Snake reference: state, input handling,
//! `dt`-based timing (the brief "look at the mismatch" pause), a grid render,
//! a win overlay, and self-registration.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

const COLS: usize = 4;
const ROWS: usize = 4;
const CARDS: usize = COLS * ROWS; // 16 cards → 8 pairs
const PAIRS: usize = CARDS / 2;

/// How long a mismatched pair stays face-up before flipping back.
const FLIP_BACK: Duration = Duration::from_millis(750);

/// Card faces, each a distinct symbol + colour so pairs are easy to tell apart.
const FACES: [(&str, Color); PAIRS] = [
    ("A", Color::LightRed),
    ("B", Color::LightGreen),
    ("C", Color::LightYellow),
    ("D", Color::LightBlue),
    ("E", Color::LightMagenta),
    ("F", Color::LightCyan),
    ("G", Color::White),
    ("H", Color::Indexed(208)), // orange
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Face {
    Down,
    Up,
    Matched,
}

#[derive(Clone, Copy)]
struct Card {
    /// Index into `FACES`; the other card with the same value is its pair.
    sym: usize,
    face: Face,
}

pub struct Memory {
    cards: [Card; CARDS],
    /// Cursor position as (col, row).
    cursor: (usize, usize),
    /// Counts up while a mismatched pair is shown; flips them back at FLIP_BACK.
    mismatch: Option<Duration>,
    moves: u32,
    rng: u64,
}

impl Game for Memory {
    fn new() -> Self {
        let mut game = Memory {
            cards: [Card {
                sym: 0,
                face: Face::Down,
            }; CARDS],
            cursor: (0, 0),
            mismatch: None,
            moves: 0,
            rng: seed(),
        };
        game.deal();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        // Let a mismatched pair flip itself back after the pause.
        if let Some(t) = self.mismatch.as_mut() {
            *t += ctx.dt;
            if *t >= FLIP_BACK {
                self.hide_unmatched();
                self.mismatch = None;
            }
        }

        if self.won() {
            if ctx.pressed(KeyCode::Enter) {
                *self = Memory::new();
            }
            return Transition::Stay;
        }

        let (mut c, mut r) = self.cursor;
        if ctx.pressed(KeyCode::Left) && c > 0 {
            c -= 1;
        }
        if ctx.pressed(KeyCode::Right) && c + 1 < COLS {
            c += 1;
        }
        if ctx.pressed(KeyCode::Up) && r > 0 {
            r -= 1;
        }
        if ctx.pressed(KeyCode::Down) && r + 1 < ROWS {
            r += 1;
        }
        self.cursor = (c, r);

        if ctx.pressed(KeyCode::Enter) || ctx.pressed(KeyCode::Char(' ')) {
            if self.mismatch.is_some() {
                // Don't wait out the timer — resolve the pair and play on.
                self.hide_unmatched();
                self.mismatch = None;
            } else {
                self.flip();
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let found = self
            .cards
            .iter()
            .filter(|c| c.face == Face::Matched)
            .count()
            / 2;
        let title = format!(
            " Memory  ·  pairs {found}/{PAIRS}  ·  moves {} ",
            self.moves
        );

        // Card cell is 8 wide (6 inner + 2 border), 3 tall, with a 1-space gap.
        let board_w = (COLS * 8 + (COLS - 1)) as u16;
        let board_h = (ROWS * 3 + (ROWS - 1)) as u16;
        let field = centered(board_w + 2, board_h + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let mut lines: Vec<Line> = Vec::new();
        for r in 0..ROWS {
            let mut top = Vec::new();
            let mut mid = Vec::new();
            let mut bot = Vec::new();
            for c in 0..COLS {
                let card = self.cards[r * COLS + c];
                let under_cursor = self.cursor == (c, r);

                let border_style = if under_cursor {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else if card.face == Face::Matched {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::Gray)
                };

                let content = match card.face {
                    Face::Down => Span::styled("▒▒▒▒▒▒", Style::default().fg(Color::DarkGray)),
                    Face::Up => {
                        let (sym, color) = FACES[card.sym];
                        Span::styled(
                            format!("  {sym}   "),
                            Style::default().fg(color).add_modifier(Modifier::BOLD),
                        )
                    }
                    Face::Matched => {
                        let (sym, color) = FACES[card.sym];
                        Span::styled(
                            format!("  {sym}   "),
                            Style::default().fg(color).add_modifier(Modifier::DIM),
                        )
                    }
                };

                top.push(Span::styled("┌──────┐", border_style));
                mid.push(Span::styled("│", border_style));
                mid.push(content);
                mid.push(Span::styled("│", border_style));
                bot.push(Span::styled("└──────┘", border_style));

                if c + 1 < COLS {
                    top.push(Span::raw(" "));
                    mid.push(Span::raw(" "));
                    bot.push(Span::raw(" "));
                }
            }
            lines.push(Line::from(top));
            lines.push(Line::from(mid));
            lines.push(Line::from(bot));
            if r + 1 < ROWS {
                lines.push(Line::raw(""));
            }
        }
        frame.render_widget(Paragraph::new(lines), inner);

        if self.won() {
            let msg = format!(" YOU WIN · {} moves · Enter: replay · q: menu ", self.moves);
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

impl Memory {
    /// Build a shuffled deck of paired faces.
    fn deal(&mut self) {
        for (i, card) in self.cards.iter_mut().enumerate() {
            card.sym = i / 2; // 0,0,1,1,2,2,...
            card.face = Face::Down;
        }
        // Fisher–Yates shuffle of the cards.
        for i in (1..CARDS).rev() {
            let j = (self.next_rand() % (i as u64 + 1)) as usize;
            self.cards.swap(i, j);
        }
    }

    /// Reveal the card under the cursor; resolve the pair once two are up.
    fn flip(&mut self) {
        let (c, r) = self.cursor;
        let i = r * COLS + c;
        if self.cards[i].face != Face::Down {
            return; // already face-up or already matched
        }
        self.cards[i].face = Face::Up;

        let up: Vec<usize> = (0..CARDS)
            .filter(|&k| self.cards[k].face == Face::Up)
            .collect();
        if up.len() == 2 {
            self.moves += 1;
            if self.cards[up[0]].sym == self.cards[up[1]].sym {
                self.cards[up[0]].face = Face::Matched;
                self.cards[up[1]].face = Face::Matched;
            } else {
                self.mismatch = Some(Duration::ZERO);
            }
        }
    }

    /// Turn every still-face-up card back over (used to clear a mismatch).
    fn hide_unmatched(&mut self) {
        for card in self.cards.iter_mut() {
            if card.face == Face::Up {
                card.face = Face::Down;
            }
        }
    }

    fn won(&self) -> bool {
        self.cards.iter().all(|c| c.face == Face::Matched)
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
    Memory,
    id: "memory",
    name: "Memory",
    description: "Flip cards two at a time and match the pairs.",
    author: "furybee",
}
