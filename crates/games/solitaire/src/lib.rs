//! Klondike Solitaire — single-card draw, keyboard-driven.
//!
//! Layout mirrors a physical game: a stock + waste and four suit foundations
//! along the top, seven tableau columns below. A single cursor roams the board;
//! you pick a card (or a face-up run) with Enter/Space and drop it on a legal
//! destination with Enter/Space again. `f` auto-sends the focused card to its
//! foundation. Pure logic, no `unsafe`, no panics in the loop.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Suits, ordered to match the four foundation slots left-to-right.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Suit {
    Clubs,
    Diamonds,
    Hearts,
    Spades,
}

impl Suit {
    const ALL: [Suit; 4] = [Suit::Clubs, Suit::Diamonds, Suit::Hearts, Suit::Spades];

    fn index(self) -> usize {
        match self {
            Suit::Clubs => 0,
            Suit::Diamonds => 1,
            Suit::Hearts => 2,
            Suit::Spades => 3,
        }
    }

    fn is_red(self) -> bool {
        matches!(self, Suit::Diamonds | Suit::Hearts)
    }

    fn glyph(self) -> char {
        match self {
            Suit::Clubs => '♣',
            Suit::Diamonds => '♦',
            Suit::Hearts => '♥',
            Suit::Spades => '♠',
        }
    }
}

#[derive(Clone, Copy)]
struct Card {
    /// 1 = Ace, 11 = Jack, 12 = Queen, 13 = King.
    rank: u8,
    suit: Suit,
    face_up: bool,
}

impl Card {
    fn is_red(&self) -> bool {
        self.suit.is_red()
    }

    /// Short label like `A♠`, `10♥`, `K♦`.
    fn label(&self) -> String {
        let rank = match self.rank {
            1 => "A".to_string(),
            11 => "J".to_string(),
            12 => "Q".to_string(),
            13 => "K".to_string(),
            n => n.to_string(),
        };
        format!("{}{}", rank, self.suit.glyph())
    }
}

/// Where the roaming cursor currently sits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cursor {
    /// Top-row slot: 0 = stock, 1 = waste, 2..=5 = foundations 0..=3.
    Top(usize),
    /// A tableau column `col` with the highlight on absolute card index `idx`.
    Tab { col: usize, idx: usize },
}

/// A picked-up source awaiting a destination.
#[derive(Clone, Copy)]
enum Selection {
    Waste,
    Foundation(usize),
    Tableau { col: usize, idx: usize },
}

const COLS: usize = 7;

pub struct Solitaire {
    stock: Vec<Card>,
    waste: Vec<Card>,
    foundations: [Vec<Card>; 4],
    tableau: [Vec<Card>; COLS],
    cursor: Cursor,
    selection: Option<Selection>,
    moves: u32,
    won: bool,
    /// Transient hint shown on the status line (e.g. "illegal move").
    message: String,
    rng: u64,
}

