#!/usr/bin/env bash
#
# Prepare one isolated git worktree + branch per game, then print the command
# to launch a Claude agent in each. Run from anywhere inside the repo.
#
#   ./scripts/spawn-games.sh tetris 2048 minesweeper
#
# Each game gets:
#   - branch  game/<name>
#   - worktree ../cli-games-<name>
# so several agents can build in parallel without colliding. Merges stay
# conflict-free because each only adds files + two append-only registry lines.

set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "usage: $0 <game> [game...]" >&2
  echo "example: $0 tetris 2048 minesweeper" >&2
  exit 1
fi

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

# Make sure local main is current so every worktree branches from the same base.
git fetch --quiet origin main || true

echo "Repo: $repo_root"
echo

for raw in "$@"; do
  # Normalize: lowercase, spaces/underscores -> hyphens.
  name="$(echo "$raw" | tr '[:upper:] _' '[:lower:]--')"
  branch="game/${name}"
  worktree="${repo_root}/../cli-games-${name}"

  if git show-ref --quiet "refs/heads/${branch}"; then
    echo "⚠ branch ${branch} already exists — skipping ${name}"
    continue
  fi
  if [ -e "$worktree" ]; then
    echo "⚠ ${worktree} already exists — skipping ${name}"
    continue
  fi

  git worktree add -b "$branch" "$worktree" main >/dev/null
  echo "✓ ${name}"
  echo "    worktree: ${worktree}"
  echo "    branch:   ${branch}"
  echo "    launch:   (cd \"${worktree}\" && claude \"$(printf '%s' "Implement the '${name}' mini-game. Read CLAUDE.md and docs/ADD_A_GAME.md first, then add ONLY crates/games/${name}/ plus the two append-only registry lines. Verify with: cargo build && cargo run -p cli-games -- ${name}.")\")"
  echo
done

echo "When a game is done, from its worktree:"
echo "    git add -A && git commit -m \"feat(<game>): implement <game>\" && git push -u origin game/<game>"
echo "    gh pr create --fill"
echo
echo "Clean up a finished worktree:  git worktree remove ../cli-games-<game>"
