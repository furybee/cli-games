//! Anagram — type the unscrambled word before the timer runs out.
//!
//! Mirrors the Snake reference: `dt`-based timing (the countdown), a centred
//! playfield, typed input via `ctx.keys()`, a game-over overlay, and
//! self-registration. Dependency-free randomness uses the same xorshift + clock
//! seed pattern.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, KeyEventKind, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Built-in word list. Kept to clean, lowercase, 4–8 letter words.
const WORDS: &[&str] = &[
    "puzzle", "garden", "rocket", "planet", "silver", "forest", "castle", "wizard", "dragon",
    "marble", "candle", "anchor", "bridge", "copper", "danger", "engine", "flavor", "guitar",
    "hunter", "island", "jungle", "kettle", "ladder", "magnet", "nature", "orange", "pencil",
    "rabbit", "saddle", "temple", "velvet", "wisdom", "yellow", "zigzag", "basket", "circle",
    "diamond", "feather", "harvest", "lantern", "monster", "octopus", "rainbow", "treasure",
    "village", "whisper", "blossom", "captain",
];

/// Seconds allowed for each word before it expires.
const TIME_PER_WORD: f32 = 15.0;
/// Width of the centred play panel.
const PANEL_W: u16 = 44;
/// Height of the centred play panel.
const PANEL_H: u16 = 13;

pub struct Anagram {
    /// The word the player must produce.
    answer: String,
    /// The shuffled letters shown on screen.
    scrambled: String,
    /// What the player has typed so far.
    typed: String,
    /// Seconds remaining on the current word.
    time_left: f32,
    score: u32,
    /// Number of words skipped or timed out.
    misses: u32,
    /// Brief flash shown after a correct/wrong/skip event.
    feedback: Option<(String, Color)>,
    /// How long the current feedback flash stays on screen.
    feedback_left: f32,
    /// `true` once the timer has expired with the wrong word.
    over: bool,
    rng: u64,
}

impl Game for Anagram {
    fn new() -> Self {
        let mut game = Anagram {
            answer: String::new(),
            scrambled: String::new(),
            typed: String::new(),
            time_left: TIME_PER_WORD,
            score: 0,
            misses: 0,
            feedback: None,
            feedback_left: 0.0,
            over: false,
            rng: seed(),
        };
        game.load_word();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.over {
            if ctx.pressed(KeyCode::Enter) {
                *self = Anagram::new();
            }
            if ctx.pressed(KeyCode::Char('q')) {
                return Transition::Exit;
            }
            return Transition::Stay;
        }

        let dt = ctx.dt.as_secs_f32();

        if self.feedback_left > 0.0 {
            self.feedback_left -= dt;
            if self.feedback_left <= 0.0 {
                self.feedback = None;
            }
        }

        // Handle typed input. Letters extend the guess; Backspace removes;
        // Enter submits; the skip key loads a fresh word.
        for key in ctx.keys() {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char(c) if c.is_ascii_alphabetic() => {
                    if self.typed.chars().count() < self.answer.chars().count() {
                        self.typed.push(c.to_ascii_lowercase());
                    }
                }
                KeyCode::Backspace => {
                    self.typed.pop();
                }
                KeyCode::Tab => self.skip(),
                KeyCode::Enter => self.submit(),
                _ => {}
            }
        }

