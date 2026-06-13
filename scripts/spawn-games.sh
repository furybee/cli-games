#!/usr/bin/env bash
#
# Print one ready-to-paste `claude --worktree <game>` command per game, so you
# can launch each in its own terminal and watch it work. Claude Code's built-in
# `--worktree` flag creates the worktree (under .claude/worktrees/<game>/,
# branched from origin/HEAD), starts an interactive session in it, and cleans it
# up on exit — so worktrees are never managed by hand.
#
#   ./scripts/spawn-games.sh tetris 2048 minesweeper
#
# Run each printed command in a separate terminal. Always run interactively:
# headless background launches (`claude -p &`) detach from the terminal, give no
# visibility, and can hang silently — don't go there.

set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "usage: $0 <game> [game...]" >&2
  echo "example: $0 tetris 2048 minesweeper" >&2
  exit 1
fi

prompt_for() {
  local game="$1"
  printf '%s' "Implement the '${game}' mini-game for this Rust workspace. Read CLAUDE.md and docs/ADD_A_GAME.md FIRST. Add ONLY crates/games/${game}/ plus the two append-only lines in crates/games/_registry/. Do NOT touch core/, app/, or other games, and never run bare 'cargo fmt'. Verify with: cargo build && cargo run -p cli-games -- ${game}."
}

for raw in "$@"; do
  game="$(echo "$raw" | tr '[:upper:] _' '[:lower:]--')"
  echo "# --- $game (run in its own terminal) ---"
  echo "claude --worktree $game \"$(prompt_for "$game")\""
  echo
done
