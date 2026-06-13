//! Video Poker — Jacks-or-Better.
//!
//! Mirrors the snake reference: state machine, `dt`-free input-driven play,
//! a centred playfield, a controls hint, a result overlay, and self
//! registration. The xorshift `next_rand` + `seed` pattern is copied verbatim.
//!
//! Flow: a deal shows five cards. Keys `1`-`5` toggle a HOLD on each card,
//! `Enter` draws replacements for the cards you did not hold, the final hand is
//! scored against the standard 9/6 Jacks-or-Better table, and `Enter` again
//! starts the next deal.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Credits handed out at the start of a session.
const STARTING_CREDITS: i64 = 100;
/// Credits wagered on every deal.
const BET: i64 = 5;

/// Which screen the game is currently showing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Cards dealt, player toggling holds before the draw.
    Holding,
    /// Draw resolved, hand scored, waiting for the next deal.
    Showdown,
    /// Out of credits — game over.
    Broke,
}

/// A poker hand ranking, ordered low to high for clarity.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HandRank {
    Nothing,
    JacksOrBetter,
    TwoPair,
    ThreeOfAKind,
    Straight,
    Flush,
    FullHouse,
    FourOfAKind,
    StraightFlush,
    RoyalFlush,
}

impl HandRank {
    /// Payout multiplier per credit bet (0 means the bet is lost).
    fn payout(self) -> i64 {
        match self {
            HandRank::Nothing => 0,
            HandRank::JacksOrBetter => 1,
            HandRank::TwoPair => 2,
            HandRank::ThreeOfAKind => 3,
            HandRank::Straight => 4,
            HandRank::Flush => 6,
            HandRank::FullHouse => 9,
            HandRank::FourOfAKind => 25,
            HandRank::StraightFlush => 50,
            HandRank::RoyalFlush => 250,
        }
    }

    fn label(self) -> &'static str {
        match self {
            HandRank::Nothing => "No win",
            HandRank::JacksOrBetter => "Jacks or Better",
            HandRank::TwoPair => "Two Pair",
            HandRank::ThreeOfAKind => "Three of a Kind",
            HandRank::Straight => "Straight",
            HandRank::Flush => "Flush",
            HandRank::FullHouse => "Full House",
            HandRank::FourOfAKind => "Four of a Kind",
            HandRank::StraightFlush => "Straight Flush",
            HandRank::RoyalFlush => "Royal Flush",
        }
    }
}

/// A playing card encoded as `rank` (2..=14, where 11=J … 14=A) and `suit`
/// (0..=3). Suit colour follows the conventional red/black split.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Card {
    rank: u8,
    suit: u8,
}

impl Card {
    fn rank_str(self) -> &'static str {
        match self.rank {
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
            13 => "K",
            _ => "A",
        }
    }

    fn suit_str(self) -> &'static str {
        match self.suit {
            0 => "\u{2660}", // spades
            1 => "\u{2665}", // hearts
            2 => "\u{2666}", // diamonds
            _ => "\u{2663}", // clubs
        }
    }

    /// Hearts and diamonds are red; spades and clubs are white-ish.
    fn color(self) -> Color {
        match self.suit {
            1 | 2 => Color::LightRed,
            _ => Color::White,
        }
    }
}

pub struct Videopoker {
    deck: Vec<Card>,
    hand: [Card; 5],
    held: [bool; 5],
    phase: Phase,
    credits: i64,
    last_rank: HandRank,
    last_win: i64,
    rng: u64,
}

