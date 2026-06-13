#!/usr/bin/env bash
#
# Batch-spawn one isolated Claude Code session per game, each in its own git
# worktree, using Claude Code's built-in `--worktree` flag. The flag creates the
# worktree (under .claude/worktrees/<game>/, branched from origin/HEAD), starts
# Claude in it, and cleans it up on exit — so we don't manage worktrees by hand.
#
#   ./scripts/spawn-games.sh tetris 2048 minesweeper      # print the commands
#   ./scripts/spawn-games.sh --run tetris 2048            # actually launch them
#
# Without --run it only prints the commands, so you can paste each into its own
# terminal (recommended: you watch each session). With --run it launches them in
# the background via `claude -p` (headless, non-interactive).

set -euo pipefail

run=false
if [ "${1:-}" = "--run" ]; then
  run=true
  shift
fi

if [ "$#" -eq 0 ]; then
  echo "usage: $0 [--run] <game> [game...]" >&2
  echo "example: $0 tetris 2048 minesweeper" >&2
  exit 1
fi

prompt_for() {
  local game="$1"
  printf '%s' "Implement the '${game}' mini-game for this Rust workspace. Read CLAUDE.md and docs/ADD_A_GAME.md FIRST. Add ONLY crates/games/${game}/ plus the two append-only lines in crates/games/_registry/. Do NOT touch core/, app/, or other games, and never run bare 'cargo fmt'. Verify with: cargo build && cargo run -p cli-games -- ${game}."
}

for raw in "$@"; do
  game="$(echo "$raw" | tr '[:upper:] _' '[:lower:]--')"
  prompt="$(prompt_for "$game")"

  if [ "$run" = true ]; then
    echo "▶ launching '$game' (headless, background)…"
    claude -p --worktree "$game" "$prompt" &
  else
    echo "# --- $game ---"
    echo "claude --worktree $game \"$prompt\""
    echo
  fi
done

if [ "$run" = true ]; then
  echo "All sessions launched in the background. Wait for them with: wait"
  echo "Each worktree lives under .claude/worktrees/<game>/ until the session exits."
fi
