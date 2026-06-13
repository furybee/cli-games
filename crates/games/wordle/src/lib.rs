//! Wordle — guess the hidden five-letter word in six tries.
//!
//! Type a guess and press Enter; each tile then reveals whether the letter is
//! in the right spot (green), in the word elsewhere (yellow), or absent (gray).
//! An on-screen keyboard tracks what you've learned. The crate stays
//! dependency-free: words are embedded and randomness is a small xorshift.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, KeyEventKind, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

const WORD_LEN: usize = 5;
const MAX_GUESSES: usize = 6;
/// How long the "not in word list" hint stays on screen.
const HINT_TIME: Duration = Duration::from_millis(1500);

/// State of a single letter, both per-tile and accumulated on the keyboard.
/// Ordering matters: a better-known state never downgrades (`Correct > Present`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Mark {
    Unknown,
    Absent,
    Present,
    Correct,
}

impl Mark {
    fn color(self) -> Color {
        match self {
            Mark::Unknown => Color::DarkGray,
            Mark::Absent => Color::Rgb(58, 58, 60),
            Mark::Present => Color::Rgb(181, 159, 59),
            Mark::Correct => Color::Rgb(83, 141, 78),
        }
    }
}

#[derive(PartialEq, Eq)]
enum Status {
    Playing,
    Won,
    Lost,
}

pub struct Wordle {
    answer: [char; WORD_LEN],
    /// Submitted guesses, each scored into per-tile marks.
    guesses: Vec<([char; WORD_LEN], [Mark; WORD_LEN])>,
    /// The row currently being typed.
    current: String,
    status: Status,
    /// Best state learned for each keyboard letter, indexed by `letter - 'A'`.
    keyboard: [Mark; 26],
    /// Transient "not in word list" hint and its remaining lifetime.
    hint: Option<Duration>,
}