impl Game for Videopoker {
    fn new() -> Self {
        let mut game = Videopoker {
            deck: Vec::with_capacity(52),
            hand: [Card { rank: 2, suit: 0 }; 5],
            held: [false; 5],
            phase: Phase::Holding,
            credits: STARTING_CREDITS,
            last_rank: HandRank::Nothing,
            last_win: 0,
            rng: seed(),
        };
        game.deal();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        match self.phase {
            Phase::Holding => {
                for (i, key) in [
                    KeyCode::Char('1'),
                    KeyCode::Char('2'),
                    KeyCode::Char('3'),
                    KeyCode::Char('4'),
                    KeyCode::Char('5'),
                ]
                .into_iter()
                .enumerate()
                {
                    if ctx.pressed(key) {
                        self.held[i] = !self.held[i];
                    }
                }
                if ctx.pressed(KeyCode::Enter) {
                    self.draw_and_score();
                }
            }
            Phase::Showdown => {
                if ctx.pressed(KeyCode::Enter) {
                    self.deal();
                }
            }
            Phase::Broke => {
                if ctx.pressed(KeyCode::Enter) {
                    *self = Videopoker::new();
                }
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        // Card art is 9 wide; five cards + gaps fit in ~55 columns.
        let field = centered(58, 18, area);
        let title = format!(" Video Poker  \u{00b7}  credits {} ", self.credits);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::raw(""));
        lines.extend(self.render_cards());
        lines.push(Line::raw(""));
        lines.push(self.render_status());
        lines.push(Line::raw(""));
        lines.push(self.render_hint());
        lines.push(Line::raw(""));
        lines.extend(payout_table_lines());

        frame.render_widget(Paragraph::new(lines), inner);

        if self.phase == Phase::Broke {
            let msg = " OUT OF CREDITS \u{00b7} Enter: new session \u{00b7} q: menu ";
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
        // Input-driven; a modest poll keeps key handling responsive.
        Duration::from_millis(30)
    }
}

impl Videopoker {
    /// Draw the five card "art" rows: each card is a small bordered box that
    /// shows its index, rank/suit, and a HOLD marker.
    fn render_cards(&self) -> Vec<Line<'static>> {
        let mut tops = Vec::with_capacity(5);
        let mut mids = Vec::with_capacity(5);
        let mut bots = Vec::with_capacity(5);
        let mut tags = Vec::with_capacity(5);

        for (i, card) in self.hand.iter().enumerate() {
            let style = Style::default().fg(card.color());
            let rank = card.rank_str();
            // Pad the face to a fixed inner width of 7 columns.
            let face = format!("{:<3}{:>4}", rank, card.suit_str());

            tops.push(Span::styled(
                "\u{250c}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}"
                    .to_string(),
                style,
            ));
            mids.push(Span::styled(format!("\u{2502}{}\u{2502}", face), style));
            bots.push(Span::styled(
                "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}"
                    .to_string(),
                style,
            ));

            let tag = if self.held[i] {
                Span::styled(
                    "  [HOLD] ",
                    Style::default()
                        .fg(Color::LightGreen)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    format!("   ({})   ", i + 1),
                    Style::default().fg(Color::DarkGray),
                )
            };
            tags.push(tag);

            let gap = Span::raw("  ");
            tops.push(gap.clone());
            mids.push(gap.clone());
            bots.push(gap.clone());
            tags.push(gap);
        }

        vec![
            Line::from(tops),
            Line::from(mids),
            Line::from(bots),
            Line::from(tags),
        ]
    }

    fn render_status(&self) -> Line<'static> {
        match self.phase {
            Phase::Holding => Line::styled(
                format!(
                    "  Bet {} \u{00b7} pick cards to HOLD, then Enter to draw.",
                    BET
                ),
                Style::default().fg(Color::Gray),
            ),
            Phase::Showdown | Phase::Broke => {
                let style = if self.last_win > 0 {
                    Style::default()
                        .fg(Color::LightGreen)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };
                let text = if self.last_win > 0 {
                    format!(
                        "  {} \u{2014} won {} credits!",
                        self.last_rank.label(),
                        self.last_win
                    )
                } else {
                    format!("  {} \u{2014} bet lost.", self.last_rank.label())
                };
                Line::styled(text, style)
            }
        }
    }

