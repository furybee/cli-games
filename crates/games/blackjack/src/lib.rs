//! Blackjack — single-player versus a dealer that draws to 17.
//!
//! Mirrors the snake reference: dependency-free xorshift RNG (`next_rand` +
//! `seed`), a `centered` helper, `dt`-driven dealer pacing, and `Clear` +
//! bordered `Paragraph` overlays. The loop is a small state machine:
//! Betting → Player turn → Dealer turn → Settle, with Enter dealing the next
//! hand and `q`/`Esc` returning to the menu.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Starting bankroll in chips.
const START_CHIPS: u32 = 100;
/// Default wager, nudged with Left/Right while betting.
const DEFAULT_BET: u32 = 10;
/// Dealer reveals a fresh card every `DEAL_STEP` once it is the house's turn.
const DEAL_STEP: Duration = Duration::from_millis(550);

/// A 52-card deck is addressed by index 0..52; rank = idx % 13, suit = idx / 13.
#[derive(Clone, Copy)]
struct Card {
    /// 0 = Ace, 1..9 = 2..10, 10 = J, 11 = Q, 12 = K.
    rank: u8,
    /// 0 = ♠, 1 = ♥, 2 = ♦, 3 = ♣.
    suit: u8,
}

impl Card {
    fn from_index(idx: u8) -> Card {
        Card {
            rank: idx % 13,
            suit: idx / 13,
        }
    }

    /// Base point value; Aces count as 11 here and are softened later.
    fn value(self) -> u32 {
        match self.rank {
            0 => 11,
            r if r >= 9 => 10,
            r => (r + 1) as u32,
        }
    }

    fn rank_label(self) -> &'static str {
        match self.rank {
            0 => "A",
            1 => "2",
            2 => "3",
            3 => "4",
            4 => "5",
            5 => "6",
            6 => "7",
            7 => "8",
            8 => "9",
            9 => "10",
            10 => "J",
            11 => "Q",
            _ => "K",
        }
    }

    fn suit_label(self) -> &'static str {
        match self.suit {
            0 => "\u{2660}",
            1 => "\u{2665}",
            2 => "\u{2666}",
            _ => "\u{2663}",
        }
    }

    fn color(self) -> Color {
        if self.suit == 1 || self.suit == 2 {
            Color::Red
        } else {
            Color::White
        }
    }
}

/// Total points for a hand, treating Aces as 11 then demoting to 1 as needed.
fn hand_total(cards: &[Card]) -> u32 {
    let mut total: u32 = cards.iter().map(|c| c.value()).sum();
    let mut aces = cards.iter().filter(|c| c.rank == 0).count();
    while total > 21 && aces > 0 {
        total -= 10;
        aces -= 1;
    }
    total
}

fn is_blackjack(cards: &[Card]) -> bool {
    cards.len() == 2 && hand_total(cards) == 21
}

/// Where we are in the round.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Choosing a wager; Left/Right adjust, Enter deals.
    Betting,
    /// Player acts: h/hit, s/stand, d/double.
    Player,
    /// Dealer reveals and draws, paced by `dt`.
    Dealer,
    /// Hand settled; Enter starts the next one.
    Settled,
}

/// Outcome of a settled hand, used for the result banner.
#[derive(Clone, Copy)]
enum Outcome {
    PlayerBlackjack,
    PlayerWin,
    DealerWin,
    Push,
    Broke,
}

pub struct Blackjack {
    chips: u32,
    bet: u32,
    /// The remaining shoe; cards are popped from the back as they are dealt.
    deck: Vec<Card>,
    player: Vec<Card>,
    dealer: Vec<Card>,
    phase: Phase,
    /// True after the player doubled, so they get exactly one extra card.
    doubled: bool,
    outcome: Option<Outcome>,
    /// Paces the dealer's draws while in `Phase::Dealer`.
    accumulator: Duration,
    rng: u64,
}

