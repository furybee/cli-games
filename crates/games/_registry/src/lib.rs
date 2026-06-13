//! Aggregator crate. It contains no logic — its only job is to force every game
//! crate to be linked into the final binary so their `inventory` registrations
//! are present at runtime.
//!
//! To add a game, append ONE line below (and the matching dependency in
//! Cargo.toml). The `as _` import links the crate without using any of its
//! symbols. Keep entries alphabetical and append-only for conflict-free merges.

use game_2048 as _;
use game_anagram as _;
use game_asteroids as _;
use game_blackjack as _;
use game_boggle as _;
use game_breakout as _;
use game_chess as _;
use game_conway as _;
use game_dinorun as _;
use game_flappy as _;
use game_freecell as _;
use game_hangman as _;
use game_lightsout as _;
use game_mastermind as _;
use game_maze as _;
use game_memory as _;
use game_minesweeper as _;
use game_nonogram as _;
use game_peg as _;
use game_pong as _;
use game_racer as _;
use game_roguelike as _;
use game_simon as _;
use game_slidepuzzle as _;
use game_snake as _;
use game_sokoban as _;
use game_solitaire as _;
use game_spaceinvaders as _;
use game_sudoku as _;
use game_tetris as _;
use game_tictactoe as _;
use game_tron as _;
use game_typing as _;
use game_videopoker as _;
use game_wordle as _;
use game_yahtzee as _;
