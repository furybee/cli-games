//! Hangman — guess the hidden word one letter at a time before the gallows
//! drawing is completed. Type letters `a`–`z` to guess; six wrong guesses lose.
//!
//! Self-contained and dependency-free beyond `game_core` + `ratatui`: the word
//! list is baked in and word selection uses a small xorshift RNG.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Wrong guesses allowed before the drawing is complete and the round is lost.
const MAX_WRONG: usize = 6;

/// Words to guess. Kept lowercase ASCII so guessing stays simple.
const WORDS: &[&str] = &[
    "rust",
    "cargo",
    "borrow",
    "trait",
    "macro",
    "vector",
    "closure",
    "lifetime",
    "ownership",
    "compiler",
    "terminal",
    "keyboard",
    "function",
    "iterator",
    "pattern",
    "channel",
    "gallows",
    "mystery",
    "puzzle",
    "victory",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Playing,
    Won,
    Lost,
}

pub struct Hangman {
    /// The hidden word, as lowercase chars.
    word: Vec<char>,
    /// Letters the player has guessed, right or wrong.
    guessed: HashSet<char>,
    /// Wrong guesses so far; drives the gallows drawing.
    wrong: usize,
    status: Status,
    rng: u64,
}

impl Game for Hangman {
    fn new() -> Self {
        let mut game = Hangman {
            word: Vec::new(),
            guessed: HashSet::new(),
            wrong: 0,
            status: Status::Playing,
            rng: seed(),
        };
        game.pick_word();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.status != Status::Playing {
            if ctx.pressed(KeyCode::Enter) {
                *self = Hangman::new();
            }
            return Transition::Stay;
        }

        // A single tick may carry several key events; process each letter once.
        for key in ctx.keys() {
            if let KeyCode::Char(c) = key.code {
                let c = c.to_ascii_lowercase();
                if c.is_ascii_alphabetic() {
                    self.guess(c);
                }
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(" Hangman  ·  {}/{} wrong ", self.wrong, MAX_WRONG);
        let panel = centered(40, 18, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(panel);
        frame.render_widget(block, panel);

        let mut lines: Vec<Line> = GALLOWS[self.wrong.min(MAX_WRONG)]
            .iter()
            .map(|&row| Line::from(Span::raw(row)))
            .collect();

        lines.push(Line::from(""));

        // The word, with revealed letters and blanks for the rest.
        let reveal = self.status == Status::Lost;
        let word: Vec<Span> = self
            .word
            .iter()
            .map(|&c| {
                if self.guessed.contains(&c) {
                    Span::styled(
                        format!("{c} "),
                        Style::default()
                            .fg(Color::LightGreen)
                            .add_modifier(Modifier::BOLD),
                    )
                } else if reveal {
                    Span::styled(format!("{c} "), Style::default().fg(Color::Red))
                } else {
                    Span::styled("_ ", Style::default().fg(Color::DarkGray))
                }
            })
            .collect();
        lines.push(Line::from(word));

        lines.push(Line::from(""));

        // Wrong guesses, so the player can track what they've tried.
        let misses: String = {
            let mut m: Vec<char> = self
                .guessed
                .iter()
                .copied()
                .filter(|c| !self.word.contains(c))
                .collect();
            m.sort_unstable();
            m.iter().map(|c| format!("{c} ")).collect()
        };
        lines.push(Line::from(Span::styled(
            format!("missed: {misses}"),
            Style::default().fg(Color::Red),
        )));

        frame.render_widget(Paragraph::new(lines), inner);

        if self.status != Status::Playing {
            let msg = match self.status {
                Status::Won => " YOU WON · Enter: replay · q: menu ".to_string(),
                _ => {
                    let answer: String = self.word.iter().collect();
                    format!(" GAME OVER · word was '{answer}' · Enter: replay · q: menu ")
                }
            };
            let color = if self.status == Status::Won {
                Color::Green
            } else {
                Color::Yellow
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
}

impl Hangman {
    /// Record a letter guess and update wrong count / win-loss status.
    fn guess(&mut self, c: char) {
        if !self.guessed.insert(c) {
            return; // already tried this letter
        }
        if self.word.contains(&c) {
            if self.word.iter().all(|w| self.guessed.contains(w)) {
                self.status = Status::Won;
            }
        } else {
            self.wrong += 1;
            if self.wrong >= MAX_WRONG {
                self.status = Status::Lost;
            }
        }
    }

    fn pick_word(&mut self) {
        let idx = (self.next_rand() % WORDS.len() as u64) as usize;
        self.word = WORDS[idx].chars().collect();
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

/// Progressive gallows art, indexed by wrong-guess count (0..=MAX_WRONG).
const GALLOWS: [[&str; 7]; MAX_WRONG + 1] = [
    [
        "  +---+",
        "  |   |",
        "      |",
        "      |",
        "      |",
        "      |",
        "=========",
    ],
    [
        "  +---+",
        "  |   |",
        "  O   |",
        "      |",
        "      |",
        "      |",
        "=========",
    ],
    [
        "  +---+",
        "  |   |",
        "  O   |",
        "  |   |",
        "      |",
        "      |",
        "=========",
    ],
    [
        "  +---+",
        "  |   |",
        "  O   |",
        " /|   |",
        "      |",
        "      |",
        "=========",
    ],
    [
        "  +---+",
        "  |   |",
        "  O   |",
        " /|\\  |",
        "      |",
        "      |",
        "=========",
    ],
    [
        "  +---+",
        "  |   |",
        "  O   |",
        " /|\\  |",
        " /    |",
        "      |",
        "=========",
    ],
    [
        "  +---+",
        "  |   |",
        "  O   |",
        " /|\\  |",
        " / \\  |",
        "      |",
        "=========",
    ],
];

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
    Hangman,
    id: "hangman",
    name: "Hangman",
    description: "Guess the word before the gallows fill up.",
    author: "furybee",
}