impl Game for Blackjack {
    fn new() -> Self {
        Blackjack {
            chips: START_CHIPS,
            bet: DEFAULT_BET,
            deck: Vec::new(),
            player: Vec::new(),
            dealer: Vec::new(),
            phase: Phase::Betting,
            doubled: false,
            outcome: None,
            accumulator: Duration::ZERO,
            rng: seed(),
        }
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        match self.phase {
            Phase::Betting => self.update_betting(ctx),
            Phase::Player => self.update_player(ctx),
            Phase::Dealer => self.update_dealer(ctx),
            Phase::Settled => self.update_settled(ctx),
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(
            " Blackjack  \u{b7}  chips {}  \u{b7}  bet {} ",
            self.chips, self.bet
        );
        let field = centered(46, 18, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        // Hide the dealer's hole card until it is the house's turn.
        let hide_hole = matches!(self.phase, Phase::Player);

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(Span::styled(
            "  Dealer",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(self.hand_line(&self.dealer, hide_hole));
        lines.push(Line::from(Span::styled(
            self.dealer_total_label(hide_hole),
            Style::default().fg(Color::Gray),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  You",
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(self.hand_line(&self.player, false));
        lines.push(Line::from(Span::styled(
            if self.player.is_empty() {
                String::new()
            } else {
                format!("  total {}", hand_total(&self.player))
            },
            Style::default().fg(Color::Gray),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            self.hint(),
            Style::default().fg(Color::DarkGray),
        )));

        frame.render_widget(Paragraph::new(lines), inner);

        if self.phase == Phase::Settled
            && let Some(outcome) = self.outcome
        {
            self.render_overlay(frame, area, outcome);
        }
    }

    fn tick_rate(&self) -> Duration {
        Duration::from_millis(40)
    }
}

impl Blackjack {
    fn update_betting(&mut self, ctx: &GameContext) {
        if self.chips == 0 {
            // Out of chips: a fresh stake on Enter so the table never softlocks.
            if ctx.pressed(KeyCode::Enter) {
                *self = Blackjack::new();
            }
            return;
        }
        if ctx.pressed(KeyCode::Left) {
            self.bet = self.bet.saturating_sub(5).max(5);
        }
        if ctx.pressed(KeyCode::Right) {
            self.bet = (self.bet + 5).min(self.chips);
        }
        self.bet = self.bet.clamp(5, self.chips);
        if ctx.pressed(KeyCode::Enter) {
            self.deal();
        }
    }

    fn update_player(&mut self, ctx: &GameContext) {
        if ctx.pressed(KeyCode::Char('h')) {
            self.player_draw();
            if hand_total(&self.player) >= 21 {
                self.start_dealer();
            }
        } else if ctx.pressed(KeyCode::Char('s')) {
            self.start_dealer();
        } else if ctx.pressed(KeyCode::Char('d'))
            && self.player.len() == 2
            && self.chips >= self.bet
        {
            // Double down: stake doubles, take one card, then stand.
            self.chips -= self.bet;
            self.bet *= 2;
            self.doubled = true;
            self.player_draw();
            self.start_dealer();
        }
    }

    fn update_dealer(&mut self, ctx: &GameContext) {
        self.accumulator += ctx.dt;
        while self.accumulator >= DEAL_STEP {
            self.accumulator -= DEAL_STEP;
            if hand_total(&self.dealer) < 17 {
                self.dealer_draw();
            } else {
                self.settle();
                break;
            }
        }
    }

    fn update_settled(&mut self, ctx: &GameContext) {
        if ctx.pressed(KeyCode::Enter) {
            self.phase = Phase::Betting;
            self.player.clear();
            self.dealer.clear();
            self.outcome = None;
            self.doubled = false;
            self.bet = self.bet.clamp(5, self.chips.max(5));
        }
    }

    /// Deal the opening two cards each and resolve naturals immediately.
    fn deal(&mut self) {
        self.chips -= self.bet;
        self.refill_if_low();
        self.player.clear();
        self.dealer.clear();
        self.doubled = false;
        self.outcome = None;
        self.accumulator = Duration::ZERO;
        self.player_draw();
        self.dealer_draw();
        self.player_draw();
        self.dealer_draw();

        if is_blackjack(&self.player) || is_blackjack(&self.dealer) {
            self.phase = Phase::Dealer;
            self.settle();
        } else {
            self.phase = Phase::Player;
        }
    }

    fn start_dealer(&mut self) {
        self.accumulator = Duration::ZERO;
        if hand_total(&self.player) > 21 {
            // Player busted: no need to play the house out.
            self.phase = Phase::Dealer;
            self.settle();
        } else {
            self.phase = Phase::Dealer;
        }
    }

    fn player_draw(&mut self) {
        if let Some(card) = self.draw_card() {
            self.player.push(card);
        }
    }

    fn dealer_draw(&mut self) {
        if let Some(card) = self.draw_card() {
            self.dealer.push(card);
        }
    }

    fn draw_card(&mut self) -> Option<Card> {
        self.refill_if_low();
        self.deck.pop()
    }

    /// Reshuffle a full 52-card shoe whenever it runs thin.
    fn refill_if_low(&mut self) {
        if self.deck.len() >= 15 {
            return;
        }
        self.deck = (0u8..52).map(Card::from_index).collect();
        // Fisher–Yates with the shared xorshift RNG.
        let len = self.deck.len();
        for i in (1..len).rev() {
            let j = (self.next_rand() % (i as u64 + 1)) as usize;
            self.deck.swap(i, j);
        }
    }

    /// Decide the hand and pay out. Called once the dealer is done (or a side
    /// busted / had a natural). The current `bet` was already deducted.
    fn settle(&mut self) {
        let player = hand_total(&self.player);
        let dealer = hand_total(&self.dealer);
        let player_bj = is_blackjack(&self.player);
        let dealer_bj = is_blackjack(&self.dealer);

        let outcome = if player > 21 {
            Outcome::DealerWin
        } else if player_bj && !dealer_bj {
            // Natural pays 3:2 — return stake plus 1.5x.
            self.chips += self.bet + self.bet + self.bet / 2;
            Outcome::PlayerBlackjack
        } else if dealer_bj && !player_bj {
            Outcome::DealerWin
        } else if dealer > 21 || player > dealer {
            self.chips += self.bet * 2;
            Outcome::PlayerWin
        } else if player < dealer {
            Outcome::DealerWin
        } else {
            // Push: stake returned.
            self.chips += self.bet;
            Outcome::Push
        };

        self.phase = Phase::Settled;
        self.outcome = Some(if self.chips == 0 {
            Outcome::Broke
        } else {
            outcome
        });
    }

    fn hand_line(&self, cards: &[Card], hide_hole: bool) -> Line<'static> {
        if cards.is_empty() {
            return Line::from(Span::styled("  —", Style::default().fg(Color::DarkGray)));
        }
        let mut spans: Vec<Span<'static>> = vec![Span::raw("  ")];
        for (i, card) in cards.iter().enumerate() {
            if hide_hole && i == 1 {
                spans.push(Span::styled("[ ?? ]", Style::default().fg(Color::DarkGray)));
            } else {
                let face = format!("[{}{} ]", card.rank_label(), card.suit_label());
                spans.push(Span::styled(face, Style::default().fg(card.color())));
            }
            spans.push(Span::raw(" "));
        }
        Line::from(spans)
    }

    fn dealer_total_label(&self, hide_hole: bool) -> String {
        if self.dealer.is_empty() {
            String::new()
        } else if hide_hole {
            format!("  showing {}", self.dealer[0].value())
        } else {
            format!("  total {}", hand_total(&self.dealer))
        }
    }

    fn hint(&self) -> String {
        match self.phase {
            Phase::Betting => {
                if self.chips == 0 {
                    "  Out of chips · Enter: new stake · q: menu".to_string()
                } else {
                    "  </>: bet · Enter: deal · q: menu".to_string()
                }
            }
            Phase::Player => "  h: hit · s: stand · d: double · q: menu".to_string(),
            Phase::Dealer => "  Dealer drawing...".to_string(),
            Phase::Settled => "  Enter: next hand · q: menu".to_string(),
        }
    }

    fn render_overlay(&self, frame: &mut Frame, area: Rect, outcome: Outcome) {
        let (text, color) = match outcome {
            Outcome::PlayerBlackjack => (" BLACKJACK! Paid 3:2 · Enter: next ", Color::Yellow),
            Outcome::PlayerWin => (" YOU WIN · Enter: next ", Color::LightGreen),
            Outcome::DealerWin => (" DEALER WINS · Enter: next ", Color::Red),
            Outcome::Push => (" PUSH · bet returned · Enter: next ", Color::Cyan),
            Outcome::Broke => (" BUSTED OUT · Enter: rebuy ", Color::Magenta),
        };
        let overlay = centered(text.chars().count() as u16 + 2, 3, area);
        frame.render_widget(Clear, overlay);
        frame.render_widget(
            Paragraph::new(text)
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
    Blackjack,
    id: "blackjack",
    name: "Blackjack",
    description: "Beat the dealer at 21: bet, hit, stand, double.",
    author: "furybee",
}
