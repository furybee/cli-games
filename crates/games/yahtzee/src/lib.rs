//! Yahtzee — solo dice game.
//!
//! Roll five dice up to three times per turn (keys `1`-`5` toggle which dice to
//! keep, `r` rolls). When you are happy with the dice, pick one of the 13
//! scorecard categories (`Up`/`Down` to move the cursor, `Enter`/`Space` to
//! score it). After all 13 categories are filled the final score — including the
//! upper-section bonus — is shown.
//!
//! Mirrors the structure of the `snake` reference crate: state + input + a
//! `dt`-driven roll animation, a centred playfield, a controls hint, and a
//! game-over overlay rendered with `Clear` + a bordered `Paragraph`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Number of dice in a Yahtzee hand.
const DICE: usize = 5;
/// Number of scorecard categories (= number of turns).
const CATEGORIES: usize = 13;
/// Maximum rolls per turn.
const MAX_ROLLS: u8 = 3;
/// Upper-section threshold that awards the bonus.
const BONUS_THRESHOLD: u32 = 63;
/// Bonus awarded for reaching the upper threshold.
const BONUS: u32 = 35;
/// How long the dice "tumble" before settling, per roll.
const ROLL_TIME: Duration = Duration::from_millis(420);
/// How fast tumbling faces flicker during a roll.
const TUMBLE_STEP: Duration = Duration::from_millis(60);

/// The 13 scorecard categories, in scorecard order.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Category {
    Ones,
    Twos,
    Threes,
    Fours,
    Fives,
    Sixes,
    ThreeKind,
    FourKind,
    FullHouse,
    SmallStraight,
    LargeStraight,
    Yahtzee,
    Chance,
}

const ALL_CATEGORIES: [Category; CATEGORIES] = [
    Category::Ones,
    Category::Twos,
    Category::Threes,
    Category::Fours,
    Category::Fives,
    Category::Sixes,
    Category::ThreeKind,
    Category::FourKind,
    Category::FullHouse,
    Category::SmallStraight,
    Category::LargeStraight,
    Category::Yahtzee,
    Category::Chance,
];

impl Category {
    fn label(self) -> &'static str {
        match self {
            Category::Ones => "Ones",
            Category::Twos => "Twos",
            Category::Threes => "Threes",
            Category::Fours => "Fours",
            Category::Fives => "Fives",
            Category::Sixes => "Sixes",
            Category::ThreeKind => "Three of a Kind",
            Category::FourKind => "Four of a Kind",
            Category::FullHouse => "Full House",
            Category::SmallStraight => "Small Straight",
            Category::LargeStraight => "Large Straight",
            Category::Yahtzee => "Yahtzee",
            Category::Chance => "Chance",
        }
    }

    /// `true` for the six upper-section categories that feed the bonus.
    fn is_upper(self) -> bool {
        matches!(
            self,
            Category::Ones
                | Category::Twos
                | Category::Threes
                | Category::Fours
                | Category::Fives
                | Category::Sixes
        )
    }

    /// Score this category for the given dice.
    fn score(self, dice: &[u8; DICE]) -> u32 {
        let counts = counts_of(dice);
        let sum: u32 = dice
            .iter()
            .map(|&d| d as u32)
            .collect::<Vec<_>>()
            .iter()
            .sum();
        match self {
            Category::Ones => face_sum(&counts, 1),
            Category::Twos => face_sum(&counts, 2),
            Category::Threes => face_sum(&counts, 3),
            Category::Fours => face_sum(&counts, 4),
            Category::Fives => face_sum(&counts, 5),
            Category::Sixes => face_sum(&counts, 6),
            Category::ThreeKind => {
                if counts.iter().any(|&c| c >= 3) {
                    sum
                } else {
                    0
                }
            }
            Category::FourKind => {
                if counts.iter().any(|&c| c >= 4) {
                    sum
                } else {
                    0
                }
            }
            Category::FullHouse => {
                let has_three = counts.contains(&3);
                let has_two = counts.contains(&2);
                if (has_three && has_two) || counts.contains(&5) {
                    25
                } else {
                    0
                }
            }
            Category::SmallStraight => {
                if has_run(&counts, 4) {
                    30
                } else {
                    0
                }
            }
            Category::LargeStraight => {
                if has_run(&counts, 5) {
                    40
                } else {
                    0
                }
            }
            Category::Yahtzee => {
                if counts.contains(&5) {
                    50
                } else {
                    0
                }
            }
            Category::Chance => sum,
        }
    }
}

/// Count of each face value 1..=6, indexed `counts[face - 1]`.
fn counts_of(dice: &[u8; DICE]) -> [u8; 6] {
    let mut counts = [0u8; 6];
    for &d in dice {
        if (1..=6).contains(&d) {
            counts[(d - 1) as usize] += 1;
        }
    }
    counts
}

/// Sum of all dice showing `face`.
fn face_sum(counts: &[u8; 6], face: usize) -> u32 {
    if (1..=6).contains(&face) {
        counts[face - 1] as u32 * face as u32
    } else {
        0
    }
}

