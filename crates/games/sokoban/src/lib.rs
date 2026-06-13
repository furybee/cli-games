//! Sokoban — push crates onto targets in a tiny warehouse.
//!
//! Built in the style of the `snake` reference: state, `ctx.pressed` input,
//! a centred grid render, an overlay on win, and self-registration. There is
//! no real-time motion here (moves are turn-based), so `tick_rate` just keeps
//! the input loop responsive.

use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Built-in levels as char maps.
///
/// `#` wall · `.` target · `$` crate · `@` player · `*` crate already on target
/// · `+` player standing on a target · space is open floor.
const LEVELS: &[&str] = &[
    // Level 1 — a gentle two-crate warm-up.
    "\
#######
#     #
# $.$ #
# .@. #
#  $  #
#  .  #
#######",
    // Level 2 — corridors and a corner push.
    "\
########
#  .   #
# $$ . #
# @    #
# $  . #
########",
    // Level 3 — the classic little squeeze.
    "\
########
#   . ##
# #$#  #
# .$ . #
##$@   #
# .  ###
########",
];

/// One parsed level: walls and targets are static, crates and the player move.
#[derive(Clone)]
struct Board {
    width: u16,
    height: u16,
    walls: HashSet<(u16, u16)>,
    targets: HashSet<(u16, u16)>,
    crates: HashSet<(u16, u16)>,
    player: (u16, u16),
}

impl Board {
    fn parse(map: &str) -> Board {
        let mut walls = HashSet::new();
        let mut targets = HashSet::new();
        let mut crates = HashSet::new();
        let mut player = (0u16, 0u16);
        let mut width = 0u16;
        let mut height = 0u16;

        for (y, row) in map.lines().enumerate() {
            let y = y as u16;
            height = y + 1;
            for (x, ch) in row.chars().enumerate() {
                let x = x as u16;
                if x + 1 > width {
                    width = x + 1;
                }
                let pos = (x, y);
                match ch {
                    '#' => {
                        walls.insert(pos);
                    }
                    '.' => {
                        targets.insert(pos);
                    }
                    '$' => {
                        crates.insert(pos);
                    }
                    '*' => {
                        crates.insert(pos);
                        targets.insert(pos);
                    }
                    '@' => {
                        player = pos;
                    }
                    '+' => {
                        player = pos;
                        targets.insert(pos);
                    }
                    _ => {}
                }
            }
        }

        Board {
            width,
            height,
            walls,
            targets,
            crates,
            player,
        }
    }

    /// All crates resting on a target.
    fn solved(&self) -> bool {
        self.crates.iter().all(|c| self.targets.contains(c))
    }

    fn crates_on_target(&self) -> usize {
        self.crates
            .iter()
            .filter(|c| self.targets.contains(c))
            .count()
    }

    /// Attempt to move the player by `(dx, dy)`. Returns `true` if anything
    /// changed (so the caller knows whether to record an undo step and bump the
    /// move counter). Cannot pull, cannot push two crates, cannot enter walls.
    fn try_move(&mut self, dx: i16, dy: i16) -> bool {
        let next = match step(self.player, dx, dy) {
            Some(p) => p,
            None => return false,
        };
        if self.walls.contains(&next) {
            return false;
        }
        if self.crates.contains(&next) {
            // Pushing: the cell beyond the crate must be free.
            let beyond = match step(next, dx, dy) {
                Some(p) => p,
                None => return false,
            };
            if self.walls.contains(&beyond) || self.crates.contains(&beyond) {
                return false;
            }
            self.crates.remove(&next);
            self.crates.insert(beyond);
        }
        self.player = next;
        true
    }
}

/// Move a position by a delta, guarding against unsigned underflow at the edge.
fn step(pos: (u16, u16), dx: i16, dy: i16) -> Option<(u16, u16)> {
    let x = pos.0 as i32 + dx as i32;
    let y = pos.1 as i32 + dy as i32;
    if x < 0 || y < 0 {
        return None;
    }
    Some((x as u16, y as u16))
}

pub struct Sokoban {
    level_index: usize,
    board: Board,
    /// Past board states for `u` to undo (capped to keep memory bounded).
    history: Vec<Board>,
    moves: u32,
    /// All built-in levels cleared.
    finished: bool,
    rng: u64,
}

impl Sokoban {
    fn load(level_index: usize, rng: u64) -> Sokoban {
        let map = LEVELS.get(level_index).copied().unwrap_or(LEVELS[0]);
        Sokoban {
            level_index,
            board: Board::parse(map),
            history: Vec::new(),
            moves: 0,
            finished: false,
            rng,
        }
    }

