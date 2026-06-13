//! Boggle — spell words by tracing a path through adjacent letters.
//!
//! A 4x4 grid of random letters is dealt. Type a word and press Enter: it
//! scores only if every consecutive letter sits in a cell adjacent (including
//! diagonally) to the previous one, no cell is reused, and the word is at least
//! three letters long. Points grow with word length, and a countdown timer ends
//! the round. Mirrors the `snake` reference: `dt` timing, a centred playfield, a
//! game-over overlay, and `register_game!` self-registration.

use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, KeyEventKind, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Grid is `SIZE`×`SIZE` cells.
const SIZE: usize = 4;
/// Length of a round.
const ROUND: Duration = Duration::from_secs(90);

/// Letter frequency bag (roughly Scrabble-like) so grids tend to be playable.
const BAG: &[u8] = b"AAAAAAAAABBCCDDDDEEEEEEEEEEEEFFGGGHHIIIIIIIIIJKLLLLMMNNNNNNOOOOOOOOPPQRRRRRRSSSSTTTTTTUUUUVVWWXYYZ";

/// A handful of common words so a tiny dictionary can reward real words.
/// Words that pass the adjacency check still score even if absent here.
const DICT: &[&str] = &[
    "CAT", "DOG", "ATE", "EAT", "TEA", "EAR", "ARE", "ERA", "ART", "RAT", "TAR", "ANT", "TAN",
    "NET", "TEN", "TON", "NOT", "ONE", "EON", "OAR", "ORE", "ROE", "RIB", "RID", "DIN", "TIN",
    "NIT", "SIT", "ITS", "SIN", "INS", "SUN", "NUT", "RUN", "URN", "GUN", "DUG", "MUD", "RUM",
    "ARM", "RAM", "MAR", "MAT", "TAM", "HAT", "THE", "HEN", "HER", "TREE", "REST", "RATE", "TEAR",
    "TARE", "NEAR", "EARN", "RAIN", "RANT", "TARN", "STAR", "RATS", "ARTS", "TARS", "STONE",
    "NOTES", "TONES", "STARE", "TEARS", "RATES", "TARES",
];

pub struct Boggle {
    grid: [[char; SIZE]; SIZE],
    /// Current word the player is typing.
    input: String,
    /// Words already accepted this round.
    found: Vec<String>,
    /// Short-lived status line after an Enter (accept / reject reason).
    message: String,
    score: u32,
    remaining: Duration,
    over: bool,
    rng: u64,
}

impl Game for Boggle {
    fn new() -> Self {
        let mut game = Boggle {
            grid: [[' '; SIZE]; SIZE],
            input: String::new(),
            found: Vec::new(),
            message: String::from("Type a word, Enter to submit."),
            score: 0,
            remaining: ROUND,
            over: false,
            rng: seed(),
        };
        game.deal();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.over {
            if ctx.pressed(KeyCode::Enter) {
                *self = Boggle::new();
            }
            return Transition::Stay;
        }

        // Countdown; saturating so it never wraps when dt overshoots.
        self.remaining = self.remaining.saturating_sub(ctx.dt);
        if self.remaining == Duration::ZERO {
            self.over = true;
            self.message = String::from("Time!");
            return Transition::Stay;
        }

        // Collect typed characters and editing keys for this tick.
        for key in ctx.keys() {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char(c) if c.is_ascii_alphabetic() => {
                    if self.input.chars().count() < SIZE * SIZE {
                        self.input.push(c.to_ascii_uppercase());
                    }
                }
                KeyCode::Backspace => {
                    self.input.pop();
                }
                KeyCode::Enter => self.submit(),
                _ => {}
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(
            " Boggle  ·  score {}  ·  {:>2}s ",
            self.score,
            self.remaining.as_secs()
        );
        // Grid cells render two columns wide; reserve space for the side panel.
        let board_w = (SIZE as u16) * 4 + 2;
        let board_h = (SIZE as u16) * 2 + 7;
        let panel_w = 26;
        let field = centered(board_w + panel_w, board_h, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::raw(""));
        for row in &self.grid {
            let mut spans = Vec::with_capacity(SIZE);
            for &ch in row {
                spans.push(Span::styled(
                    format!(" {ch} "),
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::raw(" "));
            }
            lines.push(Line::from(spans));
            lines.push(Line::raw(""));
        }

        lines.push(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Yellow)),
            Span::styled(
                self.input.clone(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::styled(
            self.message.clone(),
            Style::default().fg(Color::Gray),
        ));
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Enter submit · Backspace · q menu",
            Style::default().fg(Color::DarkGray),
        ));

        // Side list of found words, newest last.
        let mut panel: Vec<Line> = Vec::new();
        panel.push(Line::styled(
            format!("Words ({})", self.found.len()),
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        ));
        let max_rows = inner.height.saturating_sub(1) as usize;
        let start = self.found.len().saturating_sub(max_rows);
        for word in &self.found[start..] {
            panel.push(Line::styled(
                word.clone(),
                Style::default().fg(Color::Green),
            ));
        }

        let board_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width.saturating_sub(panel_w),
            height: inner.height,
        };
        let panel_area = Rect {
            x: inner.x + inner.width.saturating_sub(panel_w),
            y: inner.y,
            width: panel_w,
            height: inner.height,
        };
        frame.render_widget(Paragraph::new(lines), board_area);
        frame.render_widget(Paragraph::new(panel), panel_area);

