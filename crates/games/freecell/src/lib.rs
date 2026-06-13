//! FreeCell — classic solitaire for the terminal.
//!
//! Built on the same skeleton as the `snake` reference crate: xorshift RNG
//! seeded from the clock, a `centered` layout helper, a bordered game-over
//! (here: win) overlay, and self-registration via `register_game!`.
//!
//! Layout: 4 free cells + 4 foundations on the top row, 8 cascades below.
//! Move with the arrow keys, press Enter/Space to pick a source pile then a
//! destination; moves are validated against the standard FreeCell rules
//! (descending alternating colours on cascades, single cards into free cells,
//! ascending by suit on foundations).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

const NUM_FREE: usize = 4;
const NUM_FOUND: usize = 4;
const NUM_CASCADE: usize = 8;
/// Total selectable piles: free cells, foundations, then cascades.
const NUM_PILES: usize = NUM_FREE + NUM_FOUND + NUM_CASCADE;

/// A playing card. `rank` is 1..=13 (Ace..King); `suit` is 0..=3.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Card {
    rank: u8,
    suit: u8,
}

impl Card {
    /// Hearts and Diamonds are red; Clubs and Spades are black.
    fn is_red(self) -> bool {
        self.suit == 0 || self.suit == 1
    }

    fn suit_char(self) -> &'static str {
        match self.suit {
            0 => "♥",
            1 => "♦",
            2 => "♣",
            _ => "♠",
        }
    }

    fn rank_str(self) -> &'static str {
        match self.rank {
            1 => "A",
            2 => "2",
            3 => "3",
            4 => "4",
            5 => "5",
            6 => "6",
            7 => "7",
            8 => "8",
            9 => "9",
            10 => "10",
            11 => "J",
            12 => "Q",
            _ => "K",
        }
    }

    fn color(self) -> Color {
        if self.is_red() {
            Color::Red
        } else {
            Color::White
        }
    }
}

pub struct Freecell {
    free: [Option<Card>; NUM_FREE],
    foundations: [Vec<Card>; NUM_FOUND],
    cascades: [Vec<Card>; NUM_CASCADE],
    /// Index of the highlighted pile, 0..NUM_PILES (see pile-index helpers).
    cursor: usize,
    /// The pile a card has been picked up from, awaiting a destination.
    selected: Option<usize>,
    won: bool,
    /// Number of legal moves the player has made.
    moves: u32,
    rng: u64,
}