/// `true` if the dice contain a consecutive run of at least `len` faces.
fn has_run(counts: &[u8; 6], len: u8) -> bool {
    let mut best = 0u8;
    let mut current = 0u8;
    for &c in counts {
        if c > 0 {
            current += 1;
            if current > best {
                best = current;
            }
        } else {
            current = 0;
        }
    }
    best >= len
}

/// Phase of the current turn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Dice are tumbling; input is briefly locked.
    Rolling,
    /// Dice are settled; the player may toggle keeps, roll again, or pick.
    Choosing,
    /// All categories filled — show the final tally.
    Finished,
}

pub struct Yahtzee {
    /// Current face of each die (1..=6).
    dice: [u8; DICE],
    /// Whether each die is kept (locked) for the next roll.
    kept: [bool; DICE],
    /// Recorded score for each category, or `None` if still open.
    card: [Option<u32>; CATEGORIES],
    /// Rolls used this turn (0 means the turn hasn't started).
    rolls_used: u8,
    /// Highlighted scorecard row.
    cursor: usize,
    phase: Phase,
    /// Time left in the current tumble animation.
    roll_timer: Duration,
    /// Accumulator that drives the tumble flicker.
    tumble: Duration,
    rng: u64,
}

impl Game for Yahtzee {
    fn new() -> Self {
        Yahtzee {
            dice: [1; DICE],
            kept: [false; DICE],
            card: [None; CATEGORIES],
            rolls_used: 0,
            cursor: 0,
            phase: Phase::Choosing,
            roll_timer: Duration::ZERO,
            tumble: Duration::ZERO,
            rng: seed(),
        }
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        match self.phase {
            Phase::Finished => {
                if ctx.pressed(KeyCode::Enter) {
                    *self = Yahtzee::new();
                }
                return Transition::Stay;
            }
            Phase::Rolling => {
                // Animate the tumble; lock other input until the dice settle.
                self.tumble += ctx.dt;
                while self.tumble >= TUMBLE_STEP {
                    self.tumble -= TUMBLE_STEP;
                    self.scramble_unkept();
                }
                if ctx.dt >= self.roll_timer {
                    self.roll_timer = Duration::ZERO;
                    self.scramble_unkept();
                    self.phase = Phase::Choosing;
                } else {
                    self.roll_timer -= ctx.dt;
                }
                return Transition::Stay;
            }
            Phase::Choosing => {}
        }

        // Toggle keeps with 1..=5 (only meaningful after the first roll).
        if self.rolls_used > 0 && self.rolls_used < MAX_ROLLS {
            for (i, key) in ['1', '2', '3', '4', '5'].into_iter().enumerate() {
                if ctx.pressed(KeyCode::Char(key)) {
                    self.kept[i] = !self.kept[i];
                }
            }
        }

        // Roll again if rolls remain.
        if (ctx.pressed(KeyCode::Char('r')) || ctx.pressed(KeyCode::Char('R')))
            && self.rolls_used < MAX_ROLLS
        {
            self.begin_roll();
            return Transition::Stay;
        }

        // Move the scorecard cursor (skipping nothing — filled rows are inert).
        if ctx.pressed(KeyCode::Up) {
            self.cursor = (self.cursor + CATEGORIES - 1) % CATEGORIES;
        }
        if ctx.pressed(KeyCode::Down) {
            self.cursor = (self.cursor + 1) % CATEGORIES;
        }

        // Commit the highlighted category (only once the dice have been rolled).
        if (ctx.pressed(KeyCode::Enter) || ctx.pressed(KeyCode::Char(' ')))
            && self.rolls_used > 0
            && self.card[self.cursor].is_none()
        {
            let value = ALL_CATEGORIES[self.cursor].score(&self.dice);
            self.card[self.cursor] = Some(value);
            self.start_turn();
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let turn = self.card.iter().filter(|c| c.is_some()).count() + 1;
        let turn = turn.min(CATEGORIES);
        let title = format!(
            " Yahtzee  ·  turn {}/{}  ·  total {} ",
            turn,
            CATEGORIES,
            self.grand_total()
        );

        let field = centered(46, 24, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let mut lines: Vec<Line> = Vec::with_capacity(24);

        // --- Dice row ---------------------------------------------------
        let mut die_spans: Vec<Span> = vec![Span::raw(" ")];
        for i in 0..DICE {
            let face = self.dice[i];
            let style = if self.phase == Phase::Rolling {
                Style::default().fg(Color::DarkGray)
            } else if self.kept[i] {
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            die_spans.push(Span::styled(format!("[{}]", pip(face)), style));
            die_spans.push(Span::raw(" "));
        }
        lines.push(Line::from(die_spans));

        // Slot numbers / keep markers under each die.
        let mut tag_spans: Vec<Span> = vec![Span::raw(" ")];
        for i in 0..DICE {
            let kept = self.kept[i];
            let style = if kept {
                Style::default().fg(Color::LightGreen)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let tag = if kept {
                format!(" {}* ", i + 1)
            } else {
                format!(" {}  ", i + 1)
            };
            tag_spans.push(Span::styled(tag, style));
            tag_spans.push(Span::raw(" "));
        }
        lines.push(Line::from(tag_spans));

        let rolls_left = MAX_ROLLS.saturating_sub(self.rolls_used);
        let status = if self.rolls_used == 0 {
            " Press r to roll".to_string()
        } else {
            format!(" Rolls left: {}", rolls_left)
        };
        lines.push(Line::from(Span::styled(
            status,
            Style::default().fg(Color::Cyan),
        )));
        lines.push(Line::from(""));

        // --- Scorecard --------------------------------------------------
        let preview_value = |idx: usize| ALL_CATEGORIES[idx].score(&self.dice);
        for (idx, cat) in ALL_CATEGORIES.iter().enumerate() {
            let selected = idx == self.cursor && self.phase != Phase::Finished;
            let marker = if selected { ">" } else { " " };

            let (value_text, value_style) = match self.card[idx] {
                Some(v) => (format!("{:>3}", v), Style::default().fg(Color::Gray)),
                None => {
                    if self.rolls_used > 0 {
                        let v = preview_value(idx);
                        let col = if v > 0 {
                            Color::Yellow
                        } else {
                            Color::DarkGray
                        };
                        (format!("{:>3}", v), Style::default().fg(col))
                    } else {
                        ("  -".to_string(), Style::default().fg(Color::DarkGray))
                    }
                }
            };

            let label_style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else if self.card[idx].is_some() {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            };

            let line = Line::from(vec![
                Span::styled(format!(" {} ", marker), Style::default().fg(Color::Cyan)),
                Span::styled(format!("{:<16}", cat.label()), label_style),
                Span::styled(value_text, value_style),
            ]);
            lines.push(line);

            // Insert the bonus summary after the six upper categories.
            if idx == 5 {
                let upper = self.upper_subtotal();
                let bonus = if upper >= BONUS_THRESHOLD { BONUS } else { 0 };
                lines.push(Line::from(Span::styled(
                    format!(
                        " — upper {} / {}   bonus {} ",
                        upper, BONUS_THRESHOLD, bonus
                    ),
                    Style::default().fg(Color::Magenta),
                )));
            }
        }

        frame.render_widget(Paragraph::new(lines), inner);

        // --- Controls hint ----------------------------------------------
        let hint = " 1-5: keep · r: roll · ↑/↓: pick · Enter: score · q: menu ";
        let hint_area = Rect {
            x: field.x,
            y: field.y.saturating_add(field.height),
            width: field.width,
            height: 1,
        };
        if hint_area.y < area.y.saturating_add(area.height) {
            frame.render_widget(
                Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray))),
                hint_area,
            );
        }

        // --- Game-over overlay ------------------------------------------
        if self.phase == Phase::Finished {
            let total = self.grand_total();
            let msg = format!(" FINAL SCORE {} · Enter: replay · q: menu ", total);
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

impl Yahtzee {
    /// Begin a fresh turn: clear keeps and reset the roll counter.
    fn start_turn(&mut self) {
        if self.card.iter().all(|c| c.is_some()) {
            self.phase = Phase::Finished;
            return;
        }
        self.kept = [false; DICE];
        self.rolls_used = 0;
        self.phase = Phase::Choosing;
        self.move_cursor_to_open();
    }

    /// Move the cursor to the first open category at or after its position.
    fn move_cursor_to_open(&mut self) {
        for step in 0..CATEGORIES {
            let idx = (self.cursor + step) % CATEGORIES;
            if self.card[idx].is_none() {
                self.cursor = idx;
                return;
            }
        }
    }

    /// Kick off a roll: consume one roll and start the tumble animation.
    fn begin_roll(&mut self) {
        self.rolls_used += 1;
        // After the final roll, keeps no longer matter — but we still respect
        // existing keeps for the in-between rolls.
        self.phase = Phase::Rolling;
        self.roll_timer = ROLL_TIME;
        self.tumble = Duration::ZERO;
        self.scramble_unkept();
    }

    /// Reroll every die that isn't kept.
    fn scramble_unkept(&mut self) {
        for i in 0..DICE {
            if !self.kept[i] {
                self.dice[i] = (self.next_rand() % 6) as u8 + 1;
            }
        }
    }

    /// Sum of the six upper-section categories scored so far.
    fn upper_subtotal(&self) -> u32 {
        ALL_CATEGORIES
            .iter()
            .zip(self.card.iter())
            .filter(|(cat, _)| cat.is_upper())
            .filter_map(|(_, slot)| *slot)
            .sum()
    }

    /// Final total: every category plus the upper-section bonus.
    fn grand_total(&self) -> u32 {
        let base: u32 = self.card.iter().filter_map(|c| *c).sum();
        let bonus = if self.upper_subtotal() >= BONUS_THRESHOLD {
            BONUS
        } else {
            0
        };
        base + bonus
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

/// Render a die face as a single recognisable glyph.
fn pip(face: u8) -> char {
    match face {
        1 => '⚀',
        2 => '⚁',
        3 => '⚂',
        4 => '⚃',
        5 => '⚄',
        6 => '⚅',
        _ => '?',
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
    Yahtzee,
    id: "yahtzee",
    name: "Yahtzee",
    description: "Roll five dice, fill the 13-category scorecard.",
    author: "furybee",
}