impl Game for Solitaire {
    fn new() -> Self {
        let mut game = Solitaire {
            stock: Vec::new(),
            waste: Vec::new(),
            foundations: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            tableau: Default::default(),
            cursor: Cursor::Tab { col: 0, idx: 0 },
            selection: None,
            moves: 0,
            won: false,
            message: String::new(),
            rng: seed(),
        };
        game.deal();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if ctx.pressed(KeyCode::Char('r')) {
            *self = Solitaire::new();
            return Transition::Stay;
        }

        if self.won {
            if ctx.pressed(KeyCode::Enter) {
                *self = Solitaire::new();
            }
            return Transition::Stay;
        }

        if ctx.pressed(KeyCode::Left) {
            self.move_horizontal(-1);
        }
        if ctx.pressed(KeyCode::Right) {
            self.move_horizontal(1);
        }
        if ctx.pressed(KeyCode::Up) {
            self.move_up();
        }
        if ctx.pressed(KeyCode::Down) {
            self.move_down();
        }
        if ctx.pressed(KeyCode::Char('f')) {
            self.auto_to_foundation();
        }
        if ctx.pressed(KeyCode::Enter) || ctx.pressed(KeyCode::Char(' ')) {
            self.activate();
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let board = centered(BOARD_W, BOARD_H, area);
        let title = format!(" Solitaire  ·  moves {} ", self.moves);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(board);
        frame.render_widget(block, board);

        self.render_top_row(frame, inner);
        self.render_tableau(frame, inner);
        self.render_status(frame, inner);

        if self.won {
            let msg = format!(
                " YOU WIN · {} moves · Enter: deal again · q: menu ",
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
        // Turn-based: a brisk poll keeps input snappy without busy-spinning.
        Duration::from_millis(30)
    }
}

// ── Setup ───────────────────────────────────────────────────────────────────

impl Solitaire {
    fn deal(&mut self) {
        let mut deck: Vec<Card> = Vec::with_capacity(52);
        for &suit in &Suit::ALL {
            for rank in 1..=13 {
                deck.push(Card {
                    rank,
                    suit,
                    face_up: false,
                });
            }
        }
        // Fisher–Yates with the crate-local xorshift RNG.
        for i in (1..deck.len()).rev() {
            let j = (self.next_rand() % (i as u64 + 1)) as usize;
            deck.swap(i, j);
        }

        // Columns 0..7 get 1..7 cards; the last card of each is turned face up.
        for col in 0..COLS {
            for row in 0..=col {
                if let Some(mut card) = deck.pop() {
                    card.face_up = row == col;
                    self.tableau[col].push(card);
                }
            }
        }
        // Everything left becomes the (face-down) stock.
        self.stock = deck;
    }
}

// ── Navigation ────────────────────────────────────────────────────────────────

impl Solitaire {
    /// Grid column (0..7) a top-row slot is drawn under, so up/down feel aligned.
    fn top_slot_gridcol(slot: usize) -> usize {
        match slot {
            0 => 0,        // stock
            1 => 1,        // waste
            _ => slot + 1, // foundations 0..3 -> grid cols 3..6
        }
    }

    /// The top-row slot sitting above a tableau column (for the Up jump).
    fn gridcol_top_slot(col: usize) -> usize {
        match col {
            0 => 0,
            1 | 2 => 1,
            c => c - 1, // cols 3..6 -> foundations 0..3 (slots 2..5)
        }
    }

    /// First face-up index in a column (or `len` when fully hidden / empty).
    fn faceup_start(&self, col: usize) -> usize {
        self.tableau[col]
            .iter()
            .position(|c| c.face_up)
            .unwrap_or(self.tableau[col].len())
    }

    fn move_horizontal(&mut self, delta: isize) {
        self.message.clear();
        match self.cursor {
            Cursor::Top(slot) => {
                let next = (slot as isize + delta).clamp(0, 5) as usize;
                self.cursor = Cursor::Top(next);
            }
            Cursor::Tab { col, .. } => {
                let next = (col as isize + delta).clamp(0, COLS as isize - 1) as usize;
                self.cursor = Cursor::Tab {
                    col: next,
                    idx: self.bottom_idx(next),
                };
            }
        }
    }

    fn move_up(&mut self) {
        self.message.clear();
        if let Cursor::Tab { col, idx } = self.cursor {
            let fu = self.faceup_start(col);
            if !self.tableau[col].is_empty() && idx > fu {
                // Reach for a higher card in the face-up run (grabs more).
                self.cursor = Cursor::Tab { col, idx: idx - 1 };
            } else {
                self.cursor = Cursor::Top(Self::gridcol_top_slot(col));
            }
        }
    }

    fn move_down(&mut self) {
        self.message.clear();
        match self.cursor {
            Cursor::Top(slot) => {
                let col = Self::top_slot_gridcol(slot);
                self.cursor = Cursor::Tab {
                    col,
                    idx: self.bottom_idx(col),
                };
            }
            Cursor::Tab { col, idx } => {
                let bottom = self.bottom_idx(col);
                if idx < bottom {
                    self.cursor = Cursor::Tab { col, idx: idx + 1 };
                }
            }
        }
    }

    /// Index of the exposed (bottom) card in a column, 0 when empty.
    fn bottom_idx(&self, col: usize) -> usize {
        self.tableau[col].len().saturating_sub(1)
    }
}

// ── Actions ───────────────────────────────────────────────────────────────────

impl Solitaire {
    /// Enter / Space: draw from stock, or pick-up / drop everywhere else.
    fn activate(&mut self) {
        self.message.clear();

        // The stock slot always means "deal a card" (or recycle the waste).
        if self.cursor == Cursor::Top(0) {
            self.draw_from_stock();
            self.selection = None;
            return;
        }

        match self.selection.take() {
            None => self.pick_up(),
            Some(src) => {
                if self.is_same_location(&src) {
                    // Tapping the source again cancels the pick-up.
                } else if self.try_move(src) {
                    self.moves += 1;
                    self.check_win();
                } else {
                    // Missed drop: treat as picking up the new location instead.
                    self.message = "Illegal move".to_string();
                    self.pick_up();
                }
            }
        }
    }

    fn draw_from_stock(&mut self) {
        if let Some(mut card) = self.stock.pop() {
            card.face_up = true;
            self.waste.push(card);
        } else if !self.waste.is_empty() {
            // Recycle: waste flips back into the stock, face down, order reset.
            while let Some(mut card) = self.waste.pop() {
                card.face_up = false;
                self.stock.push(card);
            }
        } else {
            self.message = "Stock is empty".to_string();
        }
    }

    /// Select the card(s) under the cursor, if any are movable.
    fn pick_up(&mut self) {
        match self.cursor {
            Cursor::Top(1) => {
                if self.waste.is_empty() {
                    self.message = "Waste is empty".to_string();
                } else {
                    self.selection = Some(Selection::Waste);
                }
            }
            Cursor::Top(slot) if slot >= 2 => {
                let f = slot - 2;
                if self.foundations[f].is_empty() {
                    self.message = "Nothing to take".to_string();
                } else {
                    self.selection = Some(Selection::Foundation(f));
                }
            }
            Cursor::Tab { col, idx } => {
                let pile = &self.tableau[col];
                if pile.is_empty() || idx >= pile.len() || !pile[idx].face_up {
                    self.message = "Nothing to take".to_string();
                } else {
                    self.selection = Some(Selection::Tableau { col, idx });
                }
            }
            _ => {}
        }
    }

    /// True if the cursor points at the same pile the selection came from.
    fn is_same_location(&self, src: &Selection) -> bool {
        match (src, self.cursor) {
            (Selection::Waste, Cursor::Top(1)) => true,
            (Selection::Foundation(a), Cursor::Top(slot)) => slot >= 2 && slot - 2 == *a,
            (Selection::Tableau { col, .. }, Cursor::Tab { col: c, .. }) => *col == c,
            _ => false,
        }
    }

    /// Attempt to move the selected card(s) onto the pile under the cursor.
    fn try_move(&mut self, src: Selection) -> bool {
        let cards = self.selected_cards(&src);
        let Some(&lead) = cards.first() else {
            return false;
        };

        let legal = match self.cursor {
            Cursor::Top(slot) if slot >= 2 => {
                // Foundations only ever take a single card; route by its suit.
                cards.len() == 1 && self.foundation_accepts(lead)
            }
            Cursor::Tab { col, .. } => self.tableau_accepts(col, lead),
            _ => false, // stock / waste are never drop targets
        };
        if !legal {
            return false;
        }

        self.detach(src); // remove from origin (and flip any newly exposed card)
        match self.cursor {
            Cursor::Top(_) => self.foundations[lead.suit.index()].push(lead),
            Cursor::Tab { col, .. } => self.tableau[col].extend(cards),
        }
        true
    }

    /// Clone the cards a selection refers to (lead card first).
    fn selected_cards(&self, src: &Selection) -> Vec<Card> {
        match *src {
            Selection::Waste => self.waste.last().copied().into_iter().collect(),
            Selection::Foundation(f) => self.foundations[f].last().copied().into_iter().collect(),
            Selection::Tableau { col, idx } => self.tableau[col]
                .get(idx..)
                .map(|s| s.to_vec())
                .unwrap_or_default(),
        }
    }

    /// Remove the selected cards from their origin, flipping a freshly exposed
    /// tableau card face up.
    fn detach(&mut self, src: Selection) {
        match src {
            Selection::Waste => {
                self.waste.pop();
            }
            Selection::Foundation(f) => {
                self.foundations[f].pop();
            }
            Selection::Tableau { col, idx } => {
                self.tableau[col].truncate(idx);
                if let Some(top) = self.tableau[col].last_mut() {
                    top.face_up = true;
                }
            }
        }
    }

    fn foundation_accepts(&self, card: Card) -> bool {
        let f = &self.foundations[card.suit.index()];
        match f.last() {
            None => card.rank == 1,
            Some(top) => card.rank == top.rank + 1,
        }
    }

    fn tableau_accepts(&self, col: usize, card: Card) -> bool {
        match self.tableau[col].last() {
            None => card.rank == 13, // empty column only takes a King
            Some(top) => top.rank == card.rank + 1 && top.is_red() != card.is_red(),
        }
    }

    /// `f`: send the focused exposed card straight to its foundation.
    fn auto_to_foundation(&mut self) {
        self.message.clear();
        let src = match self.cursor {
            Cursor::Top(1) if !self.waste.is_empty() => Some(Selection::Waste),
            Cursor::Tab { col, idx }
                if !self.tableau[col].is_empty() && idx == self.bottom_idx(col) =>
            {
                Some(Selection::Tableau { col, idx })
            }
            _ => None,
        };
        let Some(src) = src else {
            self.message = "Focus a card to auto-play".to_string();
            return;
        };
        let Some(card) = self.selected_cards(&src).first().copied() else {
            return;
        };
        if self.foundation_accepts(card) {
            self.detach(src);
            self.foundations[card.suit.index()].push(card);
            self.selection = None;
            self.moves += 1;
            self.check_win();
        } else {
            self.message = "No foundation move".to_string();
        }
    }

    fn check_win(&mut self) {
        if self.foundations.iter().all(|f| f.len() == 13) {
            self.won = true;
        }
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Card cell width in chars, and the horizontal step between grid columns.
const CARD_W: u16 = 4;
const STEP: u16 = CARD_W + 1;
const BOARD_W: u16 = STEP * COLS as u16 + 1; // +1 for a touch of right padding
const BOARD_H: u16 = 24;
/// First tableau row, leaving a top row + a blank separator line.
const TAB_Y: u16 = 2;

impl Solitaire {
    fn render_top_row(&self, frame: &mut Frame, inner: Rect) {
        // Stock: a face-down back, or an empty "recycle" marker.
        let stock_label = if self.stock.is_empty() {
            if self.waste.is_empty() { "  " } else { "↺" }
        } else {
            "▒▒"
        };
        let stock_style = Style::default().fg(Color::DarkGray);
        self.draw_cell(
            frame,
            inner,
            0,
            0,
            stock_label,
            stock_style,
            self.cursor == Cursor::Top(0),
            false,
        );

        // Waste: top card face up.
        let (waste_label, waste_style) = match self.waste.last() {
            Some(c) => (c.label(), card_style(c)),
            None => ("·".to_string(), empty_style()),
        };
        let waste_sel = matches!(self.selection, Some(Selection::Waste));
        self.draw_cell(
            frame,
            inner,
            1,
            0,
            &waste_label,
            waste_style,
            self.cursor == Cursor::Top(1),
            waste_sel,
        );

        // Four foundations on the right, each labelled with its suit when empty.
        for (f, suit) in Suit::ALL.iter().enumerate() {
            let gridcol = Self::top_slot_gridcol(f + 2) as u16;
            let (label, style) = match self.foundations[f].last() {
                Some(c) => (c.label(), card_style(c)),
                None => (
                    suit.glyph().to_string(),
                    Style::default().fg(if suit.is_red() {
                        Color::Rgb(120, 70, 70)
                    } else {
                        Color::DarkGray
                    }),
                ),
            };
            let sel = matches!(self.selection, Some(Selection::Foundation(s)) if s == f);
            self.draw_cell(
                frame,
                inner,
                gridcol,
                0,
                &label,
                style,
                self.cursor == Cursor::Top(f + 2),
                sel,
            );
        }
    }

    fn render_tableau(&self, frame: &mut Frame, inner: Rect) {
        for col in 0..COLS {
            let pile = &self.tableau[col];

            if pile.is_empty() {
                let on = self.cursor == (Cursor::Tab { col, idx: 0 });
                self.draw_cell(
                    frame,
                    inner,
                    col as u16,
                    TAB_Y,
                    "·",
                    empty_style(),
                    on,
                    false,
                );
                continue;
            }

            for (idx, card) in pile.iter().enumerate() {
                let y = TAB_Y + idx as u16;
                if inner.y + y >= inner.bottom().saturating_sub(1) {
                    break; // clip overly tall columns above the status line
                }
                let (label, mut style) = if card.face_up {
                    (card.label(), card_style(card))
                } else {
                    ("▒▒".to_string(), Style::default().fg(Color::DarkGray))
                };

                let cursor_here = self.cursor == (Cursor::Tab { col, idx });
                let in_selection = matches!(
                    self.selection,
                    Some(Selection::Tableau { col: c, idx: s }) if c == col && idx >= s
                );
                if cursor_here {
                    style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
                } else if in_selection {
                    style = style.bg(Color::Blue).add_modifier(Modifier::BOLD);
                }
                self.put(frame, inner, col as u16, y, &label, style);
            }
        }
    }

    fn render_status(&self, frame: &mut Frame, inner: Rect) {
        let help = if self.message.is_empty() {
            "←→↑↓ move · Enter pick/drop · f auto · r redeal · q menu".to_string()
        } else {
            self.message.clone()
        };
        let area = Rect {
            x: inner.x,
            y: inner.bottom().saturating_sub(1),
            width: inner.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(help).style(Style::default().fg(Color::Gray)),
            area,
        );
    }

    /// Draw one cell at grid column `gridcol`, applying cursor / selection style.
    #[allow(clippy::too_many_arguments)]
    fn draw_cell(
        &self,
        frame: &mut Frame,
        inner: Rect,
        gridcol: u16,
        y: u16,
        label: &str,
        mut style: Style,
        cursor: bool,
        selected: bool,
    ) {
        if cursor {
            style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
        } else if selected {
            style = style.bg(Color::Blue).add_modifier(Modifier::BOLD);
        }
        self.put(frame, inner, gridcol, y, label, style);
    }

    /// Render a padded, styled label into the cell at `(gridcol, y)`.
    fn put(&self, frame: &mut Frame, inner: Rect, gridcol: u16, y: u16, label: &str, style: Style) {
        let x = gridcol * STEP;
        if x >= inner.width || inner.y + y >= inner.bottom() {
            return;
        }
        let cell = Rect {
            x: inner.x + x,
            y: inner.y + y,
            width: CARD_W.min(inner.width - x),
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(pad(label), style))),
            cell,
        );
    }
}

fn card_style(c: &Card) -> Style {
    Style::default().fg(if c.is_red() {
        Color::LightRed
    } else {
        Color::White
    })
}

fn empty_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// Pad a label to the card-cell width.
fn pad(label: &str) -> String {
    format!("{:<width$}", label, width = CARD_W as usize)
}

// ── Utilities ─────────────────────────────────────────────────────────────────

impl Solitaire {
    /// xorshift64 — keeps the crate dependency-free (matches Snake's RNG).
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
    Solitaire,
    id: "solitaire",
    name: "Solitaire",
    description: "Klondike patience — clear the tableau to the foundations.",
    author: "furybee",
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deal_lays_out_a_full_klondike() {
        let g = Solitaire::new();
        // 28 cards on the tableau (1+2+..+7), 24 left in the stock, 52 total.
        let on_table: usize = g.tableau.iter().map(|c| c.len()).sum();
        assert_eq!(on_table, 28);
        assert_eq!(g.stock.len(), 24);
        assert!(g.waste.is_empty());
        // Each column i holds i+1 cards; only its bottom card is face up.
        for (i, col) in g.tableau.iter().enumerate() {
            assert_eq!(col.len(), i + 1);
            assert!(col.last().unwrap().face_up);
            assert!(col[..col.len() - 1].iter().all(|c| !c.face_up));
        }
    }

    #[test]
    fn foundation_only_takes_ascending_same_suit() {
        let mut g = Solitaire::new();
        let ace = Card {
            rank: 1,
            suit: Suit::Hearts,
            face_up: true,
        };
        let two = Card {
            rank: 2,
            suit: Suit::Hearts,
            face_up: true,
        };
        let two_spades = Card {
            rank: 2,
            suit: Suit::Spades,
            face_up: true,
        };
        assert!(g.foundation_accepts(ace)); // empty -> Ace only
        assert!(!g.foundation_accepts(two));
        g.foundations[Suit::Hearts.index()].push(ace);
        assert!(g.foundation_accepts(two)); // 2♥ on A♥
        assert!(!g.foundation_accepts(two_spades)); // wrong foundation is empty
    }

    #[test]
    fn tableau_takes_descending_alternating_color() {
        let mut g = Solitaire::new();
        g.tableau[0].clear();
        g.tableau[0].push(Card {
            rank: 7,
            suit: Suit::Spades,
            face_up: true,
        });
        // 6♥ (red) on 7♠ (black) is legal; 6♣ (black) is not.
        assert!(g.tableau_accepts(
            0,
            Card {
                rank: 6,
                suit: Suit::Hearts,
                face_up: true
            }
        ));
        assert!(!g.tableau_accepts(
            0,
            Card {
                rank: 6,
                suit: Suit::Clubs,
                face_up: true
            }
        ));
        // Empty column accepts a King and nothing else.
        g.tableau[1].clear();
        assert!(g.tableau_accepts(
            1,
            Card {
                rank: 13,
                suit: Suit::Diamonds,
                face_up: true
            }
        ));
        assert!(!g.tableau_accepts(
            1,
            Card {
                rank: 12,
                suit: Suit::Diamonds,
                face_up: true
            }
        ));
    }

    #[test]
    fn moving_a_run_flips_the_newly_exposed_card() {
        let mut g = Solitaire::new();
        g.tableau[0] = vec![
            Card {
                rank: 9,
                suit: Suit::Clubs,
                face_up: false,
            },
            Card {
                rank: 5,
                suit: Suit::Hearts,
                face_up: true,
            },
        ];
        g.tableau[1] = vec![Card {
            rank: 6,
            suit: Suit::Spades,
            face_up: true,
        }];
        // Pick up 5♥ from column 0, drop on 6♠ in column 1.
        g.selection = Some(Selection::Tableau { col: 0, idx: 1 });
        g.cursor = Cursor::Tab { col: 1, idx: 0 };
        assert!(g.try_move(Selection::Tableau { col: 0, idx: 1 }));
        assert_eq!(g.tableau[1].len(), 2);
        assert!(g.tableau[0][0].face_up); // 9♣ turned up
    }
}
