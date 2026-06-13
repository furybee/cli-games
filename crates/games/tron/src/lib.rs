//! Tron — real-time light-cycles. You vs a simple AI.
//!
//! Both cycles move continuously on a grid, leaving solid trails behind them.
//! Arrow keys steer (no 180° reversals). Running into any trail or a wall is
//! fatal. Last cycle standing wins the round; Enter restarts.
//!
//! It mirrors the `snake` reference: `dt`-accumulated stepping, a centred
//! playfield, the xorshift `next_rand()` + `seed()` pattern, a controls hint,
//! and a `Clear`-backed bordered game-over overlay.

use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

const WIDTH: u16 = 32;
const HEIGHT: u16 = 20;
/// Each cycle advances one cell every `STEP`.
const STEP: Duration = Duration::from_millis(85);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    fn opposite(self) -> Dir {
        match self {
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
            Dir::Left => Dir::Right,
            Dir::Right => Dir::Left,
        }
    }

    /// Step from `(x, y)` in this direction. `None` means a wall lies ahead.
    fn step(self, (x, y): (u16, u16)) -> Option<(u16, u16)> {
        match self {
            Dir::Up if y > 0 => Some((x, y - 1)),
            Dir::Down if y + 1 < HEIGHT => Some((x, y + 1)),
            Dir::Left if x > 0 => Some((x - 1, y)),
            Dir::Right if x + 1 < WIDTH => Some((x + 1, y)),
            _ => None,
        }
    }
}

/// Who, if anyone, has lost — decides the round outcome.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Playing,
    Win,
    Lose,
    Draw,
}

struct Cycle {
    pos: (u16, u16),
    dir: Dir,
    /// Buffered direction so a key press never causes an instant reversal.
    next_dir: Dir,
    alive: bool,
}

impl Cycle {
    fn new(pos: (u16, u16), dir: Dir) -> Self {
        Cycle {
            pos,
            dir,
            next_dir: dir,
            alive: true,
        }
    }
}

pub struct Tron {
    player: Cycle,
    ai: Cycle,
    /// Every cell currently filled by a trail (both cycles' bodies).
    trails: HashSet<(u16, u16)>,
    accumulator: Duration,
    outcome: Outcome,
    wins: u32,
    losses: u32,
    rng: u64,
}

