#!/bin/sh
# Install the diff-narrative skill for Claude Code, Codex and/or GitHub
# Copilot CLI. All three read the same SKILL.md format, each from its own
# folder.
#
#   ./install-skill.sh                 # every agent CLI found on PATH
#   ./install-skill.sh claude codex    # just these
#   ./install-skill.sh --dry-run       # show what would happen
#
# The skill is copied (not linked), so it keeps working if this checkout
# moves. Re-run the script to update it. Optional: install the `structdiff`
# command (./build.sh) so narratives match the viewer's fingerprint exactly;
# without it the skill still works and the viewer marks the narrative
# "unverified".
set -eu

skill_src="$(cd "$(dirname "$0")" && pwd)/skill/diff-narrative"
name="diff-narrative"
dry_run=""

skills_dir() {
  case "$1" in
    claude) echo "${CLAUDE_CONFIG_DIR:-$HOME/.claude}/skills" ;;
    codex) echo "$HOME/.agents/skills" ;;
    copilot) echo "${COPILOT_HOME:-$HOME/.copilot}/skills" ;;
    *) return 1 ;;
  esac
}

usage() {
  sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

run() {
  if [ -n "$dry_run" ]; then echo "  would run: $*"; else "$@"; fi
}

install_for() {
  agent="$1"
  dir="$(skills_dir "$agent")" || { echo "unknown agent: $agent (use claude, codex or copilot)" >&2; exit 2; }
  target="$dir/$name"
  echo "$agent: $target"
  if [ -L "$target" ]; then
    run rm "$target"
  elif [ -e "$target" ]; then
    if grep -qs "^name: $name\$" "$target/SKILL.md"; then
      run rm -rf "$target" # a previous install of this skill
    else
      backup="$target.bak.$(date +%Y%m%d%H%M%S)"
      echo "  something else is there; moving it to $backup"
      run mv "$target" "$backup"
    fi
  fi
  run mkdir -p "$dir"
  run cp -R "$skill_src" "$target"
}

agents=""
for arg in "$@"; do
  case "$arg" in
    -h | --help) usage ;;
    -n | --dry-run) dry_run=1 ;;
    claude | codex | copilot) agents="$agents $arg" ;;
    *) echo "unknown argument: $arg" >&2; usage 2 >&2 ;;
  esac
done

[ -f "$skill_src/SKILL.md" ] || { echo "skill not found at $skill_src" >&2; exit 1; }

if [ -z "$agents" ]; then
  for agent in claude codex copilot; do
    command -v "$agent" > /dev/null 2>&1 && agents="$agents $agent"
  done
  [ -n "$agents" ] || { echo "no agent CLI found on PATH (claude, codex, copilot); name one explicitly" >&2; exit 1; }
fi

for agent in $agents; do
  install_for "$agent"
done
[ -n "$dry_run" ] || echo "done. Agents load skills at startup, so restart any running session."
command -v structdiff > /dev/null 2>&1 || echo "note: \`structdiff\` isn't on PATH; narratives will show as unverified (see ./build.sh)."
