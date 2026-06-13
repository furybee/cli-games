//! Tic-Tac-Toe — you play `X`, an unbeatable minimax AI plays `O`.
//!
//! Move the cursor with the arrow keys, drop your mark with Enter / Space.
//! The AI replies instantly. When the board is decided, Enter starts a new
//! round. Self-registers with the launcher like every other game.

use std::time::Duration;

use game_core::{Game, GameContext, KeyCode, Transition, register_game};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    Empty,
    X,
    O,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// The game is still in progress.
    Playing,
    /// `X` (the human) won.
    WinX,
    /// `O` (the AI) won.
    WinO,
    /// The board is full with no winner.
    Draw,
}

pub struct TicTacToe {
    board: [Cell; 9],
    /// Highlighted cell the player will fill next (0..9).
    cursor: usize,
    outcome: Outcome,
}

impl Game for TicTacToe {
    fn new() -> Self {
        TicTacToe {
            board: [Cell::Empty; 9],
            cursor: 4, // centre
            outcome: Outcome::Playing,
        }
    }

    fn update(&mut self, ctx: &GameContext) -> Transition {
        if ctx.pressed(KeyCode::Char('q')) || ctx.pressed(KeyCode::Esc) {
            return Transition::Exit;
        }

        // Round is decided: wait for Enter to replay.
        if self.outcome != Outcome::Playing {
            if ctx.pressed(KeyCode::Enter) {
                *self = TicTacToe::new();
            }
            return Transition::Stay;
        }

        // Move the cursor; rows wrap top/bottom, columns wrap left/right.
        if ctx.pressed(KeyCode::Left) {
            self.cursor = self.cursor - (self.cursor % 3) + (self.cursor + 2) % 3;
        }
        if ctx.pressed(KeyCode::Right) {
            self.cursor = self.cursor - (self.cursor % 3) + (self.cursor + 1) % 3;
        }
        if ctx.pressed(KeyCode::Up) {
            self.cursor = (self.cursor + 6) % 9;
        }
        if ctx.pressed(KeyCode::Down) {
            self.cursor = (self.cursor + 3) % 9;
        }

        // Place a mark on Enter / Space.
        if (ctx.pressed(KeyCode::Enter) || ctx.pressed(KeyCode::Char(' ')))
            && self.board[self.cursor] == Cell::Empty
        {
            self.board[self.cursor] = Cell::X;
            self.outcome = evaluate(&self.board);
            if self.outcome == Outcome::Playing {
                self.ai_move();
                self.outcome = evaluate(&self.board);
            }
        }

        Transition::Stay
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        // 3 cells × 4 cols wide + grid lines, 3 rows × 2 high + lines.
        let title = " Tic-Tac-Toe  ·  you: X  ·  AI: O ";
        let field = centered(19, 9, area);
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(field);
        frame.render_widget(block, field);

        let mut lines: Vec<Line> = Vec::with_capacity(5);
        for row in 0..3 {
            let mut spans = Vec::with_capacity(5);
            for col in 0..3 {
                let idx = row * 3 + col;
                spans.push(self.cell_span(idx));
                if col < 2 {
                    spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
                }
            }
            lines.push(Line::from(spans));
            if row < 2 {
                lines.push(Line::styled(
                    "───┼───┼───",
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }
        frame.render_widget(Paragraph::new(lines), inner);

        if self.outcome != Outcome::Playing {
            let msg = match self.outcome {
                Outcome::WinX => " You win!  ·  Enter: replay · q: menu ",
                Outcome::WinO => " AI wins.  ·  Enter: replay · q: menu ",
                Outcome::Draw => " Draw.  ·  Enter: replay · q: menu ",
                Outcome::Playing => unreachable!(),
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

impl TicTacToe {
    /// Render one cell as ` X `, ` O ` or ` · `, highlighting the cursor.
    fn cell_span(&self, idx: usize) -> Span<'static> {
        let (glyph, color) = match self.board[idx] {
            Cell::X => ("X", Color::LightCyan),
            Cell::O => ("O", Color::LightRed),
            Cell::Empty => ("·", Color::DarkGray),
        };
        let text = format!(" {glyph} ");
        let mut style = Style::default().fg(color);
        // Only highlight the cursor while the round is live.
        if idx == self.cursor && self.outcome == Outcome::Playing {
            style = Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD);
        }
        Span::styled(text, style)
    }

    /// Play the AI's `O` using minimax — it never loses.
    fn ai_move(&mut self) {
        let mut best_score = i32::MIN;
        let mut best_move = None;
        for idx in 0..9 {
            if self.board[idx] == Cell::Empty {
                self.board[idx] = Cell::O;
                let score = minimax(&self.board, false);
                self.board[idx] = Cell::Empty;
                if score > best_score {
                    best_score = score;
                    best_move = Some(idx);
                }
            }
        }
        if let Some(idx) = best_move {
            self.board[idx] = Cell::O;
        }
    }
}

const LINES: [[usize; 3]; 8] = [
    [0, 1, 2],
    [3, 4, 5],
    [6, 7, 8],
    [0, 3, 6],
    [1, 4, 7],
    [2, 5, 8],
    [0, 4, 8],
    [2, 4, 6],
];

/// Classify the board: win for either side, a draw, or still in play.
fn evaluate(board: &[Cell; 9]) -> Outcome {
    for line in LINES {
        let [a, b, c] = line;
        if board[a] != Cell::Empty && board[a] == board[b] && board[b] == board[c] {
            return match board[a] {
                Cell::X => Outcome::WinX,
                Cell::O => Outcome::WinO,
                Cell::Empty => unreachable!(),
            };
        }
    }
    if board.iter().all(|&c| c != Cell::Empty) {
        Outcome::Draw
    } else {
        Outcome::Playing
    }
}

/// Minimax score from `O`'s perspective: +10 for an O win (sooner is better),
/// -10 for an X win, 0 for a draw. `o_turn` is whose move it is now.
fn minimax(board: &[Cell; 9], o_turn: bool) -> i32 {
    match evaluate(board) {
        Outcome::WinO => return 10,
        Outcome::WinX => return -10,
        Outcome::Draw => return 0,
        Outcome::Playing => {}
    }

    let mut next = *board;
    if o_turn {
        let mut best = i32::MIN;
        for idx in 0..9 {
            if next[idx] == Cell::Empty {
                next[idx] = Cell::O;
                best = best.max(minimax(&next, false));
                next[idx] = Cell::Empty;
            }
        }
        best
    } else {
        let mut best = i32::MAX;
        for idx in 0..9 {
            if next[idx] == Cell::Empty {
                next[idx] = Cell::X;
                best = best.min(minimax(&next, true));
                next[idx] = Cell::Empty;
            }
        }
        best
    }
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
    TicTacToe,
    id: "tictactoe",
    name: "Tic-Tac-Toe",
    description: "Outsmart an unbeatable AI — or settle for a draw.",
    author: "furybee",
}
