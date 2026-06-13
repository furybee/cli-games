//! Simon — memorise and repeat a growing sequence of four coloured pads.
//!
//! Built on the same pattern as the `snake` reference: `dt`-driven timing,
//! a centred playfield, a controls hint, and a game-over overlay you can
//! restart with Enter. Each round the machine flashes one extra pad and the
//! playback gets a touch faster; reproduce the sequence with keys 1-4 (or the
//! arrow keys). One wrong key ends the run and shows the length you reached.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Playfield size (a 2x2 grid of fat pads with margins).
const WIDTH: u16 = 40;
const HEIGHT: u16 = 18;

/// Base on/off durations for a flashed pad; scaled down as rounds progress.
const FLASH_ON: Duration = Duration::from_millis(420);
const FLASH_OFF: Duration = Duration::from_millis(180);
/// How long the player's own press lights its pad.
const PRESS_FLASH: Duration = Duration::from_millis(160);
/// Short beat before the machine starts playing a new round.
const PREPLAY_PAUSE: Duration = Duration::from_millis(550);

/// The four pads. Index order matches keys 1-4 and the on-screen layout.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pad {
    Green,
    Red,
    Blue,
    Yellow,
}

impl Pad {
    fn all() -> [Pad; 4] {
        [Pad::Green, Pad::Red, Pad::Blue, Pad::Yellow]
    }

    fn dim(self) -> Color {
        match self {
            Pad::Green => Color::Rgb(0, 70, 0),
            Pad::Red => Color::Rgb(80, 0, 0),
            Pad::Blue => Color::Rgb(0, 0, 90),
            Pad::Yellow => Color::Rgb(80, 70, 0),
        }
    }

    fn lit(self) -> Color {
        match self {
            Pad::Green => Color::LightGreen,
            Pad::Red => Color::LightRed,
            Pad::Blue => Color::LightBlue,
            Pad::Yellow => Color::LightYellow,
        }
    }
}

/// What the game is currently doing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Brief pause, then playback begins.
    PrePlay,
    /// Machine is flashing the sequence; `step` is the pad being shown.
    Playing { step: usize },
    /// Waiting for the player to reproduce the sequence; `step` is next index.
    Input { step: usize },
    /// Player failed; show their reached length and offer a replay.
    GameOver,
}

pub struct Simon {
    sequence: Vec<Pad>,
    phase: Phase,
    /// Timer for the current phase sub-step.
    timer: Duration,
    /// `true` while a flashed pad is in its on segment.
    flash_on: bool,
    /// Pad lit by the player's own press, with its remaining glow time.
    press_glow: Option<(Pad, Duration)>,
    rng: u64,
}

