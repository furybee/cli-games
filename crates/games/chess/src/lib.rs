//! Chess — you (White) versus a simple greedy/minimax AI (Black).
//!
//! Mirrors the Snake reference: explicit state, `dt`-free turn-based input,
//! a centred bordered board, a controls hint, and a game-over overlay with
//! Enter to restart. Self-registers via `register_game!`.
//!
//! Implements full legal move generation: pieces only make moves that do not
//! leave their own king in check, including castling and en passant. The AI
//! plays Black with a depth-2 minimax (alpha-beta) on material + position.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Piece colour.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    White,
    Black,
}

impl Side {
    fn opposite(self) -> Side {
        match self {
            Side::White => Side::Black,
            Side::Black => Side::White,
        }
    }
}

/// Piece kind.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl Kind {
    /// Centipawn-ish base value used by the AI and the score readout.
    fn value(self) -> i32 {
        match self {
            Kind::Pawn => 100,
            Kind::Knight => 320,
            Kind::Bishop => 330,
            Kind::Rook => 500,
            Kind::Queen => 900,
            Kind::King => 20000,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Piece {
    side: Side,
    kind: Kind,
}

impl Piece {
    /// A single ASCII letter for the piece: uppercase = White, lowercase = Black.
    /// ASCII keeps every board cell exactly one column wide, which Unicode chess
    /// glyphs do not (they're ambiguous/double-width and break alignment).
    fn letter(self) -> char {
        let c = match self.kind {
            Kind::Pawn => 'p',
            Kind::Knight => 'n',
            Kind::Bishop => 'b',
            Kind::Rook => 'r',
            Kind::Queen => 'q',
            Kind::King => 'k',
        };
        if self.side == Side::White {
            c.to_ascii_uppercase()
        } else {
            c
        }
    }
}

/// A square on the board, 0..64. Files 0..8 = a..h, ranks 0..8 = 8..1
/// (index 0 is a8, top-left, the way the board is drawn).
type Sq = usize;

fn file_of(sq: Sq) -> i32 {
    (sq % 8) as i32
}
fn rank_of(sq: Sq) -> i32 {
    (sq / 8) as i32
}
fn sq_at(file: i32, rank: i32) -> Option<Sq> {
    if (0..8).contains(&file) && (0..8).contains(&rank) {
        Some((rank * 8 + file) as usize)
    } else {
        None
    }
}

/// A single move. Promotions always go to a queen for simplicity.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Move {
    from: Sq,
    to: Sq,
    /// Square of a pawn captured en passant (differs from `to`).
    en_passant_capture: Option<Sq>,
    /// `true` if this is a castling move (the rook is moved alongside).
    castle: bool,
    /// `true` if a pawn reaches the last rank and is promoted to a queen.
    promotion: bool,
}

impl Move {
    fn plain(from: Sq, to: Sq) -> Move {
        Move {
            from,
            to,
            en_passant_capture: None,
            castle: false,
            promotion: false,
        }
    }
}

/// Full board state plus the rights needed for castling and en passant.
#[derive(Clone)]
struct Board {
    squares: [Option<Piece>; 64],
    to_move: Side,
    /// Target square a pawn could be captured on by en passant this turn.
    en_passant: Option<Sq>,
    white_can_oo: bool,
    white_can_ooo: bool,
    black_can_oo: bool,
    black_can_ooo: bool,
}

impl Board {
    fn initial() -> Board {
        let mut squares = [None; 64];
        let back = [
            Kind::Rook,
            Kind::Knight,
            Kind::Bishop,
            Kind::Queen,
            Kind::King,
            Kind::Bishop,
            Kind::Knight,
            Kind::Rook,
        ];
        for (file, &kind) in back.iter().enumerate() {
            squares[file] = Some(Piece {
                side: Side::Black,
                kind,
            });
            squares[8 + file] = Some(Piece {
                side: Side::Black,
                kind: Kind::Pawn,
            });
            squares[48 + file] = Some(Piece {
                side: Side::White,
                kind: Kind::Pawn,
            });
            squares[56 + file] = Some(Piece {
                side: Side::White,
                kind,
            });
        }
        Board {
            squares,
            to_move: Side::White,
            en_passant: None,
            white_can_oo: true,
            white_can_ooo: true,
            black_can_oo: true,
            black_can_ooo: true,
        }
    }

    fn piece_at(&self, sq: Sq) -> Option<Piece> {
        self.squares.get(sq).copied().flatten()
    }

    fn king_sq(&self, side: Side) -> Option<Sq> {
        self.squares
            .iter()
            .position(|p| matches!(p, Some(pc) if pc.side == side && pc.kind == Kind::King))
    }

    /// Is `sq` attacked by any piece of `by`? Ignores pins and check (raw
    /// attack map), which is exactly what's needed for legality testing.
    fn is_attacked(&self, sq: Sq, by: Side) -> bool {
        let f = file_of(sq);
        let r = rank_of(sq);

        // Pawn attacks. A `by`-side pawn attacks diagonally "forward".
        let pawn_dir = match by {
            Side::White => -1, // white pawns move up (towards rank index 0)
            Side::Black => 1,
        };
        for df in [-1, 1] {
            if let Some(s) = sq_at(f + df, r - pawn_dir)
                && let Some(p) = self.piece_at(s)
                && p.side == by
                && p.kind == Kind::Pawn
            {
                return true;
            }
        }

        // Knight attacks.
        const KN: [(i32, i32); 8] = [
            (1, 2),
            (2, 1),
            (-1, 2),
            (-2, 1),
            (1, -2),
            (2, -1),
            (-1, -2),
            (-2, -1),
        ];
        for (df, dr) in KN {
            if let Some(s) = sq_at(f + df, r + dr)
                && let Some(p) = self.piece_at(s)
                && p.side == by
                && p.kind == Kind::Knight
            {
                return true;
            }
        }

        // King attacks (adjacent squares).
        for df in -1..=1 {
            for dr in -1..=1 {
                if df == 0 && dr == 0 {
                    continue;
                }
                if let Some(s) = sq_at(f + df, r + dr)
                    && let Some(p) = self.piece_at(s)
                    && p.side == by
                    && p.kind == Kind::King
                {
                    return true;
                }
            }
        }

        // Sliding: bishops/queens on diagonals, rooks/queens on files/ranks.
        const DIAG: [(i32, i32); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];
        const ORTHO: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
        for (dirs, slide_kinds) in [
            (DIAG, [Kind::Bishop, Kind::Queen]),
            (ORTHO, [Kind::Rook, Kind::Queen]),
        ] {
            for (df, dr) in dirs {
                let mut cf = f + df;
                let mut cr = r + dr;
                while let Some(s) = sq_at(cf, cr) {
                    if let Some(p) = self.piece_at(s) {
                        if p.side == by && slide_kinds.contains(&p.kind) {
                            return true;
                        }
                        break; // first blocker stops the ray
                    }
                    cf += df;
                    cr += dr;
                }
            }
        }

        false
    }

    fn in_check(&self, side: Side) -> bool {
        match self.king_sq(side) {
            Some(k) => self.is_attacked(k, side.opposite()),
            None => false,
        }
    }

    /// Apply a move, returning the resulting board (does not verify legality).
    fn apply(&self, mv: Move) -> Board {
        let mut b = self.clone();
        let mover = match b.squares[mv.from] {
            Some(p) => p,
            None => return b, // defensive: nothing to move
        };

        // Remove an en-passant-captured pawn.
        if let Some(cap) = mv.en_passant_capture {
            b.squares[cap] = None;
        }

        // Move the piece.
        b.squares[mv.from] = None;
        let placed = if mv.promotion {
            Piece {
                side: mover.side,
                kind: Kind::Queen,
            }
        } else {
            mover
        };
        b.squares[mv.to] = Some(placed);

        // Castling: relocate the rook.
        if mv.castle {
            let rank = rank_of(mv.from);
            if file_of(mv.to) == 6 {
                // king side
                if let (Some(rf), Some(rt)) = (sq_at(7, rank), sq_at(5, rank)) {
                    b.squares[rt] = b.squares[rf].take();
                }
            } else if file_of(mv.to) == 2 {
                // queen side
                if let (Some(rf), Some(rt)) = (sq_at(0, rank), sq_at(3, rank)) {
                    b.squares[rt] = b.squares[rf].take();
                }
            }
        }

        // Update castling rights when king or rooks move (or are captured).
        if mover.kind == Kind::King {
            match mover.side {
                Side::White => {
                    b.white_can_oo = false;
                    b.white_can_ooo = false;
                }
                Side::Black => {
                    b.black_can_oo = false;
                    b.black_can_ooo = false;
                }
            }
        }
        for sq in [mv.from, mv.to] {
            match sq {
                56 => b.white_can_ooo = false, // a1
                63 => b.white_can_oo = false,  // h1
                0 => b.black_can_ooo = false,  // a8
                7 => b.black_can_oo = false,   // h8
                _ => {}
            }
        }

        // Set new en-passant target if a pawn made a double step.
        b.en_passant = None;
        if mover.kind == Kind::Pawn {
            let dr = rank_of(mv.to) - rank_of(mv.from);
            if dr == 2 || dr == -2 {
                b.en_passant = sq_at(file_of(mv.from), (rank_of(mv.from) + rank_of(mv.to)) / 2);
            }
        }

        b.to_move = mover.side.opposite();
        b
    }

    /// All pseudo-legal moves for the side to move (may leave king in check).
    fn pseudo_moves(&self) -> Vec<Move> {
        let side = self.to_move;
        let mut moves = Vec::new();
        for from in 0..64 {
            let piece = match self.piece_at(from) {
                Some(p) if p.side == side => p,
                _ => continue,
            };
            let f = file_of(from);
            let r = rank_of(from);
            match piece.kind {
                Kind::Pawn => self.pawn_moves(from, f, r, side, &mut moves),
                Kind::Knight => {
                    const KN: [(i32, i32); 8] = [
                        (1, 2),
                        (2, 1),
                        (-1, 2),
                        (-2, 1),
                        (1, -2),
                        (2, -1),
                        (-1, -2),
                        (-2, -1),
                    ];
                    for (df, dr) in KN {
                        if let Some(to) = sq_at(f + df, r + dr)
                            && self.piece_at(to).map(|p| p.side) != Some(side)
                        {
                            moves.push(Move::plain(from, to));
                        }
                    }
                }
                Kind::King => {
                    for df in -1..=1 {
                        for dr in -1..=1 {
                            if df == 0 && dr == 0 {
                                continue;
                            }
                            if let Some(to) = sq_at(f + df, r + dr)
                                && self.piece_at(to).map(|p| p.side) != Some(side)
                            {
                                moves.push(Move::plain(from, to));
                            }
                        }
                    }
                    self.castle_moves(side, &mut moves);
                }
                Kind::Bishop => self.slide(from, f, r, side, &DIAG, &mut moves),
                Kind::Rook => self.slide(from, f, r, side, &ORTHO, &mut moves),
                Kind::Queen => {
                    self.slide(from, f, r, side, &DIAG, &mut moves);
                    self.slide(from, f, r, side, &ORTHO, &mut moves);
                }
            }
        }
        moves
    }

    fn slide(
        &self,
        from: Sq,
        f: i32,
        r: i32,
        side: Side,
        dirs: &[(i32, i32)],
        out: &mut Vec<Move>,
    ) {
        for &(df, dr) in dirs {
            let mut cf = f + df;
            let mut cr = r + dr;
            while let Some(to) = sq_at(cf, cr) {
                match self.piece_at(to) {
                    None => out.push(Move::plain(from, to)),
                    Some(p) => {
                        if p.side != side {
                            out.push(Move::plain(from, to));
                        }
                        break;
                    }
                }
                cf += df;
                cr += dr;
            }
        }
    }

    fn pawn_moves(&self, from: Sq, f: i32, r: i32, side: Side, out: &mut Vec<Move>) {
        // White moves up the board (towards rank index 0).
        let dir = match side {
            Side::White => -1,
            Side::Black => 1,
        };
        let start_rank = match side {
            Side::White => 6,
            Side::Black => 1,
        };
        let last_rank = match side {
            Side::White => 0,
            Side::Black => 7,
        };

        // Single push.
        if let Some(one) = sq_at(f, r + dir)
            && self.piece_at(one).is_none()
        {
            push_pawn(from, one, last_rank, out);
            // Double push from the start rank.
            if r == start_rank
                && let Some(two) = sq_at(f, r + 2 * dir)
                && self.piece_at(two).is_none()
            {
                out.push(Move::plain(from, two));
            }
        }

        // Captures (including en passant).
        for df in [-1, 1] {
            if let Some(to) = sq_at(f + df, r + dir) {
                if let Some(p) = self.piece_at(to) {
                    if p.side != side {
                        push_pawn(from, to, last_rank, out);
                    }
                } else if self.en_passant == Some(to) {
                    // Captured pawn sits on the moving pawn's own rank.
                    if let Some(cap) = sq_at(f + df, r) {
                        out.push(Move {
                            from,
                            to,
                            en_passant_capture: Some(cap),
                            castle: false,
                            promotion: false,
                        });
                    }
                }
            }
        }
    }

    fn castle_moves(&self, side: Side, out: &mut Vec<Move>) {
        if self.in_check(side) {
            return;
        }
        let rank = match side {
            Side::White => 7,
            Side::Black => 0,
        };
        let enemy = side.opposite();
        let (can_oo, can_ooo) = match side {
            Side::White => (self.white_can_oo, self.white_can_ooo),
            Side::Black => (self.black_can_oo, self.black_can_ooo),
        };
        let king_from = match sq_at(4, rank) {
            Some(s) => s,
            None => return,
        };

        // King side: squares f,g empty and king path (e,f,g) not attacked.
        if can_oo
            && let (Some(f1), Some(g1), Some(to)) = (sq_at(5, rank), sq_at(6, rank), sq_at(6, rank))
            && self.piece_at(f1).is_none()
            && self.piece_at(g1).is_none()
            && !self.is_attacked(f1, enemy)
            && !self.is_attacked(g1, enemy)
        {
            out.push(Move {
                from: king_from,
                to,
                en_passant_capture: None,
                castle: true,
                promotion: false,
            });
        }

        // Queen side: squares b,c,d empty; king path (e,d,c) not attacked.
        if can_ooo
            && let (Some(b1), Some(c1), Some(d1), Some(to)) = (
                sq_at(1, rank),
                sq_at(2, rank),
                sq_at(3, rank),
                sq_at(2, rank),
            )
            && self.piece_at(b1).is_none()
            && self.piece_at(c1).is_none()
            && self.piece_at(d1).is_none()
            && !self.is_attacked(d1, enemy)
            && !self.is_attacked(c1, enemy)
        {
            out.push(Move {
                from: king_from,
                to,
                en_passant_capture: None,
                castle: true,
                promotion: false,
            });
        }
    }

    /// Fully legal moves: pseudo-legal moves that don't leave the king in check.
    fn legal_moves(&self) -> Vec<Move> {
        let side = self.to_move;
        self.pseudo_moves()
            .into_iter()
            .filter(|&mv| !self.apply(mv).in_check(side))
            .collect()
    }

    /// Static evaluation from White's perspective (positive favours White).
    fn evaluate(&self) -> i32 {
        let mut score = 0;
        for (sq, cell) in self.squares.iter().enumerate() {
            if let Some(p) = cell {
                let mut v = p.kind.value();
                // Light positional nudge: reward central control.
                let f = file_of(sq);
                let r = rank_of(sq);
                let centre = 7 - ((3 - (3 - f).abs()).abs() + (3 - (3 - r).abs()).abs());
                v += centre.max(0);
                match p.side {
                    Side::White => score += v,
                    Side::Black => score -= v,
                }
            }
        }
        score
    }
}

fn push_pawn(from: Sq, to: Sq, last_rank: i32, out: &mut Vec<Move>) {
    if rank_of(to) == last_rank {
        out.push(Move {
            from,
            to,
            en_passant_capture: None,
            castle: false,
            promotion: true,
        });
    } else {
        out.push(Move::plain(from, to));
    }
}

const DIAG: [(i32, i32); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];
const ORTHO: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

/// Outcome of the game once neither side can sensibly continue.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Ongoing,
    WhiteWins,
    BlackWins,
    Stalemate,
}