        if self.over {
            let msg = format!(
                " TIME · score {} · {} words · Enter: replay · q: menu ",
                self.score,
                self.found.len()
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

impl Boggle {
    /// Fill the grid from the weighted letter bag.
    fn deal(&mut self) {
        let rows = self.grid.len();
        for r in 0..rows {
            let cols = self.grid[r].len();
            for c in 0..cols {
                let idx = (self.next_rand() % BAG.len() as u64) as usize;
                self.grid[r][c] = BAG[idx] as char;
            }
        }
    }

    /// Validate and, if good, score the current input.
    fn submit(&mut self) {
        let word: String = self.input.drain(..).collect();
        if word.chars().count() < 3 {
            self.message = String::from("Too short (min 3).");
            return;
        }
        if self.found.iter().any(|w| w == &word) {
            self.message = String::from("Already found.");
            return;
        }
        if !self.traceable(&word) {
            self.message = String::from("No adjacent path.");
            return;
        }

        // Length-based scoring, with a bonus for dictionary hits.
        let len = word.chars().count() as u32;
        let mut gain = match len {
            3 | 4 => 1,
            5 => 2,
            6 => 3,
            7 => 5,
            _ => 11,
        };
        let dict_hit = DICT.iter().any(|&w| w == word);
        if dict_hit {
            gain += 1;
        }
        self.score += gain;
        self.message = if dict_hit {
            format!("+{gain}  (dictionary word!)")
        } else {
            format!("+{gain}")
        };
        self.found.push(word);
    }

    /// Depth-first search for an adjacency path spelling `word` without reuse.
    fn traceable(&self, word: &str) -> bool {
        let letters: Vec<char> = word.chars().collect();
        if letters.is_empty() {
            return false;
        }
        let mut used: HashSet<(usize, usize)> = HashSet::new();
        for r in 0..SIZE {
            for c in 0..SIZE {
                if self.grid[r][c] == letters[0] && self.walk(&letters, 1, r, c, &mut used) {
                    return true;
                }
            }
        }
        false
    }

    /// Recursive step of the adjacency search starting from cell (`r`, `c`),
    /// which already matches `letters[idx - 1]`.
    fn walk(
        &self,
        letters: &[char],
        idx: usize,
        r: usize,
        c: usize,
        used: &mut HashSet<(usize, usize)>,
    ) -> bool {
        if idx >= letters.len() {
            return true;
        }
        used.insert((r, c));
        let want = letters[idx];
        for dr in -1i32..=1 {
            for dc in -1i32..=1 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let nr = r as i32 + dr;
                let nc = c as i32 + dc;
                if nr < 0 || nc < 0 || nr >= SIZE as i32 || nc >= SIZE as i32 {
                    continue;
                }
                let (nr, nc) = (nr as usize, nc as usize);
                if self.grid[nr][nc] == want
                    && !used.contains(&(nr, nc))
                    && self.walk(letters, idx + 1, nr, nc, used)
                {
                    used.remove(&(r, c));
                    return true;
                }
            }
        }
        used.remove(&(r, c));
        false
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
    Boggle,
    id: "boggle",
    name: "Boggle",
    description: "Trace adjacent letters to spell words against the clock.",
    author: "furybee",
}