impl Game for Tron {
    fn new() -> Self {
        let player_start = (WIDTH / 4, HEIGHT / 2);
        let ai_start = (WIDTH - WIDTH / 4 - 1, HEIGHT / 2);

        let mut trails = HashSet::new();
        trails.insert(player_start);
        trails.insert(ai_start);

        Tron {
            player: Cycle::new(player_start, Dir::Right),
            ai: Cycle::new(ai_start, Dir::Left),
            trails,
            accumulator: Duration::ZERO,
            outcome: Outcome::Playing,
            wins: 0,
            losses: 0,
            rng: seed(),
        }
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.outcome != Outcome::Playing {
            if ctx.pressed(KeyCode::Enter) {
                self.restart();
            }
            return Transition::Stay;
        }

        // Latest steering key wins, but never a 180° turn.
        for &(code, dir) in &[
            (KeyCode::Up, Dir::Up),
            (KeyCode::Down, Dir::Down),
            (KeyCode::Left, Dir::Left),
            (KeyCode::Right, Dir::Right),
        ] {
            if ctx.pressed(code) && dir != self.player.dir.opposite() {
                self.player.next_dir = dir;
            }
        }

        self.accumulator += ctx.dt;
        while self.accumulator >= STEP {
            self.accumulator -= STEP;
            self.step();
            if self.outcome != Outcome::Playing {
                break;
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(" Tron  ·  wins {}  losses {} ", self.wins, self.losses);
        let field = centered(WIDTH * 2 + 2, HEIGHT + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let player_head = self.player.pos;
        let ai_head = self.ai.pos;

        let mut lines = Vec::with_capacity(HEIGHT as usize);
        for y in 0..HEIGHT {
            let mut spans = Vec::with_capacity(WIDTH as usize);
            for x in 0..WIDTH {
                let cell = (x, y);
                let span = if cell == player_head {
                    Span::styled("██", Style::default().fg(Color::LightCyan))
                } else if cell == ai_head {
                    Span::styled("██", Style::default().fg(Color::LightRed))
                } else if self.trails.contains(&cell) {
                    // Colour the trail by whichever cycle's region it is closer
                    // to is overkill; a single neutral trail colour per side is
                    // distinguished by the heads above — keep trails uniform.
                    Span::styled("██", Style::default().fg(Color::DarkGray))
                } else {
                    Span::raw("  ")
                };
                spans.push(span);
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        // Controls hint just below the field.
        let hint = " Arrows: steer · Enter: restart · q: menu ";
        let hint_area = Rect {
            x: field.x,
            y: field.y + field.height,
            width: hint.len() as u16,
            height: 1,
        };
        if hint_area.y < area.y + area.height {
            frame.render_widget(
                Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
                hint_area,
            );
        }

        if self.outcome != Outcome::Playing {
            let msg = match self.outcome {
                Outcome::Win => " YOU WIN · Enter: replay · q: menu ",
                Outcome::Lose => " YOU CRASHED · Enter: replay · q: menu ",
                Outcome::Draw => " DRAW · Enter: replay · q: menu ",
                Outcome::Playing => "",
            };
            let colour = match self.outcome {
                Outcome::Win => Color::LightCyan,
                Outcome::Lose => Color::LightRed,
                _ => Color::Yellow,
            };
            let overlay = centered(msg.chars().count() as u16 + 2, 3, area);
            frame.render_widget(Clear, overlay);
            frame.render_widget(
                Paragraph::new(msg)
                    .block(Block::default().borders(Borders::ALL))
                    .style(Style::default().fg(colour).add_modifier(Modifier::BOLD)),
                overlay,
            );
        }
    }

    fn tick_rate(&self) -> Duration {
        Duration::from_millis(20)
    }
}

impl Tron {
    /// Advance both cycles one cell simultaneously and resolve collisions.
    fn step(&mut self) {
        // Decide the AI's move before anyone commits, based on the current grid.
        self.ai.next_dir = self.choose_ai_dir();

        self.player.dir = self.player.next_dir;
        self.ai.dir = self.ai.next_dir;

        let player_next = self.player.dir.step(self.player.pos);
        let ai_next = self.ai.dir.step(self.ai.pos);

        // A move is fatal if it leaves the grid or lands on an existing trail.
        let player_crash = match player_next {
            None => true,
            Some(p) => self.trails.contains(&p),
        };
        let ai_crash = match ai_next {
            None => true,
            Some(p) => self.trails.contains(&p),
        };

        // Head-on collision: both target the same free cell.
        let head_on = match (player_next, ai_next) {
            (Some(p), Some(a)) => p == a,
            _ => false,
        };

        if head_on {
            self.player.alive = false;
            self.ai.alive = false;
        } else {
            if player_crash {
                self.player.alive = false;
            }
            if ai_crash {
                self.ai.alive = false;
            }
        }

        // Commit surviving moves: lay trail and advance the head.
        if self.player.alive
            && let Some(p) = player_next
        {
            self.trails.insert(p);
            self.player.pos = p;
        }
        if self.ai.alive
            && let Some(a) = ai_next
        {
            self.trails.insert(a);
            self.ai.pos = a;
        }

        self.outcome = match (self.player.alive, self.ai.alive) {
            (true, true) => Outcome::Playing,
            (true, false) => Outcome::Win,
            (false, true) => Outcome::Lose,
            (false, false) => Outcome::Draw,
        };

        match self.outcome {
            Outcome::Win => self.wins += 1,
            Outcome::Lose | Outcome::Draw => self.losses += 1,
            Outcome::Playing => {}
        }
    }

    /// Greedy AI: keep going straight if safe, otherwise pick a safe turn.
    /// "Safe" means in-bounds and onto an empty cell. Ties are broken with the
    /// xorshift RNG so the AI is not perfectly predictable. Never reverses.
    fn choose_ai_dir(&mut self) -> Dir {
        let here = self.ai.pos;
        let current = self.ai.dir;

        // Draw the "roam" roll up front, before the closure below borrows
        // self.trails (next_rand needs &mut self, which would clash).
        let roam = !self.next_rand().is_multiple_of(8);

        let is_safe = |dir: Dir| -> bool {
            match dir.step(here) {
                Some(p) => !self.trails.contains(&p),
                None => false,
            }
        };

        // Prefer continuing straight to leave longer, less self-trapping walls,
        // but occasionally take a turn even when straight is fine, to roam.
        if is_safe(current) && roam {
            return current;
        }

        // Gather all safe, non-reversing alternatives.
        let mut options: Vec<Dir> = [Dir::Up, Dir::Down, Dir::Left, Dir::Right]
            .into_iter()
            .filter(|&d| d != current.opposite() && is_safe(d))
            .collect();

        if options.is_empty() {
            // Doomed: keep the current heading and let the crash resolve.
            return current;
        }

        // Bias toward the option with the most open space immediately around it,
        // a cheap heuristic that helps the AI avoid dead ends.
        options.sort_by_key(|&d| std::cmp::Reverse(self.open_neighbours(d)));

        // Among the best-scoring options, pick pseudo-randomly for variety.
        let best = self.open_neighbours(options[0]);
        let top: Vec<Dir> = options
            .iter()
            .copied()
            .filter(|&d| self.open_neighbours(d) == best)
            .collect();
        let idx = (self.next_rand() as usize) % top.len();
        top[idx]
    }

    /// Count empty, in-bounds cells around where moving `dir` would land.
    fn open_neighbours(&self, dir: Dir) -> usize {
        let landing = match dir.step(self.ai.pos) {
            Some(p) => p,
            None => return 0,
        };
        [Dir::Up, Dir::Down, Dir::Left, Dir::Right]
            .into_iter()
            .filter(|d| match d.step(landing) {
                Some(p) => !self.trails.contains(&p),
                None => false,
            })
            .count()
    }

    fn restart(&mut self) {
        let rng = self.next_rand();
        let wins = self.wins;
        let losses = self.losses;
        *self = Tron::new();
        self.rng = rng | 1;
        self.wins = wins;
        self.losses = losses;
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
    Tron,
    id: "tron",
    name: "Tron",
    description: "Light-cycle duel — out-survive the AI.",
    author: "furybee",
}
