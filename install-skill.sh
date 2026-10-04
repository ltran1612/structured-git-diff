#!/bin/sh
# Install structdiff's skills (every folder under skill/: diff-narrative,
# structdiff-groups) for Claude Code, Codex and/or GitHub Copilot CLI. All
# three read the same SKILL.md format, each from its own folder.
#
#   ./install-skill.sh                 # every agent CLI found on PATH
#   ./install-skill.sh claude codex    # just these
#   ./install-skill.sh --dry-run       # show what would happen
#
# Skills are copied (not linked), so they keep working if this checkout
# moves. Re-run the script to update them. Optional: the `structdiff`
# command (./build.sh) makes narratives match the viewer exactly and lets
# structdiff-groups check groupings; both skills work without it.
set -eu

skills_root="$(cd "$(dirname "$0")" && pwd)/skill"
backups="$HOME/.structdiff-skill-backups"
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
  sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

run() {
  if [ -n "$dry_run" ]; then echo "  would run: $*"; else "$@"; fi
}

# True if $1 is a previous install of skill $2 (its SKILL.md declares that
# name; CRLF line endings allowed).
is_our_skill() {
  [ -f "$1/SKILL.md" ] && tr -d '\r' < "$1/SKILL.md" | grep -qs "^name: $2\$"
}

install_skill() {
  agent="$1"
  skill_src="$2"
  name="$(basename "$skill_src")"
  dir="$(skills_dir "$agent")" || { echo "unknown agent: $agent (use claude, codex or copilot)" >&2; exit 2; }
  target="$dir/$name"

  # Copilot also loads ~/.claude/skills and ~/.agents/skills, so a third
  # copy would show up twice.
  if [ "$agent" = copilot ]; then
    case " $agents " in
      *" claude "* | *" codex "*)
        echo "copilot: skipping $name; Copilot also loads Claude's and Codex's skill folders"
        return
        ;;
    esac
    for shared in "$(skills_dir claude)/$name" "$(skills_dir codex)/$name"; do
      if is_our_skill "$shared" "$name"; then
        echo "copilot: skipping $name; Copilot already loads it from $shared"
        return
      fi
    done
  fi

  echo "$agent: $target"
  if [ -L "$target" ]; then
    run rm "$target"
  elif [ -e "$target" ]; then
    if is_our_skill "$target" "$name"; then
      run rm -rf "$target" # a previous install of this skill
    else
      # Keep backups out of the skills folder, where they'd load as skills.
      backup="$backups/$agent/$name.$(date +%Y%m%d%H%M%S)"
      echo "  something else is there; moving it to $backup"
      run mkdir -p "$(dirname "$backup")"
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

found=0
for d in "$skills_root"/*/; do
  [ -f "$d/SKILL.md" ] && found=1
done
[ "$found" = 1 ] || { echo "no skills found under $skills_root" >&2; exit 1; }

if [ -z "$agents" ]; then
  for agent in claude codex copilot; do
    command -v "$agent" > /dev/null 2>&1 && agents="$agents $agent"
  done
  [ -n "$agents" ] || { echo "no agent CLI found on PATH (claude, codex, copilot); name one explicitly" >&2; exit 1; }
fi

# Agents are fixed words, so splitting $agents is safe. Skill folders are
# globbed afresh and always quoted, so a checkout path with spaces or glob
# characters works.
for agent in $agents; do
  for d in "$skills_root"/*/; do
    [ -f "$d/SKILL.md" ] && install_skill "$agent" "${d%/}"
  done
done
[ -n "$dry_run" ] || echo "done. Agents load skills at startup, so restart any running session."
command -v structdiff > /dev/null 2>&1 || echo "note: \`structdiff\` isn't on PATH; narratives will show as unverified (see ./build.sh)."