impl Game for Simon {
    fn new() -> Self {
        let mut game = Simon {
            sequence: Vec::new(),
            phase: Phase::PrePlay,
            timer: Duration::ZERO,
            flash_on: true,
            press_glow: None,
            rng: seed(),
        };
        game.add_step();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        // Fade out the player's press highlight regardless of phase.
        if let Some((pad, remaining)) = self.press_glow {
            let left = remaining.saturating_sub(ctx.dt);
            self.press_glow = if left.is_zero() {
                None
            } else {
                Some((pad, left))
            };
        }

        match self.phase {
            Phase::GameOver => {
                if ctx.pressed(KeyCode::Enter) {
                    *self = Simon::new();
                }
            }
            Phase::PrePlay => {
                self.timer = self.timer.saturating_add(ctx.dt);
                if self.timer >= PREPLAY_PAUSE {
                    self.timer = Duration::ZERO;
                    self.flash_on = true;
                    self.phase = Phase::Playing { step: 0 };
                }
            }
            Phase::Playing { step } => self.advance_playback(ctx, step),
            Phase::Input { step } => self.handle_input(ctx, step),
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let score = self.sequence.len();
        let title = format!(" Simon  ·  round {} ", score);
        let field = centered(WIDTH + 2, HEIGHT + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        // Which pad (if any) is currently glowing?
        let active = self.active_pad();

        // Lay out a 2x2 grid of pads inside `inner`, drawn cell by cell.
        let cols = inner.width as usize;
        let rows = inner.height as usize;
        let pads = Pad::all();
        let mut lines = Vec::with_capacity(rows);
        for ry in 0..rows {
            let mut spans = Vec::with_capacity(1);
            let mut row = String::with_capacity(cols);
            // Determine pad row/col by splitting the area in half.
            let bottom = ry * 2 >= rows;
            let mid_gap = ry == rows / 2;
            if mid_gap {
                spans.push(Span::raw(" ".repeat(cols)));
                lines.push(Line::from(spans));
                continue;
            }
            for rx in 0..cols {
                let right = rx * 2 >= cols;
                let pad = match (bottom, right) {
                    (false, false) => pads[0],
                    (false, true) => pads[1],
                    (true, false) => pads[2],
                    (true, true) => pads[3],
                };
                // A gutter column down the middle.
                if rx == cols / 2 {
                    row.push(' ');
                } else {
                    let _ = pad;
                    row.push('█');
                }
            }
            // Build coloured spans for this row by scanning columns.
            spans.clear();
            let mut current: Option<(Color, String)> = None;
            for (rx, ch) in row.chars().enumerate() {
                let color = if ch == ' ' {
                    None
                } else {
                    let right = rx * 2 >= cols;
                    let pad = match (bottom, right) {
                        (false, false) => pads[0],
                        (false, true) => pads[1],
                        (true, false) => pads[2],
                        (true, true) => pads[3],
                    };
                    Some(if Some(pad) == active {
                        pad.lit()
                    } else {
                        pad.dim()
                    })
                };
                match (&mut current, color) {
                    (Some((c, s)), Some(col)) if *c == col => s.push(ch),
                    (cur, Some(col)) => {
                        if let Some((c, s)) = cur.take() {
                            spans.push(Span::styled(s, Style::default().fg(c)));
                        }
                        *cur = Some((col, ch.to_string()));
                    }
                    (cur, None) => {
                        if let Some((c, s)) = cur.take() {
                            spans.push(Span::styled(s, Style::default().fg(c)));
                        }
                        spans.push(Span::raw(" "));
                    }
                }
            }
            if let Some((c, s)) = current.take() {
                spans.push(Span::styled(s, Style::default().fg(c)));
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        // Status / controls hint under the field.
        let hint = match self.phase {
            Phase::PrePlay => " Watch the sequence... ".to_string(),
            Phase::Playing { .. } => " Watch the sequence... ".to_string(),
            Phase::Input { step } => {
                format!(
                    " Your turn: {}/{}  ·  keys 1-4 / arrows ",
                    step,
                    self.sequence.len()
                )
            }
            Phase::GameOver => " q: menu ".to_string(),
        };
        let hint_area = Rect {
            x: field.x,
            y: field.y.saturating_add(field.height),
            width: field.width,
            height: 1,
        };
        if hint_area.y < area.y.saturating_add(area.height) {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    hint,
                    Style::default().fg(Color::DarkGray),
                ))),
                hint_area,
            );
        }

        if let Phase::GameOver = self.phase {
            let reached = self.sequence.len().saturating_sub(1);
            let msg = format!(
                " GAME OVER · reached {} · Enter: replay · q: menu ",
                reached
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

impl Simon {
    /// Append one random pad to the sequence.
    fn add_step(&mut self) {
        let pads = Pad::all();
        let idx = (self.next_rand() % 4) as usize;
        self.sequence.push(pads[idx]);
    }

    /// Playback timing scales down (speeds up) as the sequence grows.
    fn flash_durations(&self) -> (Duration, Duration) {
        // Shave up to ~55% off the base timings as rounds climb.
        let steps = self.sequence.len().min(12) as u32;
        let num = 100u32.saturating_sub(steps.saturating_mul(4));
        let scale = |d: Duration| d.mul_f32(num as f32 / 100.0);
        (scale(FLASH_ON), scale(FLASH_OFF))
    }

    /// Drive the machine's flashing of the sequence.
    fn advance_playback(&mut self, ctx: &GameContext, step: usize) {
        let (on, off) = self.flash_durations();
        self.timer = self.timer.saturating_add(ctx.dt);
        if self.flash_on {
            if self.timer >= on {
                self.timer = Duration::ZERO;
                self.flash_on = false;
            }
        } else if self.timer >= off {
            self.timer = Duration::ZERO;
            self.flash_on = true;
            let next = step + 1;
            if next >= self.sequence.len() {
                self.phase = Phase::Input { step: 0 };
            } else {
                self.phase = Phase::Playing { step: next };
            }
        }
    }

    /// Read player input and validate it against the sequence.
    fn handle_input(&mut self, ctx: &GameContext, step: usize) {
        let pressed = if ctx.pressed(KeyCode::Char('1')) {
            Some(Pad::Green)
        } else if ctx.pressed(KeyCode::Char('2')) {
            Some(Pad::Red)
        } else if ctx.pressed(KeyCode::Char('3')) {
            Some(Pad::Blue)
        } else if ctx.pressed(KeyCode::Char('4')) {
            Some(Pad::Yellow)
        } else if ctx.pressed(KeyCode::Up) || ctx.pressed(KeyCode::Char('w')) {
            Some(Pad::Green)
        } else if ctx.pressed(KeyCode::Right) || ctx.pressed(KeyCode::Char('d')) {
            Some(Pad::Red)
        } else if ctx.pressed(KeyCode::Left) || ctx.pressed(KeyCode::Char('a')) {
            Some(Pad::Blue)
        } else if ctx.pressed(KeyCode::Down) || ctx.pressed(KeyCode::Char('s')) {
            Some(Pad::Yellow)
        } else {
            None
        };

        let Some(pad) = pressed else {
            return;
        };

        self.press_glow = Some((pad, PRESS_FLASH));

        let expected = self.sequence.get(step).copied();
        if expected != Some(pad) {
            self.phase = Phase::GameOver;
            return;
        }

        let next = step + 1;
        if next >= self.sequence.len() {
            // Round complete — grow and replay.
            self.add_step();
            self.timer = Duration::ZERO;
            self.phase = Phase::PrePlay;
        } else {
            self.phase = Phase::Input { step: next };
        }
    }

    /// The pad that should currently be drawn lit (playback or player press).
    fn active_pad(&self) -> Option<Pad> {
        match self.phase {
            Phase::Playing { step } if self.flash_on => self.sequence.get(step).copied(),
            _ => self.press_glow.map(|(pad, _)| pad),
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

fn seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x2545_F491_4F6C_DD1D)
        | 1
}

/// Centre a `w`x`h` rect inside `area`, clamped to its bounds.
fn centered(w: u16, h: u16, area: Rect) -> Rect {
    Rect {
        x: area.x + area.width.saturating_sub(w) / 2,
        y: area.y + area.height.saturating_sub(h) / 2,
        width: w.min(area.width),
        height: h.min(area.height),
    }
}

register_game! {
    Simon,
    id: "simon",
    name: "Simon",
    description: "Repeat the growing colour sequence.",
    author: "furybee",
}
