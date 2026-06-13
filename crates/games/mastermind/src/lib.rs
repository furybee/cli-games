//! Mastermind — guess a hidden 4-slot code drawn from 6 colours.
//!
//! Mirrors the Snake reference: xorshift RNG seeded from the clock, a centred
//! playfield, a short controls hint, and a `Clear` + bordered `Paragraph`
//! game-over overlay. Left/Right pick a slot, Up/Down cycle its colour, Enter
//! submits the row. Each guess scores black pegs (right colour + spot) and
//! white pegs (right colour, wrong spot). Ten guesses to win.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Code length (slots) and number of distinct colours.
const SLOTS: usize = 4;
const COLOURS: usize = 6;
/// Tries the player gets before the code is revealed.
const MAX_GUESSES: usize = 10;

/// The six pickable colours and the glyph used to draw each.
const PALETTE: [(Color, &str); COLOURS] = [
    (Color::Red, "R"),
    (Color::Green, "G"),
    (Color::Blue, "B"),
    (Color::Yellow, "Y"),
    (Color::Magenta, "M"),
    (Color::Cyan, "C"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Playing,
    Won,
    Lost,
}

/// A scored row: the colours guessed plus the peg feedback.
struct Row {
    code: [usize; SLOTS],
    black: usize,
    white: usize,
}

pub struct Mastermind {
    /// The hidden code the player is trying to crack.
    secret: [usize; SLOTS],
    /// All previously submitted, scored guesses (oldest first).
    history: Vec<Row>,
    /// The row currently being assembled.
    current: [usize; SLOTS],
    /// Which slot the cursor sits on.
    cursor: usize,
    phase: Phase,
    rng: u64,
}

impl Game for Mastermind {
    fn new() -> Self {
        let mut game = Mastermind {
            secret: [0; SLOTS],
            history: Vec::with_capacity(MAX_GUESSES),
            current: [0; SLOTS],
            cursor: 0,
            phase: Phase::Playing,
            rng: seed(),
        };
        game.new_secret();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.phase != Phase::Playing {
            if ctx.pressed(KeyCode::Enter) {
                *self = Mastermind::new();
            }
            return Transition::Stay;
        }

        if ctx.pressed(KeyCode::Left) {
            self.cursor = (self.cursor + SLOTS - 1) % SLOTS;
        }
        if ctx.pressed(KeyCode::Right) {
            self.cursor = (self.cursor + 1) % SLOTS;
        }
        if ctx.pressed(KeyCode::Up) {
            let c = &mut self.current[self.cursor];
            *c = (*c + 1) % COLOURS;
        }
        if ctx.pressed(KeyCode::Down) {
            let c = &mut self.current[self.cursor];
            *c = (*c + COLOURS - 1) % COLOURS;
        }
        if ctx.pressed(KeyCode::Enter) {
            self.submit();
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(
            " Mastermind  ·  guess {}/{} ",
            (self.history.len() + 1).min(MAX_GUESSES),
            MAX_GUESSES
        );
        // Width: "NN " + SLOTS*2 glyphs + "  " + pegs (SLOTS*2). Height: rows
        // of history + the active row + a hint line.
        let field_w = 4 + (SLOTS as u16) * 2 + 2 + (SLOTS as u16) * 2 + 2 + 2;
        let field_h = MAX_GUESSES as u16 + 1 + 2 + 2;
        let field = centered(field_w, field_h, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let mut lines: Vec<Line> = Vec::with_capacity(MAX_GUESSES + 2);

        for (i, row) in self.history.iter().enumerate() {
            lines.push(row_line(i + 1, &row.code, Some((row.black, row.white))));
        }

        if self.phase == Phase::Playing {
            lines.push(active_line(
                self.history.len() + 1,
                &self.current,
                self.cursor,
            ));
        }

        // Pad to keep the hint pinned to the bottom of the field.
        while lines.len() < MAX_GUESSES {
            lines.push(Line::from(""));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "← → slot · ↑ ↓ colour · Enter guess · q menu",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            "● black: right spot   ○ white: wrong spot",
            Style::default().fg(Color::DarkGray),
        )));

        frame.render_widget(Paragraph::new(lines), inner);

        if self.phase != Phase::Playing {
            let secret = code_spans(&self.secret);
            let verdict = if self.phase == Phase::Won {
                format!(" CRACKED in {} · ", self.history.len())
            } else {
                " OUT OF GUESSES · code was ".to_string()
            };
            let mut msg = vec![Span::styled(
                verdict,
                Style::default()
                    .fg(if self.phase == Phase::Won {
                        Color::Green
                    } else {
                        Color::Red
                    })
                    .add_modifier(Modifier::BOLD),
            )];
            msg.extend(secret);
            msg.push(Span::styled(
                "  ·  Enter: replay · q: menu ",
                Style::default().fg(Color::Yellow),
            ));

            let width: usize = msg.iter().map(|s| s.content.chars().count()).sum();
            let overlay = centered(width as u16 + 2, 3, area);
            frame.render_widget(Clear, overlay);
            frame.render_widget(
                Paragraph::new(Line::from(msg)).block(Block::default().borders(Borders::ALL)),
                overlay,
            );
        }
    }

    fn tick_rate(&self) -> Duration {
        Duration::from_millis(30)
    }
}

impl Mastermind {
    /// Score the assembled row, store it, and update the win/lose phase.
    fn submit(&mut self) {
        let (black, white) = score(&self.secret, &self.current);
        self.history.push(Row {
            code: self.current,
            black,
            white,
        });

        if black == SLOTS {
            self.phase = Phase::Won;
        } else if self.history.len() >= MAX_GUESSES {
            self.phase = Phase::Lost;
        }

        self.cursor = 0;
    }

    fn new_secret(&mut self) {
        for i in 0..self.secret.len() {
            self.secret[i] = (self.next_rand() % COLOURS as u64) as usize;
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

/// Black = exact matches; white = right colour in the wrong slot, counted with
/// multiplicity over the remaining (non-black) pegs.
fn score(secret: &[usize; SLOTS], guess: &[usize; SLOTS]) -> (usize, usize) {
    let mut black = 0;
    let mut secret_left = [0usize; COLOURS];
    let mut guess_left = [0usize; COLOURS];

    for i in 0..SLOTS {
        if secret[i] == guess[i] {
            black += 1;
        } else {
            secret_left[secret[i]] += 1;
            guess_left[guess[i]] += 1;
        }
    }

    let white = (0..COLOURS)
        .map(|c| secret_left[c].min(guess_left[c]))
        .sum();
    (black, white)
}

/// A scored history row: index, the coloured code, then peg feedback.
fn row_line(index: usize, code: &[usize; SLOTS], pegs: Option<(usize, usize)>) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{index:>2}  "),
        Style::default().fg(Color::DarkGray),
    )];
    spans.extend(code_spans(code));
    if let Some((black, white)) = pegs {
        spans.push(Span::raw("  "));
        for _ in 0..black {
            spans.push(Span::styled("● ", Style::default().fg(Color::White)));
        }
        for _ in 0..white {
            spans.push(Span::styled("○ ", Style::default().fg(Color::Gray)));
        }
    }
    Line::from(spans)
}

/// The in-progress row, with the cursor slot underlined/bracketed.
fn active_line(index: usize, code: &[usize; SLOTS], cursor: usize) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{index:>2}  "),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )];
    for (i, &c) in code.iter().enumerate() {
        let (colour, glyph) = PALETTE[c % COLOURS];
        let mut style = Style::default().fg(colour).add_modifier(Modifier::BOLD);
        if i == cursor {
            style = style
                .add_modifier(Modifier::REVERSED)
                .add_modifier(Modifier::UNDERLINED);
        }
        spans.push(Span::styled(format!("{glyph} "), style));
    }
    Line::from(spans)
}

/// Render a finished code as coloured glyph spans.
fn code_spans(code: &[usize; SLOTS]) -> Vec<Span<'static>> {
    code.iter()
        .map(|&c| {
            let (colour, glyph) = PALETTE[c % COLOURS];
            Span::styled(
                format!("{glyph} "),
                Style::default().fg(colour).add_modifier(Modifier::BOLD),
            )
        })
        .collect()
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
    Mastermind,
    id: "mastermind",
    name: "Mastermind",
    description: "Crack the hidden 4-colour code in ten guesses.",
    author: "furybee",
}
