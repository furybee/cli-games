//! Typing Test — type the target sentence as fast and accurately as you can.
//!
//! Mirrors the Snake reference: `dt`-based timing, a centred playfield, an
//! xorshift RNG seeded from the clock, a bordered overlay on completion, and
//! self-registration. The body renders the target sentence with per-character
//! correct / incorrect colouring plus a cursor, then reports WPM and accuracy.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, KeyEventKind, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Inner field width; sentences are wrapped to fit inside this.
const FIELD_W: u16 = 64;
const FIELD_H: u16 = 12;

/// A standard "word" is five characters for WPM purposes.
const CHARS_PER_WORD: f64 = 5.0;

/// Built-in pool of target sentences.
const SENTENCES: &[&str] = &[
    "The quick brown fox jumps over the lazy dog.",
    "Pack my box with five dozen liquor jugs.",
    "How vexingly quick daft zebras jump!",
    "Sphinx of black quartz, judge my vow.",
    "The five boxing wizards jump quickly.",
    "Bright vixens jump; dozy fowl quack.",
    "A wizard's job is to vex chumps quickly in fog.",
    "Crazy Fredrick bought many very exquisite opal jewels.",
    "We promptly judged antique ivory buckles for the next prize.",
    "Jinxed wizards pluck ivy from the big quilt.",
];

pub struct Typing {
    /// The sentence the player must reproduce, as a list of characters.
    target: Vec<char>,
    /// What the player has typed so far.
    typed: Vec<char>,
    /// Elapsed time since the first keystroke of the current run.
    elapsed: Duration,
    /// `true` once the player has typed at least one character.
    started: bool,
    /// `true` once the typed text matches the target length.
    finished: bool,
    rng: u64,
}

impl Game for Typing {
    fn new() -> Self {
        let mut game = Typing {
            target: Vec::new(),
            typed: Vec::new(),
            elapsed: Duration::ZERO,
            started: false,
            finished: false,
            rng: seed(),
        };
        game.pick_sentence();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        // Enter / Tab always loads a fresh sentence (and is the "play again"
        // action on the completion screen).
        if ctx.pressed(KeyCode::Enter) || ctx.pressed(KeyCode::Tab) {
            self.pick_sentence();
            return Transition::Stay;
        }

        // Accumulate time only while a run is in progress.
        if self.started && !self.finished {
            self.elapsed += ctx.dt;
        }

        if self.finished {
            return Transition::Stay;
        }

        for key in ctx.keys() {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Backspace => {
                    self.typed.pop();
                }
                KeyCode::Char(c) if self.typed.len() < self.target.len() => {
                    if !self.started {
                        self.started = true;
                        self.elapsed = Duration::ZERO;
                    }
                    self.typed.push(c);
                    if self.typed.len() == self.target.len() {
                        self.finished = true;
                    }
                }
                _ => {}
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(" Typing Test  ·  WPM {:.0} ", self.wpm());
        let field = centered(FIELD_W + 2, FIELD_H + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let cursor = self.typed.len();
        let inner_w = inner.width.max(1) as usize;

        // Build coloured spans for every target character, wrapping into lines.
        let mut lines: Vec<Line> = Vec::new();
        let mut spans: Vec<Span> = Vec::new();
        let mut col = 0usize;
        for (i, &ch) in self.target.iter().enumerate() {
            if col >= inner_w {
                lines.push(Line::from(std::mem::take(&mut spans)));
                col = 0;
            }
            let display: String = if ch == ' ' {
                " ".into()
            } else {
                ch.to_string()
            };
            let style = if i < cursor {
                if self.typed.get(i) == Some(&ch) {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Red)
                        .add_modifier(Modifier::BOLD)
                }
            } else if i == cursor && !self.finished {
                // The cursor position: highlight the upcoming character.
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            spans.push(Span::styled(display, style));
            col += 1;
        }
        lines.push(Line::from(std::mem::take(&mut spans)));

        // Stats line.
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("time ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.1}s", self.elapsed.as_secs_f64()),
                Style::default().fg(Color::White),
            ),
            Span::raw("   "),
            Span::styled("accuracy ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.0}%", self.accuracy()),
                Style::default().fg(Color::White),
            ),
        ]));

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Enter/Tab: new sentence   ·   Backspace: correct   ·   Esc: menu",
            Style::default().fg(Color::DarkGray),
        )));

        frame.render_widget(Paragraph::new(lines), inner);

        if self.finished {
            let msg = format!(
                " DONE · {:.0} WPM · {:.0}% accuracy · {:.1}s · Enter: again ",
                self.wpm(),
                self.accuracy(),
                self.elapsed.as_secs_f64(),
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

impl Typing {
    /// Load a fresh random sentence and reset the run.
    fn pick_sentence(&mut self) {
        let idx = if SENTENCES.is_empty() {
            0
        } else {
            (self.next_rand() % SENTENCES.len() as u64) as usize
        };
        let sentence = SENTENCES.get(idx).copied().unwrap_or("type here");
        self.target = sentence.chars().collect();
        self.typed.clear();
        self.elapsed = Duration::ZERO;
        self.started = false;
        self.finished = false;
    }

    /// Number of typed characters that match the target.
    fn correct_count(&self) -> usize {
        self.typed
            .iter()
            .zip(self.target.iter())
            .filter(|(a, b)| a == b)
            .count()
    }

    /// Accuracy over the characters typed so far (0–100).
    fn accuracy(&self) -> f64 {
        if self.typed.is_empty() {
            100.0
        } else {
            self.correct_count() as f64 / self.typed.len() as f64 * 100.0
        }
    }

    /// Words-per-minute based on correctly typed characters and elapsed time.
    fn wpm(&self) -> f64 {
        let secs = self.elapsed.as_secs_f64();
        if secs <= 0.0 {
            return 0.0;
        }
        let minutes = secs / 60.0;
        (self.correct_count() as f64 / CHARS_PER_WORD) / minutes
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
    Typing,
    id: "typing",
    name: "Typing Test",
    description: "Type the sentence — measure your WPM and accuracy.",
    author: "furybee",
}