    fn reset_level(&mut self) {
        let rng = self.rng;
        *self = Sokoban::load(self.level_index, rng);
    }

    fn next_level(&mut self) {
        let next = self.level_index + 1;
        if next < LEVELS.len() {
            let rng = self.next_rand();
            *self = Sokoban::load(next, rng);
        } else {
            self.finished = true;
        }
    }

    fn undo(&mut self) {
        if let Some(prev) = self.history.pop() {
            self.board = prev;
            self.moves = self.moves.saturating_sub(1);
        }
    }

    fn do_move(&mut self, dx: i16, dy: i16) {
        let before = self.board.clone();
        if self.board.try_move(dx, dy) {
            // Cap history so a long session can't grow without bound.
            if self.history.len() >= 512 {
                self.history.remove(0);
            }
            self.history.push(before);
            self.moves += 1;
        }
    }

    /// xorshift64 — keeps the crate dependency-free (only used to vary seeds).
    fn next_rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }
}

impl Game for Sokoban {
    fn new() -> Self {
        Sokoban::load(0, seed())
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.finished {
            if ctx.pressed(KeyCode::Enter) {
                *self = Sokoban::new();
            }
            return Transition::Stay;
        }

        // Level solved: wait for Enter to advance to the next one.
        if self.board.solved() {
            if ctx.pressed(KeyCode::Enter) {
                self.next_level();
            } else if ctx.pressed(KeyCode::Char('r')) {
                self.reset_level();
            }
            return Transition::Stay;
        }

        if ctx.pressed(KeyCode::Char('r')) {
            self.reset_level();
            return Transition::Stay;
        }
        if ctx.pressed(KeyCode::Char('u')) {
            self.undo();
            return Transition::Stay;
        }

        // One step per directional press (turn-based movement).
        if ctx.pressed(KeyCode::Up) {
            self.do_move(0, -1);
        } else if ctx.pressed(KeyCode::Down) {
            self.do_move(0, 1);
        } else if ctx.pressed(KeyCode::Left) {
            self.do_move(-1, 0);
        } else if ctx.pressed(KeyCode::Right) {
            self.do_move(1, 0);
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let board = &self.board;
        let title = format!(
            " Sokoban  ·  level {}/{}  ·  moves {}  ·  {}/{} ",
            self.level_index + 1,
            LEVELS.len(),
            self.moves,
            board.crates_on_target(),
            board.crates.len(),
        );

        // Each tile is two columns wide so the grid looks square.
        let field = centered(board.width * 2 + 2, board.height + 4, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let mut lines = Vec::with_capacity(board.height as usize + 2);
        for y in 0..board.height {
            let mut spans = Vec::with_capacity(board.width as usize);
            for x in 0..board.width {
                let pos = (x, y);
                let is_target = board.targets.contains(&pos);
                let span = if board.walls.contains(&pos) {
                    Span::styled("██", Style::default().fg(Color::DarkGray))
                } else if board.player == pos {
                    let style = if is_target {
                        Style::default()
                            .fg(Color::LightCyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                            .fg(Color::LightGreen)
                            .add_modifier(Modifier::BOLD)
                    };
                    Span::styled("@ ", style)
                } else if board.crates.contains(&pos) {
                    if is_target {
                        Span::styled(
                            "[]",
                            Style::default()
                                .fg(Color::LightGreen)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Span::styled("[]", Style::default().fg(Color::Yellow))
                    }
                } else if is_target {
                    Span::styled(" .", Style::default().fg(Color::Red))
                } else {
                    Span::raw("  ")
                };
                spans.push(span);
            }
            lines.push(Line::from(spans));
        }

        // A compact controls hint beneath the grid.
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "arrows move · u undo · r reset · q menu",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(Paragraph::new(lines), inner);

        if self.finished {
            let msg = " ALL LEVELS CLEARED! · Enter: play again · q: menu ";
            let overlay = centered(msg.chars().count() as u16 + 2, 3, area);
            frame.render_widget(Clear, overlay);
            frame.render_widget(
                Paragraph::new(msg)
                    .block(Block::default().borders(Borders::ALL))
                    .style(
                        Style::default()
                            .fg(Color::LightGreen)
                            .add_modifier(Modifier::BOLD),
                    ),
                overlay,
            );
        } else if board.solved() {
            let msg = if self.level_index + 1 < LEVELS.len() {
                format!(
                    " SOLVED in {} moves · Enter: next level · r: replay ",
                    self.moves
                )
            } else {
                format!(" SOLVED in {} moves · Enter: finish ", self.moves)
            };
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
    Sokoban,
    id: "sokoban",
    name: "Sokoban",
    description: "Push every crate onto its target.",
    author: "furybee",
}