impl Game for Freecell {
    fn new() -> Self {
        let mut game = Freecell {
            free: [None; NUM_FREE],
            foundations: Default::default(),
            cascades: Default::default(),
            cursor: free_to_pile(0),
            selected: None,
            won: false,
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

        if self.won {
            if ctx.pressed(KeyCode::Enter) {
                *self = Freecell::new();
            }
            return Transition::Stay;
        }

        // Restart at any time with 'r'.
        if ctx.pressed(KeyCode::Char('r')) {
            *self = Freecell::new();
            return Transition::Stay;
        }

        if ctx.pressed(KeyCode::Left) {
            self.move_cursor(-1, 0);
        }
        if ctx.pressed(KeyCode::Right) {
            self.move_cursor(1, 0);
        }
        if ctx.pressed(KeyCode::Up) {
            self.move_cursor(0, -1);
        }
        if ctx.pressed(KeyCode::Down) {
            self.move_cursor(0, 1);
        }

        if ctx.pressed(KeyCode::Enter) || ctx.pressed(KeyCode::Char(' ')) {
            self.activate();
        }

        // Quick "send card under cursor to a foundation".
        if ctx.pressed(KeyCode::Char('f')) {
            self.auto_to_foundation();
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        // 11 cards tall is plenty for an opening cascade plus headers.
        let field = centered(70, 22, area);
        let title = format!(" FreeCell  ·  moves {} ", self.moves);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let mut lines: Vec<Line> = Vec::new();

        // --- Top row: free cells and foundations. ---
        let mut top: Vec<Span> = Vec::new();
        top.push(Span::styled("Free: ", Style::default().fg(Color::DarkGray)));
        for i in 0..NUM_FREE {
            top.extend(self.slot_spans(free_to_pile(i), self.free[i]));
            top.push(Span::raw(" "));
        }
        top.push(Span::styled(
            "  Found: ",
            Style::default().fg(Color::DarkGray),
        ));
        for i in 0..NUM_FOUND {
            let card = self.foundations[i].last().copied();
            top.extend(self.slot_spans(found_to_pile(i), card));
            top.push(Span::raw(" "));
        }
        lines.push(Line::from(top));
        lines.push(Line::from(""));

        // --- Cascade headers (column numbers / cursor markers). ---
        let mut header: Vec<Span> = Vec::new();
        for i in 0..NUM_CASCADE {
            let pile = cascade_to_pile(i);
            let here = pile == self.cursor;
            let sel = Some(pile) == self.selected;
            let label = format!(" {} ", i + 1);
            let style = if sel {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if here {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            header.push(Span::styled(label, style));
            header.push(Span::raw("  "));
        }
        lines.push(Line::from(header));

        // --- Cascade bodies, row by row. ---
        let max_len = self
            .cascades
            .iter()
            .map(|c| c.len())
            .max()
            .unwrap_or(0)
            .max(1);
        for row in 0..max_len {
            let mut spans: Vec<Span> = Vec::new();
            for i in 0..NUM_CASCADE {
                let cascade = &self.cascades[i];
                if row < cascade.len() {
                    let card = cascade[row];
                    let bottom = row + 1 == cascade.len();
                    spans.push(card_span(card, bottom));
                } else {
                    spans.push(Span::raw("   "));
                }
                spans.push(Span::raw("  "));
            }
            lines.push(Line::from(spans));
        }

        lines.push(Line::from(""));
        let hint = if self.selected.is_some() {
            "←→↑↓ move · Enter: drop here · f: to foundation · r: restart · q: menu"
        } else {
            "←→↑↓ move · Enter: pick up · f: to foundation · r: restart · q: menu"
        };
        lines.push(Line::from(Span::styled(
            hint,
            Style::default().fg(Color::DarkGray),
        )));

        frame.render_widget(Paragraph::new(lines), inner);

        if self.won {
            let msg = format!(
                " YOU WIN! · {} moves · Enter: replay · q: menu ",
                self.moves
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
        Duration::from_millis(30)
    }
}

impl Freecell {
    /// Shuffle a fresh 52-card deck and deal it across the eight cascades.
    fn deal(&mut self) {
        let mut deck: Vec<Card> = Vec::with_capacity(52);
        for suit in 0..4u8 {
            for rank in 1..=13u8 {
                deck.push(Card { rank, suit });
            }
        }
        // Fisher–Yates with the xorshift RNG.
        for i in (1..deck.len()).rev() {
            let j = (self.next_rand() % (i as u64 + 1)) as usize;
            deck.swap(i, j);
        }
        for (idx, card) in deck.into_iter().enumerate() {
            self.cascades[idx % NUM_CASCADE].push(card);
        }
    }

    /// Render the two-character spans for a top-row slot (free cell / foundation).
    fn slot_spans(&self, pile: usize, card: Option<Card>) -> Vec<Span<'static>> {
        let here = pile == self.cursor;
        let sel = Some(pile) == self.selected;
        let style = if sel {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else if here {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        match card {
            Some(c) => {
                let inner = format!("{}{}", c.rank_str(), c.suit_char());
                let text = format!("[{:>3}]", inner);
                vec![Span::styled(
                    text,
                    if sel || here {
                        style
                    } else {
                        Style::default().fg(c.color())
                    },
                )]
            }
            None => vec![Span::styled("[   ]", style)],
        }
    }

    /// Move the cursor in a grid-like fashion. The top row holds the 8 free
    /// cells + foundations; the bottom holds the 8 cascades.
    fn move_cursor(&mut self, dx: i32, dy: i32) {
        let top = self.cursor < NUM_FREE + NUM_FOUND;
        if dy != 0 {
            // Toggle between the top row and the cascades, keeping the column.
            if dy > 0 && top {
                let col = self.cursor.min(NUM_CASCADE - 1);
                self.cursor = cascade_to_pile(col);
            } else if dy < 0 && !top {
                let col = (self.cursor - (NUM_FREE + NUM_FOUND)).min(NUM_FREE + NUM_FOUND - 1);
                self.cursor = col;
            }
            return;
        }
        if dx != 0 {
            let (lo, hi) = if top {
                (0, NUM_FREE + NUM_FOUND)
            } else {
                (NUM_FREE + NUM_FOUND, NUM_PILES)
            };
            let span = hi - lo;
            let rel = self.cursor - lo;
            let next = (rel as i32 + dx).rem_euclid(span as i32) as usize;
            self.cursor = lo + next;
        }
    }

    /// Pick up from / drop onto the pile under the cursor.
    fn activate(&mut self) {
        match self.selected {
            None => {
                // Only select a pile that actually has a movable card.
                if self.top_card(self.cursor).is_some() {
                    self.selected = Some(self.cursor);
                }
            }
            Some(src) => {
                if src == self.cursor {
                    // Cancel selection.
                    self.selected = None;
                } else if self.try_move(src, self.cursor) {
                    self.selected = None;
                    self.moves += 1;
                    self.check_win();
                } else {
                    // Illegal: re-target selection to the new pile if it holds
                    // a card, otherwise just cancel.
                    self.selected = if self.top_card(self.cursor).is_some() {
                        Some(self.cursor)
                    } else {
                        None
                    };
                }
            }
        }
    }

    /// Try to drop the card under the cursor onto the first valid foundation.
    fn auto_to_foundation(&mut self) {
        let src = self.cursor;
        if self.top_card(src).is_none() {
            return;
        }
        for i in 0..NUM_FOUND {
            let dst = found_to_pile(i);
            if dst != src && self.try_move(src, dst) {
                self.selected = None;
                self.moves += 1;
                self.check_win();
                return;
            }
        }
    }

    /// The top (movable) card of any pile, if present.
    fn top_card(&self, pile: usize) -> Option<Card> {
        if let Some(i) = pile_as_free(pile) {
            self.free[i]
        } else if let Some(i) = pile_as_found(pile) {
            self.foundations[i].last().copied()
        } else if let Some(i) = pile_as_cascade(pile) {
            self.cascades[i].last().copied()
        } else {
            None
        }
    }

    /// Attempt to move the top card of `src` onto `dst`, enforcing the rules.
    /// Returns whether the move was performed.
    fn try_move(&mut self, src: usize, dst: usize) -> bool {
        let card = match self.top_card(src) {
            Some(c) => c,
            None => return false,
        };

        let allowed = if let Some(i) = pile_as_free(dst) {
            // Free cell: must be empty.
            self.free[i].is_none()
        } else if let Some(i) = pile_as_found(dst) {
            // Foundation: Ace onto empty, else same suit ascending by one.
            match self.foundations[i].last() {
                None => card.rank == 1,
                Some(top) => top.suit == card.suit && card.rank == top.rank + 1,
            }
        } else if let Some(i) = pile_as_cascade(dst) {
            // Cascade: empty accepts anything; else descending, alt colour.
            match self.cascades[i].last() {
                None => true,
                Some(top) => top.is_red() != card.is_red() && top.rank == card.rank + 1,
            }
        } else {
            false
        };

        if !allowed {
            return false;
        }

        // Remove from source, then add to destination.
        self.remove_top(src);
        if let Some(i) = pile_as_free(dst) {
            self.free[i] = Some(card);
        } else if let Some(i) = pile_as_found(dst) {
            self.foundations[i].push(card);
        } else if let Some(i) = pile_as_cascade(dst) {
            self.cascades[i].push(card);
        }
        true
    }

    fn remove_top(&mut self, pile: usize) {
        if let Some(i) = pile_as_free(pile) {
            self.free[i] = None;
        } else if let Some(i) = pile_as_found(pile) {
            self.foundations[i].pop();
        } else if let Some(i) = pile_as_cascade(pile) {
            self.cascades[i].pop();
        }
    }

    fn check_win(&mut self) {
        if self.foundations.iter().map(|f| f.len()).sum::<usize>() == 52 {
            self.won = true;
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

/// Render a single cascade card; the bottom card of a cascade is brightened.
fn card_span(card: Card, bottom: bool) -> Span<'static> {
    let text = format!("{:>2}{}", card.rank_str(), card.suit_char());
    let mut style = Style::default().fg(card.color());
    if bottom {
        style = style.add_modifier(Modifier::BOLD);
    } else {
        style = style.fg(if card.is_red() {
            Color::Rgb(150, 60, 60)
        } else {
            Color::Gray
        });
    }
    Span::styled(text, style)
}

// --- Pile index helpers. Piles are laid out as:
//     [0..NUM_FREE)                     free cells
//     [NUM_FREE..NUM_FREE+NUM_FOUND)    foundations
//     [..NUM_PILES)                     cascades
fn free_to_pile(i: usize) -> usize {
    i
}
fn found_to_pile(i: usize) -> usize {
    NUM_FREE + i
}
fn cascade_to_pile(i: usize) -> usize {
    NUM_FREE + NUM_FOUND + i
}

fn pile_as_free(pile: usize) -> Option<usize> {
    if pile < NUM_FREE { Some(pile) } else { None }
}
fn pile_as_found(pile: usize) -> Option<usize> {
    if (NUM_FREE..NUM_FREE + NUM_FOUND).contains(&pile) {
        Some(pile - NUM_FREE)
    } else {
        None
    }
}
fn pile_as_cascade(pile: usize) -> Option<usize> {
    if (NUM_FREE + NUM_FOUND..NUM_PILES).contains(&pile) {
        Some(pile - NUM_FREE - NUM_FOUND)
    } else {
        None
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
    Freecell,
    id: "freecell",
    name: "FreeCell",
    description: "Classic FreeCell solitaire: free cells, foundations, eight cascades.",
    author: "furybee",
}
