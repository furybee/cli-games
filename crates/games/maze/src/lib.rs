//! Maze — navigate `@` from the start to the exit through a procedurally
//! generated perfect maze.
//!
//! Mirrors the Snake reference: `dt`-based timing for the clock, a centred grid
//! render, a win overlay, the xorshift `next_rand()` + `seed()` RNG, and
//! self-registration. The maze itself is carved with a recursive-backtracker
//! (iterative, stack-based) so it is always a "perfect" maze: exactly one path
//! between any two cells.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Maze dimensions in *cells* (each cell becomes a 2x2 region of glyphs once the
/// wall lattice is added). Kept modest so it fits a typical terminal.
const COLS: usize = 17;
const ROWS: usize = 11;

/// Rendered grid size: a wall lattice surrounds every cell, so a `COLS`×`ROWS`
/// maze occupies `(2*COLS + 1)`×`(2*ROWS + 1)` glyph cells.
const GW: usize = COLS * 2 + 1;
const GH: usize = ROWS * 2 + 1;

pub struct Maze {
    /// Wall grid. `true` == wall, `false` == open. Indexed `[y * GW + x]`.
    walls: Vec<bool>,
    /// Player position in grid coordinates (always on an odd-odd cell centre).
    player: (usize, usize),
    /// Exit position in grid coordinates.
    exit: (usize, usize),
    steps: u32,
    elapsed: Duration,
    won: bool,
    /// Time frozen at the moment of winning, so the clock stops on the overlay.
    win_time: Duration,
    rng: u64,
}

impl Game for Maze {
    fn new() -> Self {
        let mut game = Maze {
            walls: vec![true; GW * GH],
            player: (1, 1),
            exit: (GW - 2, GH - 2),
            steps: 0,
            elapsed: Duration::ZERO,
            won: false,
            win_time: Duration::ZERO,
            rng: seed(),
        };
        game.generate();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.won {
            if ctx.pressed(KeyCode::Enter) {
                *self = Maze::new();
            }
            return Transition::Stay;
        }

        self.elapsed += ctx.dt;

        // One tile per key press; check open-ness before committing.
        for &(key, dx, dy) in &[
            (KeyCode::Up, 0isize, -1isize),
            (KeyCode::Down, 0, 1),
            (KeyCode::Left, -1, 0),
            (KeyCode::Right, 1, 0),
        ] {
            if ctx.pressed(key) {
                self.try_move(dx, dy);
            }
        }

        if self.player == self.exit {
            self.won = true;
            self.win_time = self.elapsed;
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let secs = if self.won {
            self.win_time
        } else {
            self.elapsed
        }
        .as_secs();
        let title = format!(" Maze  ·  steps {}  ·  {}s ", self.steps, secs);
        let field = centered(GW as u16 + 2, GH as u16 + 2, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let mut lines = Vec::with_capacity(GH);
        for y in 0..GH {
            let mut spans = Vec::with_capacity(GW);
            for x in 0..GW {
                let span = if (x, y) == self.player {
                    Span::styled(
                        "@",
                        Style::default()
                            .fg(Color::LightYellow)
                            .add_modifier(Modifier::BOLD),
                    )
                } else if (x, y) == self.exit {
                    Span::styled(
                        "⚑",
                        Style::default()
                            .fg(Color::LightGreen)
                            .add_modifier(Modifier::BOLD),
                    )
                } else if self.is_wall(x, y) {
                    Span::styled("█", Style::default().fg(Color::Blue))
                } else {
                    Span::raw(" ")
                };
                spans.push(span);
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        let hint = " Arrows: move · Enter: new maze · q: menu ";
        let hint_rect = Rect {
            x: field.x,
            y: field.y.saturating_add(field.height),
            width: hint.len() as u16,
            height: 1,
        };
        if hint_rect.y < area.y.saturating_add(area.height) {
            frame.render_widget(
                Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
                hint_rect,
            );
        }

        if self.won {
            let msg = format!(
                " YOU ESCAPED! · {} steps · {}s · Enter: replay · q: menu ",
                self.steps,
                self.win_time.as_secs()
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

impl Maze {
    fn idx(x: usize, y: usize) -> usize {
        y * GW + x
    }

    fn is_wall(&self, x: usize, y: usize) -> bool {
        self.walls.get(Self::idx(x, y)).copied().unwrap_or(true)
    }

    /// Attempt to step the player one tile in `(dx, dy)`; only succeeds onto an
    /// open, in-bounds tile.
    fn try_move(&mut self, dx: isize, dy: isize) {
        let (px, py) = self.player;
        let nx = px as isize + dx;
        let ny = py as isize + dy;
        if nx < 0 || ny < 0 || nx >= GW as isize || ny >= GH as isize {
            return;
        }
        let (nx, ny) = (nx as usize, ny as usize);
        if !self.is_wall(nx, ny) {
            self.player = (nx, ny);
            self.steps += 1;
        }
    }

    /// Carve a perfect maze with an iterative recursive-backtracker.
    ///
    /// Cells live at odd grid coordinates; walls between neighbouring cells are
    /// knocked out as we visit them. The start `(1, 1)` and exit (bottom-right
    /// cell) are guaranteed connected because the algorithm spans every cell.
    fn generate(&mut self) {
        for w in self.walls.iter_mut() {
            *w = true;
        }

        let mut visited = [false; COLS * ROWS];
        let cell_idx = |cx: usize, cy: usize| cy * COLS + cx;

        let mut stack: Vec<(usize, usize)> = Vec::with_capacity(COLS * ROWS);
        let start = (0usize, 0usize);
        stack.push(start);
        visited[cell_idx(start.0, start.1)] = true;
        // Open the starting cell centre.
        self.walls[Self::idx(1, 1)] = false;

        // Directions between cells: (dcx, dcy).
        let dirs = [(0isize, -1isize), (1, 0), (0, 1), (-1, 0)];

        while let Some(&(cx, cy)) = stack.last() {
            // Collect unvisited neighbours.
            let mut candidates: [usize; 4] = [0; 4];
            let mut count = 0usize;
            for (i, &(dcx, dcy)) in dirs.iter().enumerate() {
                let ncx = cx as isize + dcx;
                let ncy = cy as isize + dcy;
                if ncx < 0 || ncy < 0 || ncx >= COLS as isize || ncy >= ROWS as isize {
                    continue;
                }
                let (ncx, ncy) = (ncx as usize, ncy as usize);
                if !visited[cell_idx(ncx, ncy)] {
                    candidates[count] = i;
                    count += 1;
                }
            }

            if count == 0 {
                stack.pop();
                continue;
            }

            let pick = candidates[(self.next_rand() % count as u64) as usize];
            let (dcx, dcy) = dirs[pick];
            let ncx = (cx as isize + dcx) as usize;
            let ncy = (cy as isize + dcy) as usize;

            // Grid coordinates of both cell centres and the wall between them.
            let gx = cx * 2 + 1;
            let gy = cy * 2 + 1;
            let ngx = ncx * 2 + 1;
            let ngy = ncy * 2 + 1;
            let wx = (gx + ngx) / 2;
            let wy = (gy + ngy) / 2;

            self.walls[Self::idx(ngx, ngy)] = false;
            self.walls[Self::idx(wx, wy)] = false;

            visited[cell_idx(ncx, ncy)] = true;
            stack.push((ncx, ncy));
        }

        self.player = (1, 1);
        self.exit = (GW - 2, GH - 2);
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
    Maze,
    id: "maze",
    name: "Maze",
    description: "Find the exit of a freshly generated perfect maze.",
    author: "furybee",
}