    fn render_hint(&self) -> Line<'static> {
        let text = match self.phase {
            Phase::Holding => "  1-5: toggle hold   Enter: draw   q/Esc: menu",
            Phase::Showdown => "  Enter: deal again   q/Esc: menu",
            Phase::Broke => "  Enter: restart   q/Esc: menu",
        };
        Line::styled(text, Style::default().fg(Color::DarkGray))
    }

    /// Build a fresh, shuffled deck and deal five cards, taking the bet.
    fn deal(&mut self) {
        if self.credits < BET {
            self.phase = Phase::Broke;
            return;
        }
        self.credits -= BET;
        self.build_deck();
        self.held = [false; 5];
        for slot in self.hand.iter_mut() {
            *slot = self.deck.pop().unwrap_or(Card { rank: 2, suit: 0 });
        }
        self.last_rank = HandRank::Nothing;
        self.last_win = 0;
        self.phase = Phase::Holding;
    }

    /// Replace non-held cards from the remaining deck, then score the hand and
    /// credit any winnings.
    fn draw_and_score(&mut self) {
        for i in 0..5 {
            if !self.held[i]
                && let Some(card) = self.deck.pop()
            {
                self.hand[i] = card;
            }
        }
        let rank = evaluate(&self.hand);
        let win = rank.payout() * BET;
        self.credits += win;
        self.last_rank = rank;
        self.last_win = win;
        self.phase = Phase::Showdown;
    }

    /// Fill `deck` with a full 52-card pack and Fisher-Yates shuffle it.
    fn build_deck(&mut self) {
        self.deck.clear();
        for suit in 0u8..4 {
            for rank in 2u8..=14 {
                self.deck.push(Card { rank, suit });
            }
        }
        // Fisher-Yates using the xorshift PRNG.
        let len = self.deck.len();
        for i in (1..len).rev() {
            let j = (self.next_rand() % (i as u64 + 1)) as usize;
            self.deck.swap(i, j);
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

/// Score a five-card hand under Jacks-or-Better rules.
fn evaluate(hand: &[Card; 5]) -> HandRank {
    // Rank counts indexed by rank value (2..=14).
    let mut counts = [0u8; 15];
    for card in hand {
        counts[card.rank as usize] += 1;
    }

    let flush = hand.iter().all(|c| c.suit == hand[0].suit);

    // Collect distinct ranks present, sorted ascending, to test straights.
    let mut ranks: Vec<u8> = (2u8..=14).filter(|&r| counts[r as usize] > 0).collect();
    ranks.sort_unstable();

    let straight = is_straight(&counts, &ranks);
    // A straight is "royal" when it runs Ten through Ace.
    let royal = straight && counts[14] == 1 && counts[10] == 1;

    let mut pairs = 0u8;
    let mut trips = 0u8;
    let mut quads = 0u8;
    let mut has_high_pair = false;
    for r in 2u8..=14 {
        match counts[r as usize] {
            2 => {
                pairs += 1;
                if r >= 11 || r == 14 {
                    has_high_pair = true;
                }
            }
            3 => trips += 1,
            4 => quads += 1,
            _ => {}
        }
    }

    if flush && royal {
        HandRank::RoyalFlush
    } else if flush && straight {
        HandRank::StraightFlush
    } else if quads == 1 {
        HandRank::FourOfAKind
    } else if trips == 1 && pairs == 1 {
        HandRank::FullHouse
    } else if flush {
        HandRank::Flush
    } else if straight {
        HandRank::Straight
    } else if trips == 1 {
        HandRank::ThreeOfAKind
    } else if pairs == 2 {
        HandRank::TwoPair
    } else if pairs == 1 && has_high_pair {
        HandRank::JacksOrBetter
    } else {
        HandRank::Nothing
    }
}

/// Whether five distinct ranks form a straight, including the Ace-low wheel
/// (A-2-3-4-5).
fn is_straight(counts: &[u8; 15], ranks: &[u8]) -> bool {
    if ranks.len() != 5 {
        return false;
    }
    // Ace-low wheel: treat Ace as 1.
    if counts[14] == 1 && counts[2] == 1 && counts[3] == 1 && counts[4] == 1 && counts[5] == 1 {
        return true;
    }
    ranks.windows(2).all(|w| w[1] == w[0] + 1)
}

/// The static 9/6 Jacks-or-Better paytable, rendered as dim reference lines.
fn payout_table_lines() -> Vec<Line<'static>> {
    let rows = [
        HandRank::RoyalFlush,
        HandRank::StraightFlush,
        HandRank::FourOfAKind,
        HandRank::FullHouse,
        HandRank::Flush,
        HandRank::Straight,
        HandRank::ThreeOfAKind,
        HandRank::TwoPair,
        HandRank::JacksOrBetter,
    ];
    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(Line::styled(
        "  PAYTABLE (per credit bet)",
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    ));
    for rank in rows {
        lines.push(Line::styled(
            format!("    {:<18}{:>4}x", rank.label(), rank.payout()),
            Style::default().fg(Color::DarkGray),
        ));
    }
    lines
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
    Videopoker,
    id: "videopoker",
    name: "Video Poker",
    description: "Jacks-or-Better: hold, draw, and chase the royal flush.",
    author: "furybee",
}
