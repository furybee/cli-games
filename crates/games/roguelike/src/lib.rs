//! Mini Roguelike — a single procedurally-generated dungeon floor.
//!
//! Mirrors the structure of the `snake` reference crate: a `centered` helper,
//! an xorshift PRNG seeded from the clock, and a `Clear`+bordered overlay for
//! the win / death screens.
//!
//! It is fully turn-based: nothing moves until you press a movement key, so the
//! `dt` accumulator only drives a small cosmetic message timer. Bump into a
//! monster to attack it; step over a potion (`!`) to heal; reach the stairs
//! (`>`) to clear the floor.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

const WIDTH: usize = 48;
const HEIGHT: usize = 22;
const MAX_ROOMS: usize = 8;
const ROOM_MIN: usize = 4;
const ROOM_MAX: usize = 9;

const PLAYER_MAX_HP: i32 = 20;
const PLAYER_ATK: i32 = 5;
const POTION_HEAL: i32 = 8;

/// A map tile. Floors and corridors are both walkable; walls block everything.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tile {
    Wall,
    Floor,
}

/// The kinds of monster that roam the floor. They differ in toughness, bite,
/// and how far away they notice (and start chasing) the player.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Rat,
    Goblin,
}

impl Kind {
    fn glyph(self) -> &'static str {
        match self {
            Kind::Rat => "r",
            Kind::Goblin => "g",
        }
    }

    fn color(self) -> Color {
        match self {
            Kind::Rat => Color::Gray,
            Kind::Goblin => Color::LightGreen,
        }
    }

    fn max_hp(self) -> i32 {
        match self {
            Kind::Rat => 4,
            Kind::Goblin => 9,
        }
    }

    fn attack(self) -> i32 {
        match self {
            Kind::Rat => 2,
            Kind::Goblin => 4,
        }
    }

    /// Chebyshev distance within which the monster wakes and chases.
    fn sight(self) -> i32 {
        match self {
            Kind::Rat => 5,
            Kind::Goblin => 8,
        }
    }
}

struct Monster {
    kind: Kind,
    pos: (usize, usize),
    hp: i32,
}

/// The result of the player's most recent action — drives the status banner.
enum Phase {
    Playing,
    Dead,
    Won,
}

pub struct Roguelike {
    tiles: Vec<Tile>,
    player: (usize, usize),
    hp: i32,
    stairs: (usize, usize),
    potions: Vec<(usize, usize)>,
    monsters: Vec<Monster>,
    phase: Phase,
    /// Player-facing log of the last thing that happened.
    message: String,
    /// Time the current message has been shown (purely cosmetic).
    msg_age: Duration,
    turns: u32,
    rng: u64,
}