pub struct Chess {
    board: Board,
    /// Currently highlighted square under the cursor.
    cursor: Sq,
    /// The square the player has selected to move from, if any.
    selected: Option<Sq>,
    /// Legal destinations for the selected piece (highlighted).
    targets: Vec<Sq>,
    outcome: Outcome,
    /// `true` while it's the AI's turn (rendered, then resolved next tick).
    ai_thinking: bool,
    status: String,
    rng: u64,
}

impl Game for Chess {
    fn new() -> Self {
        let board = Board::initial();
        Chess {
            board,
            cursor: 52, // e2-ish, a sensible starting cursor for White
            selected: None,
            targets: Vec::new(),
            outcome: Outcome::Ongoing,
            ai_thinking: false,
            status: String::from("White to move."),
            rng: seed(),
        }
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        if self.outcome != Outcome::Ongoing {
            if ctx.pressed(KeyCode::Enter) {
                *self = Chess::new();
            }
            return Transition::Stay;
        }

        // Resolve the AI move scheduled on the previous tick (so its
        // "thinking" state is visible for one frame).
        if self.ai_thinking {
            self.ai_thinking = false;
            self.play_ai();
            return Transition::Stay;
        }

        // Only the human (White) drives input.
        if self.board.to_move == Side::White {
            self.handle_input(ctx);
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        // Each square is rendered 4 cells wide, 2 cells tall for a roomy board.
        let board_w = 8 * 4 + 2;
        let board_h = 8 * 2 + 2;
        let title = format!(" Chess  ·  material {:+} ", self.board.evaluate());
        let field = centered(board_w, board_h, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let check_white = self.board.in_check(Side::White);
        let check_black = self.board.in_check(Side::Black);
        let target_set = &self.targets;

        let mut lines: Vec<Line> = Vec::with_capacity(16);
        for rank in 0..8i32 {
            // Two text rows per board rank for a chunky look.
            for row in 0..2 {
                let mut spans: Vec<Span> = Vec::with_capacity(8);
                for file in 0..8i32 {
                    let sq = (rank * 8 + file) as usize;
                    let light = (file + rank) % 2 == 0;
                    let mut bg = if light {
                        Color::Rgb(181, 136, 99)
                    } else {
                        Color::Rgb(101, 67, 33)
                    };
                    if self.selected == Some(sq) {
                        bg = Color::Rgb(106, 168, 79);
                    } else if target_set.contains(&sq) {
                        bg = Color::Rgb(120, 140, 90);
                    }
                    if sq == self.cursor {
                        bg = Color::Rgb(80, 120, 200);
                    }

                    let piece = self.board.piece_at(sq);
                    // King-in-check squares glow red.
                    let king_check = matches!(
                        piece,
                        Some(p) if p.kind == Kind::King
                            && ((p.side == Side::White && check_white)
                                || (p.side == Side::Black && check_black))
                    );

                    // Every cell is exactly 4 columns wide so ranks stay aligned.
                    let content = match (row, piece) {
                        (0, Some(p)) => format!(" {}  ", p.letter()),
                        _ => "    ".to_string(),
                    };
                    let fg = match piece {
                        Some(p) if p.side == Side::White => Color::White,
                        Some(_) => Color::Black,
                        None => Color::Gray,
                    };
                    let mut style = Style::default().bg(bg).fg(fg);
                    if king_check {
                        style = style
                            .bg(Color::Rgb(190, 50, 50))
                            .add_modifier(Modifier::BOLD);
                    }
                    spans.push(Span::styled(content, style));
                }
                lines.push(Line::from(spans));
            }
        }

        // Status + controls hint below the board.
        lines.push(Line::from(Span::styled(
            self.status.clone(),
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(Span::styled(
            "Arrows: move  Enter/Space: select/confirm  c: cancel  q: menu",
            Style::default().fg(Color::DarkGray),
        )));

        frame.render_widget(Paragraph::new(lines), inner);

        // Game-over overlay, snake-style.
        if self.outcome != Outcome::Ongoing {
            let msg = match self.outcome {
                Outcome::WhiteWins => " CHECKMATE · You win! · Enter: replay · q: menu ",
                Outcome::BlackWins => " CHECKMATE · AI wins · Enter: replay · q: menu ",
                Outcome::Stalemate => " STALEMATE · draw · Enter: replay · q: menu ",
                Outcome::Ongoing => "",
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
        Duration::from_millis(40)
    }
}

impl Chess {
    fn handle_input(&mut self, ctx: &GameContext) {
        let f = file_of(self.cursor);
        let r = rank_of(self.cursor);
        if ctx.pressed(KeyCode::Up)
            && let Some(s) = sq_at(f, r - 1)
        {
            self.cursor = s;
        }
        if ctx.pressed(KeyCode::Down)
            && let Some(s) = sq_at(f, r + 1)
        {
            self.cursor = s;
        }
        if ctx.pressed(KeyCode::Left)
            && let Some(s) = sq_at(f - 1, r)
        {
            self.cursor = s;
        }
        if ctx.pressed(KeyCode::Right)
            && let Some(s) = sq_at(f + 1, r)
        {
            self.cursor = s;
        }
        if ctx.pressed(KeyCode::Char('c')) {
            self.clear_selection();
        }
        if ctx.pressed(KeyCode::Enter) || ctx.pressed(KeyCode::Char(' ')) {
            self.confirm();
        }
    }

    fn clear_selection(&mut self) {
        self.selected = None;
        self.targets.clear();
        self.status = String::from("White to move.");
    }

    /// Handle a select / confirm-destination press on the cursor square.
    fn confirm(&mut self) {
        match self.selected {
            None => {
                // Select a White piece under the cursor and list its targets.
                if let Some(p) = self.board.piece_at(self.cursor) {
                    if p.side == Side::White {
                        let targets: Vec<Sq> = self
                            .board
                            .legal_moves()
                            .into_iter()
                            .filter(|m| m.from == self.cursor)
                            .map(|m| m.to)
                            .collect();
                        if targets.is_empty() {
                            self.status = String::from("That piece has no legal moves.");
                        } else {
                            self.selected = Some(self.cursor);
                            self.targets = targets;
                            self.status = String::from("Select a destination (c to cancel).");
                        }
                    } else {
                        self.status = String::from("Select one of your pieces.");
                    }
                }
            }
            Some(from) => {
                if self.cursor == from {
                    self.clear_selection();
                    return;
                }
                // Find the legal move matching from->cursor.
                let chosen = self
                    .board
                    .legal_moves()
                    .into_iter()
                    .find(|m| m.from == from && m.to == self.cursor);
                match chosen {
                    Some(mv) => {
                        self.board = self.board.apply(mv);
                        self.clear_selection();
                        if self.check_game_state(Side::Black) {
                            // Black to move next: let the AI think.
                            self.ai_thinking = true;
                            self.status = String::from("AI is thinking…");
                        }
                    }
                    None => {
                        // Reselect if the cursor landed on another White piece.
                        if let Some(p) = self.board.piece_at(self.cursor)
                            && p.side == Side::White
                        {
                            self.selected = None;
                            self.confirm();
                            return;
                        }
                        self.status = String::from("Not a legal move for that piece.");
                    }
                }
            }
        }
    }

    /// Run the AI (Black) move, then re-evaluate the game state.
    fn play_ai(&mut self) {
        if let Some(mv) = self.choose_ai_move() {
            self.board = self.board.apply(mv);
        }
        self.check_game_state(Side::White);
        if self.outcome == Outcome::Ongoing {
            self.status = String::from("White to move.");
        }
    }

    /// After a move, set `outcome` if the `side` to move is mated/stalemated.
    /// Returns `true` if the game is still ongoing.
    fn check_game_state(&mut self, side_to_move: Side) -> bool {
        if self.board.legal_moves().is_empty() {
            if self.board.in_check(side_to_move) {
                self.outcome = match side_to_move {
                    Side::White => Outcome::BlackWins,
                    Side::Black => Outcome::WhiteWins,
                };
            } else {
                self.outcome = Outcome::Stalemate;
            }
            false
        } else {
            true
        }
    }

    /// Pick Black's move with depth-2 alpha-beta minimax, breaking ties at
    /// random for variety. Black minimises the White-relative evaluation.
    fn choose_ai_move(&mut self) -> Option<Move> {
        let moves = self.board.legal_moves();
        if moves.is_empty() {
            return None;
        }
        let mut best_score = i32::MAX;
        let mut best: Vec<Move> = Vec::new();
        for &mv in &moves {
            let next = self.board.apply(mv);
            let score = minimax(&next, 2, i32::MIN + 1, i32::MAX - 1);
            if score < best_score {
                best_score = score;
                best.clear();
                best.push(mv);
            } else if score == best_score {
                best.push(mv);
            }
        }
        if best.is_empty() {
            return moves.first().copied();
        }
        let idx = (self.next_rand() % best.len() as u64) as usize;
        best.get(idx).copied()
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

/// Negamax-style alpha-beta over the White-relative evaluation. The side to
/// move in `board` is the maximiser when White, minimiser when Black.
fn minimax(board: &Board, depth: u32, mut alpha: i32, mut beta: i32) -> i32 {
    let moves = board.legal_moves();
    if moves.is_empty() {
        if board.in_check(board.to_move) {
            // Checkmate: huge value for the delivering side. Adjust by depth so
            // faster mates are preferred.
            return match board.to_move {
                Side::White => -100_000 - depth as i32,
                Side::Black => 100_000 + depth as i32,
            };
        }
        return 0; // stalemate
    }
    if depth == 0 {
        return board.evaluate();
    }

    if board.to_move == Side::White {
        let mut best = i32::MIN + 1;
        for mv in moves {
            let score = minimax(&board.apply(mv), depth - 1, alpha, beta);
            if score > best {
                best = score;
            }
            if best > alpha {
                alpha = best;
            }
            if alpha >= beta {
                break;
            }
        }
        best
    } else {
        let mut best = i32::MAX - 1;
        for mv in moves {
            let score = minimax(&board.apply(mv), depth - 1, alpha, beta);
            if score < best {
                best = score;
            }
            if best < beta {
                beta = best;
            }
            if alpha >= beta {
                break;
            }
        }
        best
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
    Chess,
    id: "chess",
    name: "Chess",
    description: "Full legal chess versus a simple minimax AI.",
    author: "furybee",
}