impl Game for Wordle {
    fn new() -> Self {
        let mut rng = seed();
        let answer = pick_answer(&mut rng);
        Wordle {
            answer,
            guesses: Vec::with_capacity(MAX_GUESSES),
            current: String::new(),
            status: Status::Playing,
            keyboard: [Mark::Unknown; 26],
            hint: None,
        }
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        // `q` is a typeable letter here, so only Esc leaves to the menu.
        if ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        // Fade out a stale hint.
        if let Some(left) = self.hint {
            self.hint = left.checked_sub(ctx.dt).filter(|d| !d.is_zero());
        }

        if self.status != Status::Playing {
            if ctx.pressed(KeyCode::Enter) {
                *self = Wordle::new();
            }
            return Transition::Stay;
        }

        // Handle keys in arrival order so fast typing lands letters in sequence.
        for key in ctx.keys() {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char(c) if c.is_ascii_alphabetic() => {
                    if self.current.len() < WORD_LEN {
                        self.current.push(c.to_ascii_uppercase());
                    }
                }
                KeyCode::Backspace => {
                    self.current.pop();
                }
                KeyCode::Enter => self.submit(),
                _ => {}
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = match self.status {
            Status::Playing => format!(
                " Wordle  ·  guess {}/{} ",
                self.guesses.len() + 1,
                MAX_GUESSES
            ),
            Status::Won => " Wordle  ·  solved! ".to_string(),
            Status::Lost => " Wordle  ·  out of guesses ".to_string(),
        };
        // Board: 5 tiles (4 wide each) + a keyboard block underneath.
        let board_w = (WORD_LEN as u16) * 4 + 1;
        let board_h = (MAX_GUESSES as u16) * 2 + 1 + 5;
        let field = centered(board_w + 2, board_h + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let mut lines: Vec<Line> = Vec::new();
        for row in 0..MAX_GUESSES {
            lines.push(self.tile_row(row));
            lines.push(Line::raw(""));
        }
        lines.push(Line::raw(""));
        lines.extend(self.keyboard_rows());
        frame.render_widget(Paragraph::new(lines), inner);

        if let Some(msg) = self.overlay_text() {
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
}

impl Wordle {
    /// Validate and score the typed word, then advance the game state.
    fn submit(&mut self) {
        if self.current.len() < WORD_LEN {
            return;
        }
        let guess: [char; WORD_LEN] = to_array(&self.current);
        if !WORDS.contains(&self.current.to_ascii_lowercase().as_str()) {
            self.hint = Some(HINT_TIME);
            return;
        }

        let marks = score(&guess, &self.answer);
        for (i, &m) in marks.iter().enumerate() {
            let idx = (guess[i] as u8 - b'A') as usize;
            if m > self.keyboard[idx] {
                self.keyboard[idx] = m;
            }
        }
        self.guesses.push((guess, marks));
        self.current.clear();
        self.hint = None;

        if marks.iter().all(|&m| m == Mark::Correct) {
            self.status = Status::Won;
        } else if self.guesses.len() >= MAX_GUESSES {
            self.status = Status::Lost;
        }
    }

    /// Build one board row: a committed guess, the active input, or empty tiles.
    fn tile_row(&self, row: usize) -> Line<'static> {
        let mut spans = Vec::with_capacity(WORD_LEN);
        if let Some((word, marks)) = self.guesses.get(row) {
            for i in 0..WORD_LEN {
                spans.push(tile(word[i], marks[i].color()));
                spans.push(Span::raw(" "));
            }
        } else if row == self.guesses.len() && self.status == Status::Playing {
            let typed: Vec<char> = self.current.chars().collect();
            for i in 0..WORD_LEN {
                let ch = typed.get(i).copied().unwrap_or(' ');
                spans.push(tile(ch, Color::Rgb(40, 40, 42)));
                spans.push(Span::raw(" "));
            }
        } else {
            for _ in 0..WORD_LEN {
                spans.push(tile(' ', Color::Rgb(28, 28, 30)));
                spans.push(Span::raw(" "));
            }
        }
        Line::from(spans)
    }

    /// The QWERTY keyboard, each key tinted by what's been learned about it.
    fn keyboard_rows(&self) -> Vec<Line<'static>> {
        ["QWERTYUIOP", "ASDFGHJKL", "ZXCVBNM"]
            .iter()
            .map(|row| {
                let mut spans = Vec::new();
                for ch in row.chars() {
                    let mark = self.keyboard[(ch as u8 - b'A') as usize];
                    spans.push(Span::styled(
                        format!(" {ch} "),
                        Style::default().bg(mark.color()).fg(Color::White),
                    ));
                    spans.push(Span::raw(" "));
                }
                Line::from(spans)
            })
            .collect()
    }

    fn overlay_text(&self) -> Option<String> {
        match self.status {
            Status::Won => Some(" SOLVED!  ·  Enter: play again · Esc: menu ".to_string()),
            Status::Lost => Some(format!(
                " The word was {}  ·  Enter: retry · Esc: menu ",
                self.answer.iter().collect::<String>()
            )),
            Status::Playing if self.hint.is_some() => Some(" Not in word list ".to_string()),
            Status::Playing => None,
        }
    }
}

/// Score a guess against the answer with correct duplicate-letter handling:
/// greens are claimed first, then yellows draw from the remaining letters.
fn score(guess: &[char; WORD_LEN], answer: &[char; WORD_LEN]) -> [Mark; WORD_LEN] {
    let mut marks = [Mark::Absent; WORD_LEN];
    let mut remaining = [0u8; 26];
    for &c in answer {
        remaining[(c as u8 - b'A') as usize] += 1;
    }
    for i in 0..WORD_LEN {
        if guess[i] == answer[i] {
            marks[i] = Mark::Correct;
            remaining[(guess[i] as u8 - b'A') as usize] -= 1;
        }
    }
    for i in 0..WORD_LEN {
        if marks[i] == Mark::Correct {
            continue;
        }
        let idx = (guess[i] as u8 - b'A') as usize;
        if remaining[idx] > 0 {
            marks[i] = Mark::Present;
            remaining[idx] -= 1;
        }
    }
    marks
}

/// A single uppercase tile with a colored background.
fn tile(ch: char, bg: Color) -> Span<'static> {
    Span::styled(
        format!(" {ch} "),
        Style::default()
            .bg(bg)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )
}

fn to_array(s: &str) -> [char; WORD_LEN] {
    let mut out = [' '; WORD_LEN];
    for (i, c) in s.chars().take(WORD_LEN).enumerate() {
        out[i] = c.to_ascii_uppercase();
    }
    out
}

fn pick_answer(rng: &mut u64) -> [char; WORD_LEN] {
    let mut x = *rng;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *rng = x;
    let word = WORDS[(x % WORDS.len() as u64) as usize];
    to_array(word)
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

/// Embedded five-letter words — both the answer pool and the accepted-guess set.
/// Lowercase; guesses are matched case-insensitively against this list.
#[rustfmt::skip]
const WORDS: &[&str] = &[
    "about", "above", "abuse", "actor", "acute", "admit", "adopt", "adult", "after", "again",
    "agent", "agree", "ahead", "alarm", "album", "alert", "alike", "alive", "allow", "alone",
    "along", "alter", "among", "anger", "angle", "angry", "apart", "apple", "apply", "arena",
    "argue", "arise", "armor", "array", "arrow", "aside", "asset", "audio", "audit", "avoid",
    "award", "aware", "badly", "baker", "bases", "basic", "beach", "began", "begin", "being",
    "below", "bench", "billy", "birth", "black", "blade", "blame", "blank", "blast", "blind",
    "block", "blood", "board", "boost", "booth", "bound", "brain", "brand", "brave", "bread",
    "break", "breed", "brick", "bride", "brief", "bring", "broad", "broke", "brown", "build",
    "built", "buyer", "cabin", "cable", "calif", "carry", "catch", "cause", "chain", "chair",
    "chaos", "charm", "chart", "chase", "cheap", "check", "chest", "chief", "child", "china",
    "chose", "civil", "claim", "class", "clean", "clear", "click", "climb", "clock", "close",
    "cloud", "coach", "coast", "could", "count", "court", "cover", "craft", "crash", "crazy",
    "cream", "crime", "cross", "crowd", "crown", "crude", "curve", "cycle", "daily", "dance",
    "dated", "dealt", "death", "debut", "delay", "depth", "doing", "doubt", "dozen", "draft",
    "drama", "drank", "dream", "dress", "drill", "drink", "drive", "drove", "dying", "eager",
    "early", "earth", "eight", "elite", "empty", "enemy", "enjoy", "enter", "entry", "equal",
    "error", "event", "every", "exact", "exist", "extra", "faith", "false", "fault", "fiber",
    "field", "fifth", "fifty", "fight", "final", "first", "fixed", "flash", "fleet", "floor",
    "fluid", "focus", "force", "forth", "forty", "forum", "found", "frame", "frank", "fraud",
    "fresh", "front", "fruit", "fully", "funny", "giant", "given", "glass", "globe", "going",
    "grace", "grade", "grand", "grant", "grass", "grave", "great", "green", "gross", "group",
    "grown", "guard", "guess", "guest", "guide", "happy", "harry", "heart", "heavy", "hence",
    "henry", "horse", "hotel", "house", "human", "ideal", "image", "index", "inner", "input",
    "issue", "japan", "jimmy", "joint", "jones", "judge", "known", "label", "large", "laser",
    "later", "laugh", "layer", "learn", "lease", "least", "leave", "legal", "level", "lewis",
    "light", "limit", "links", "lives", "local", "logic", "loose", "lower", "lucky", "lunch",
    "lying", "magic", "major", "maker", "march", "maria", "match", "maybe", "mayor", "meant",
    "media", "metal", "might", "minor", "minus", "mixed", "model", "money", "month", "moral",
    "motor", "mount", "mouse", "mouth", "movie", "music", "needs", "never", "newly", "night",
    "noise", "north", "noted", "novel", "nurse", "occur", "ocean", "offer", "often", "order",
    "other", "ought", "paint", "panel", "paper", "party", "peace", "peter", "phase", "phone",
    "photo", "piece", "pilot", "pitch", "place", "plain", "plane", "plant", "plate", "point",
    "pound", "power", "press", "price", "pride", "prime", "print", "prior", "prize", "proof",
    "proud", "prove", "queen", "quick", "quiet", "quite", "radio", "raise", "range", "rapid",
    "ratio", "reach", "ready", "refer", "right", "rival", "river", "robin", "roger", "roman",
    "rough", "round", "route", "royal", "rural", "scale", "scene", "scope", "score", "sense",
    "serve", "seven", "shall", "shape", "share", "sharp", "sheet", "shelf", "shell", "shift",
    "shirt", "shock", "shoot", "short", "shown", "sight", "since", "sixth", "sixty", "sized",
    "skill", "sleep", "slide", "small", "smart", "smile", "smith", "smoke", "solid", "solve",
    "sorry", "sound", "south", "space", "spare", "speak", "speed", "spend", "spent", "split",
    "spoke", "sport", "staff", "stage", "stake", "stand", "start", "state", "steam", "steel",
    "stick", "still", "stock", "stone", "stood", "store", "storm", "story", "strip", "stuck",
    "study", "stuff", "style", "sugar", "suite", "super", "sweet", "table", "taken", "taste",
    "taxes", "teach", "teeth", "terry", "texas", "thank", "theft", "their", "theme", "there",
    "these", "thick", "thing", "think", "third", "those", "three", "threw", "throw", "tight",
    "times", "tired", "title", "today", "topic", "total", "touch", "tough", "tower", "track",
    "trade", "train", "treat", "trend", "trial", "tribe", "trick", "tried", "tries", "truck",
    "truly", "trust", "truth", "twice", "under", "undue", "union", "unity", "until", "upper",
    "upset", "urban", "usage", "usual", "valid", "value", "video", "virus", "visit", "vital",
    "voice", "waste", "watch", "water", "wheel", "where", "which", "while", "white", "whole",
    "whose", "woman", "women", "world", "worry", "worse", "worst", "worth", "would", "wound",
    "write", "wrong", "wrote", "yield", "young", "youth",
];

register_game! {
    Wordle,
    id: "wordle",
    name: "Wordle",
    description: "Guess the hidden five-letter word in six tries.",
    author: "furybee",
}