        // Countdown — running out of time on a word ends the run.
        self.time_left -= dt;
        if self.time_left <= 0.0 {
            self.time_left = 0.0;
            self.misses += 1;
            self.feedback = Some((format!("Time! It was \"{}\"", self.answer), Color::Red));
            self.feedback_left = 1.5;
            self.over = true;
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(
            " Anagram  ·  score {}  ·  misses {} ",
            self.score, self.misses
        );
        let panel = centered(PANEL_W, PANEL_H, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(panel);
        frame.render_widget(block, panel);

        // Timer bar: a row of blocks that drains as time runs low.
        let ratio = (self.time_left / TIME_PER_WORD).clamp(0.0, 1.0);
        let bar_w = PANEL_W.saturating_sub(4) as usize;
        let filled = (ratio * bar_w as f32).round() as usize;
        let bar_color = if ratio > 0.5 {
            Color::Green
        } else if ratio > 0.25 {
            Color::Yellow
        } else {
            Color::Red
        };
        let bar = Line::from(vec![
            Span::styled("█".repeat(filled), Style::default().fg(bar_color)),
            Span::styled(
                "░".repeat(bar_w.saturating_sub(filled)),
                Style::default().fg(Color::DarkGray),
            ),
        ]);

        // Scrambled letters, spaced out for readability.
        let scrambled: String = self
            .scrambled
            .chars()
            .map(|c| c.to_ascii_uppercase().to_string())
            .collect::<Vec<_>>()
            .join(" ");

        // The guess line, padded with underscores for the missing letters.
        let total = self.answer.chars().count();
        let so_far = self.typed.chars().count();
        let mut guess_spans: Vec<Span> = self
            .typed
            .chars()
            .map(|c| {
                Span::styled(
                    format!("{} ", c.to_ascii_uppercase()),
                    Style::default()
                        .fg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD),
                )
            })
            .collect();
        for _ in so_far..total {
            guess_spans.push(Span::styled("_ ", Style::default().fg(Color::DarkGray)));
        }

        let feedback_line = match &self.feedback {
            Some((text, color)) => Line::from(Span::styled(
                text.clone(),
                Style::default().fg(*color).add_modifier(Modifier::BOLD),
            )),
            None => Line::from(Span::raw("")),
        };

        let lines = vec![
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                format!("{:.0}s left", self.time_left.ceil()),
                Style::default().fg(bar_color),
            )),
            bar,
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "Unscramble:",
                Style::default().fg(Color::Gray),
            )),
            Line::from(Span::styled(
                scrambled,
                Style::default()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
            Line::from(guess_spans),
            Line::from(Span::raw("")),
            feedback_line,
            Line::from(Span::styled(
                "type letters · Enter: submit · Tab: skip · q/Esc: menu",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        frame.render_widget(Paragraph::new(lines), inner);

        if self.over {
            let msg = format!(
                " GAME OVER · score {} · Enter: replay · q: menu ",
                self.score
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

impl Anagram {
    /// Check the current guess; correct answers score and advance.
    fn submit(&mut self) {
        if self.typed == self.answer {
            self.score += 1;
            self.feedback = Some(("Correct!".to_string(), Color::Green));
            self.feedback_left = 1.0;
            self.load_word();
        } else {
            self.feedback = Some(("Not quite — keep trying".to_string(), Color::Red));
            self.feedback_left = 1.0;
            self.typed.clear();
        }
    }

    /// Give up on the current word and load a new one (counts as a miss).
    fn skip(&mut self) {
        self.misses += 1;
        self.feedback = Some((format!("Skipped \"{}\"", self.answer), Color::Yellow));
        self.feedback_left = 1.2;
        self.load_word();
    }

    /// Pick a fresh word, scramble it, and reset the per-word timer.
    fn load_word(&mut self) {
        let idx = (self.next_rand() % WORDS.len() as u64) as usize;
        let word = WORDS.get(idx).copied().unwrap_or("anagram");
        self.answer = word.to_string();
        self.scrambled = self.scramble(word);
        self.typed.clear();
        self.time_left = TIME_PER_WORD;
    }

    /// Fisher–Yates shuffle of the letters; retries so the scramble differs
    /// from the answer for words longer than one letter.
    fn scramble(&mut self, word: &str) -> String {
        let original: Vec<char> = word.chars().collect();
        if original.len() < 2 {
            return word.to_string();
        }
        for _ in 0..8 {
            let mut chars = original.clone();
            for i in (1..chars.len()).rev() {
                let j = (self.next_rand() % (i as u64 + 1)) as usize;
                chars.swap(i, j);
            }
            if chars != original {
                return chars.into_iter().collect();
            }
        }
        original.into_iter().collect()
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
    Anagram,
    id: "anagram",
    name: "Anagram",
    description: "Unscramble the word before the timer runs out.",
    author: "furybee",
}