impl Game for Roguelike {
    fn new() -> Self {
        let mut game = Roguelike {
            tiles: vec![Tile::Wall; WIDTH * HEIGHT],
            player: (1, 1),
            hp: PLAYER_MAX_HP,
            stairs: (1, 1),
            potions: Vec::new(),
            monsters: Vec::new(),
            phase: Phase::Playing,
            message: String::from("Find the stairs >"),
            msg_age: Duration::ZERO,
            turns: 0,
            rng: seed(),
        };
        game.generate();
        game
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        self.msg_age += ctx.dt;

        match self.phase {
            Phase::Dead | Phase::Won => {
                if ctx.pressed(KeyCode::Enter) {
                    *self = Roguelike::new();
                }
                return Transition::Stay;
            }
            Phase::Playing => {}
        }

        // One key press == one turn. The first directional key wins so that a
        // burst of input never spends several turns at once.
        let step = if ctx.pressed(KeyCode::Up) {
            Some((0i32, -1i32))
        } else if ctx.pressed(KeyCode::Down) {
            Some((0, 1))
        } else if ctx.pressed(KeyCode::Left) {
            Some((-1, 0))
        } else if ctx.pressed(KeyCode::Right) {
            Some((1, 0))
        } else {
            None
        };

        if let Some((dx, dy)) = step {
            self.player_turn(dx, dy);
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(
            " Mini Roguelike  ·  HP {}/{}  ·  turns {} ",
            self.hp.max(0),
            PLAYER_MAX_HP,
            self.turns
        );
        let field = centered(WIDTH as u16 + 2, HEIGHT as u16 + 3, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let mut lines = Vec::with_capacity(HEIGHT + 1);
        for y in 0..HEIGHT {
            let mut spans = Vec::with_capacity(WIDTH);
            for x in 0..WIDTH {
                spans.push(self.cell_span(x, y));
            }
            lines.push(Line::from(spans));
        }
        lines.push(Line::from(Span::styled(
            format!("  {}", self.message),
            Style::default().fg(Color::Cyan),
        )));
        frame.render_widget(Paragraph::new(lines), inner);

        // Controls hint along the bottom edge of the bordered field.
        let hint = " arrows: move/attack  ·  !: potion  ·  >: stairs  ·  q: menu ";
        let hint_area = Rect {
            x: field.x + 1,
            y: field.y + field.height.saturating_sub(1),
            width: (hint.len() as u16).min(field.width.saturating_sub(2)),
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
            hint_area,
        );

        if let Phase::Dead | Phase::Won = self.phase {
            let msg = match self.phase {
                Phase::Won => format!(
                    " FLOOR CLEARED · {} turns · Enter: new floor · q: menu ",
                    self.turns
                ),
                _ => format!(
                    " YOU DIED · {} turns · Enter: try again · q: menu ",
                    self.turns
                ),
            };
            let color = if matches!(self.phase, Phase::Won) {
                Color::LightGreen
            } else {
                Color::Red
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

    fn tick_rate(&self) -> Duration {
        Duration::from_millis(30)
    }
}

impl Roguelike {
    /// The styled glyph for a single map cell, honouring draw priority:
    /// player > monster > stairs > potion > floor/wall.
    fn cell_span(&self, x: usize, y: usize) -> Span<'static> {
        if (x, y) == self.player {
            return Span::styled(
                "@",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
        }
        if let Some(m) = self.monsters.iter().find(|m| m.pos == (x, y)) {
            return Span::styled(m.kind.glyph(), Style::default().fg(m.kind.color()));
        }
        if (x, y) == self.stairs {
            return Span::styled(
                ">",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
        }
        if self.potions.contains(&(x, y)) {
            return Span::styled("!", Style::default().fg(Color::Magenta));
        }
        match self.tile(x, y) {
            Tile::Floor => Span::styled(".", Style::default().fg(Color::DarkGray)),
            Tile::Wall => Span::styled("#", Style::default().fg(Color::Blue)),
        }
    }

    /// Resolve one player action then let every awake monster act.
    fn player_turn(&mut self, dx: i32, dy: i32) {
        let (px, py) = self.player;
        let nx = px as i32 + dx;
        let ny = py as i32 + dy;
        if !in_bounds(nx, ny) {
            return;
        }
        let target = (nx as usize, ny as usize);

        // Bump-to-attack: if a monster occupies the target, strike instead of move.
        if let Some(idx) = self.monsters.iter().position(|m| m.pos == target) {
            self.monsters[idx].hp -= PLAYER_ATK;
            if self.monsters[idx].hp <= 0 {
                let kind = self.monsters[idx].kind;
                self.monsters.remove(idx);
                self.message = format!("You slay the {}.", kind_name(kind));
            } else {
                self.message = format!("You hit the {}.", kind_name(self.monsters[idx].kind));
            }
            self.msg_age = Duration::ZERO;
            self.turns += 1;
            self.monsters_turn();
            return;
        }

        if self.tile(target.0, target.1) == Tile::Wall {
            return; // walls are not a turn
        }

        self.player = target;
        self.turns += 1;

        // Pick up a potion under the new tile.
        if let Some(pi) = self.potions.iter().position(|&p| p == self.player) {
            self.potions.remove(pi);
            let before = self.hp;
            self.hp = (self.hp + POTION_HEAL).min(PLAYER_MAX_HP);
            self.message = format!("You quaff a potion (+{} HP).", self.hp - before);
            self.msg_age = Duration::ZERO;
        }

        // Reached the stairs?
        if self.player == self.stairs {
            self.phase = Phase::Won;
            self.message = String::from("You descend the stairs!");
            self.msg_age = Duration::ZERO;
            return;
        }

        self.monsters_turn();
    }

    /// Every monster within sight steps toward the player (or bites if adjacent).
    fn monsters_turn(&mut self) {
        let (px, py) = (self.player.0 as i32, self.player.1 as i32);
        let count = self.monsters.len();
        for i in 0..count {
            let (mx, my) = (self.monsters[i].pos.0 as i32, self.monsters[i].pos.1 as i32);
            let dx = px - mx;
            let dy = py - my;
            let dist = dx.abs().max(dy.abs());
            if dist > self.monsters[i].kind.sight() {
                continue; // hasn't noticed the player
            }

            if dist == 1 {
                // Adjacent — attack.
                self.hp -= self.monsters[i].kind.attack();
                self.message = format!("The {} bites you!", kind_name(self.monsters[i].kind));
                self.msg_age = Duration::ZERO;
                if self.hp <= 0 {
                    self.hp = 0;
                    self.phase = Phase::Dead;
                    return;
                }
                continue;
            }

            // Greedy chase: prefer the axis with the larger gap, fall back to the
            // other, and never walk into walls, the player, or another monster.
            let step_x = dx.signum();
            let step_y = dy.signum();
            let mut moves: Vec<(i32, i32)> = Vec::new();
            if dx.abs() >= dy.abs() {
                if step_x != 0 {
                    moves.push((step_x, 0));
                }
                if step_y != 0 {
                    moves.push((0, step_y));
                }
            } else {
                if step_y != 0 {
                    moves.push((0, step_y));
                }
                if step_x != 0 {
                    moves.push((step_x, 0));
                }
            }

            for (sx, sy) in moves {
                let tx = mx + sx;
                let ty = my + sy;
                if !in_bounds(tx, ty) {
                    continue;
                }
                let cell = (tx as usize, ty as usize);
                if self.tile(cell.0, cell.1) == Tile::Wall {
                    continue;
                }
                if cell == self.player {
                    continue;
                }
                if self
                    .monsters
                    .iter()
                    .enumerate()
                    .any(|(j, m)| j != i && m.pos == cell)
                {
                    continue;
                }
                self.monsters[i].pos = cell;
                break;
            }
        }
    }

    // --- Map generation -----------------------------------------------------

    /// Carve a fresh dungeon: rooms joined by L-shaped corridors, then scatter
    /// the player, stairs, potions, and monsters across distinct rooms.
    fn generate(&mut self) {
        self.tiles = vec![Tile::Wall; WIDTH * HEIGHT];
        let mut rooms: Vec<(usize, usize, usize, usize)> = Vec::new();

        for _ in 0..MAX_ROOMS {
            let w = ROOM_MIN + (self.next_rand() as usize % (ROOM_MAX - ROOM_MIN + 1));
            let h = ROOM_MIN + (self.next_rand() as usize % (ROOM_MAX - ROOM_MIN + 1));
            // Keep a 1-tile wall border around the whole map.
            let x = 1 + (self.next_rand() as usize % (WIDTH - w - 2).max(1));
            let y = 1 + (self.next_rand() as usize % (HEIGHT - h - 2).max(1));

            let new_room = (x, y, w, h);
            if rooms.iter().any(|r| rooms_overlap(*r, new_room)) {
                continue;
            }

            self.carve_room(new_room);
            if let Some(&prev) = rooms.last() {
                let (px, py) = room_center(prev);
                let (cx, cy) = room_center(new_room);
                // Randomise corridor elbow direction for variety.
                if self.next_rand() & 1 == 0 {
                    self.carve_h(px, cx, py);
                    self.carve_v(py, cy, cx);
                } else {
                    self.carve_v(py, cy, px);
                    self.carve_h(px, cx, cy);
                }
            }
            rooms.push(new_room);
        }

        // Guarantee at least one room so the player always has somewhere to stand.
        if rooms.is_empty() {
            let fallback = (1, 1, ROOM_MAX, ROOM_MIN);
            self.carve_room(fallback);
            rooms.push(fallback);
        }

        // Player starts in the first room; stairs go in the last (farthest) one.
        self.player = room_center(rooms[0]);
        self.stairs = room_center(rooms[rooms.len() - 1]);
        if self.stairs == self.player && rooms.len() > 1 {
            self.stairs = room_center(rooms[1]);
        }

        // Potions and monsters live in the middle rooms (skip the start room).
        self.potions.clear();
        self.monsters.clear();
        for &room in rooms.iter().skip(1) {
            // A potion in roughly half the rooms.
            if self.next_rand() & 1 == 0
                && let Some(spot) = self.free_spot(room)
            {
                self.potions.push(spot);
            }
            // One or two monsters per room.
            let n = 1 + (self.next_rand() as usize % 2);
            for _ in 0..n {
                if let Some(spot) = self.free_spot(room) {
                    let kind = if self.next_rand().is_multiple_of(3) {
                        Kind::Goblin
                    } else {
                        Kind::Rat
                    };
                    self.monsters.push(Monster {
                        kind,
                        pos: spot,
                        hp: kind.max_hp(),
                    });
                }
            }
        }
    }

    /// Find a walkable tile in `room` not already taken by an entity.
    fn free_spot(&self, room: (usize, usize, usize, usize)) -> Option<(usize, usize)> {
        let (rx, ry, rw, rh) = room;
        for yy in ry..ry + rh {
            for xx in rx..rx + rw {
                let cell = (xx, yy);
                if self.tile(xx, yy) != Tile::Floor {
                    continue;
                }
                if cell == self.player || cell == self.stairs {
                    continue;
                }
                if self.potions.contains(&cell) {
                    continue;
                }
                if self.monsters.iter().any(|m| m.pos == cell) {
                    continue;
                }
                return Some(cell);
            }
        }
        None
    }

    fn carve_room(&mut self, room: (usize, usize, usize, usize)) {
        let (rx, ry, rw, rh) = room;
        for yy in ry..ry + rh {
            for xx in rx..rx + rw {
                self.set_floor(xx, yy);
            }
        }
    }

    fn carve_h(&mut self, x0: usize, x1: usize, y: usize) {
        let (a, b) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
        for xx in a..=b {
            self.set_floor(xx, y);
        }
    }

    fn carve_v(&mut self, y0: usize, y1: usize, x: usize) {
        let (a, b) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
        for yy in a..=b {
            self.set_floor(x, yy);
        }
    }

    fn set_floor(&mut self, x: usize, y: usize) {
        if x < WIDTH && y < HEIGHT {
            self.tiles[y * WIDTH + x] = Tile::Floor;
        }
    }

    fn tile(&self, x: usize, y: usize) -> Tile {
        if x < WIDTH && y < HEIGHT {
            self.tiles[y * WIDTH + x]
        } else {
            Tile::Wall
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

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Rat => "rat",
        Kind::Goblin => "goblin",
    }
}

fn in_bounds(x: i32, y: i32) -> bool {
    x >= 0 && y >= 0 && (x as usize) < WIDTH && (y as usize) < HEIGHT
}

fn room_center(room: (usize, usize, usize, usize)) -> (usize, usize) {
    (room.0 + room.2 / 2, room.1 + room.3 / 2)
}

/// Rooms overlap if they touch — a one-tile margin keeps walls between them.
fn rooms_overlap(a: (usize, usize, usize, usize), b: (usize, usize, usize, usize)) -> bool {
    let (ax, ay, aw, ah) = a;
    let (bx, by, bw, bh) = b;
    ax <= bx + bw && ax + aw >= bx && ay <= by + bh && ay + ah >= by
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
    Roguelike,
    id: "roguelike",
    name: "Mini Roguelike",
    description: "Explore a dungeon, fight monsters, reach the stairs.",
    author: "furybee",
}
