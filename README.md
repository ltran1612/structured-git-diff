# structdiff.nvim

A grouped view of your working-tree diff for Neovim, with an AI-written narrative of *why* each file changed.

```
 StructDiff  5 files                    │ HEAD                     │ working tree
 narrative up to date                   │                          │
                                        │                          │
▾ Source (2)                            │                          │
  Adds the backoff helper and the retry │                          │
  loop in `fetch` that uses it.         │                          │
  ? backoff.py  src                     │                          │
      Adds capped exponential sleep     │                          │
      helper used by fetch's retry loop │                          │
  M http.py  src                        │                          │
▾ Config/Build (1)                      │                          │
  M config.toml                         │                          │
─────────────────────────────────────────────────────────────────────────────
# Change narrative   (overall story, then one section per group)
```

- **Grouping:** changed files are grouped by regex path patterns. By default you see your uncommitted work (staged, unstaged and untracked, all against HEAD). You can also review a branch: `:StructDiff main...HEAD`.
- **Navigation:** move between groups and files. Each file opens as a native side-by-side `:diffthis` against HEAD. The right-hand side is the real file, so you can edit it.
- **Narrative:** the `diff-narrative` Claude Code skill writes `.structdiff/narrative.json`, and the view shows it:
  - an overall story,
  - a "why" for each group,
  - a one-line reason for each file,
  - a suggested reading order.

## Requirements

Neovim ≥ 0.10, git. For narratives you also need [Claude Code](https://claude.com/claude-code).

## Install

lazy.nvim:

```lua
{
  "…/structdiff.nvim", -- or dir = "~/path/to/organizeddiffview"
  cmd = { "StructDiff", "StructDiffClose", "StructDiffRefresh", "StructDiffNarrative", "StructDiffGenerate" },
  keys = { { "<leader>gv", "<cmd>StructDiff<cr>", desc = "StructDiff" } },
  opts = {},
}
```

Install the skill so Claude Code can find it:

```sh
ln -s /path/to/organizeddiffview/skill/diff-narrative ~/.claude/skills/diff-narrative
```

## Usage

| Command | |
|---|---|
| `:StructDiff [range]` | Open the view (or refresh it if it's already open with the same range) |
| `:StructDiffGenerate [range]` | Run the skill through `claude -p` for that range, and reload when it finishes |
| `:StructDiffNarrative` | Toggle the narrative split |
| `:StructDiffRefresh` | Re-scan git and reload the narrative |
| `:StructDiffClose` | Close the view |

| Key | Where | |
|---|---|---|
| `]g` / `[g` | everywhere in the view | next / previous group |
| `]f` / `[f` | everywhere in the view | next / previous file, continuing across groups |
| `<CR>` | sidebar | open file / fold group |
| `za`, `<Tab>` | sidebar | fold group |
| `gn` | sidebar, HEAD side, narrative | toggle narrative split |
| `gr` | sidebar, HEAD side, narrative | toggle reasons in the sidebar |
| `R` | sidebar, HEAD side, narrative | refresh |
| `q` | sidebar, HEAD side | close (in the narrative split, closes just the split) |

The real file on the right only gets the `]g [g ]f [f` keys, and only while it's shown in the view. `q`, `R` and `gn` keep their normal meaning when you edit there.

You can also run `/diff-narrative [range]` in any Claude Code session in the repo. The open view watches `.structdiff/` and reloads as soon as the file is written.

## Ranges

| Range | Left | Right |
|---|---|---|
| *(none)* | HEAD | working tree, including untracked files |
| `main` | `main` | working tree, including untracked files |
| `main..feature` | `main` | `feature` |
| `main...HEAD` | merge-base of `main` and HEAD | HEAD: what the branch adds, like a PR |

An empty side of `..` or `...` means HEAD, so `:StructDiff main...` is the usual "review my branch". Ref names tab-complete, including the part after the dots.

When the right side is a revision, both sides are read-only buffers. Uncommitted edits don't appear and don't make the narrative stale. Ranges are re-resolved on every refresh, so new commits on either branch are picked up with `R`.

## Grouping

Groups are an ordered list. Each pattern is a Vim regex in very-magic mode (`\v` is prepended), matched against the repo-relative path. The **first** matching group wins, and anything unmatched goes to `other_group`. Empty groups are hidden.

```lua
opts = {
  groups = {
    { name = "API", patterns = { [[^server/api/]] } },
    { name = "Tests", patterns = { [[_test\.go$]], [[^e2e/]] } },
    { name = "Frontend", patterns = { [[^web/]], [[\.(tsx|css)$]] } },
  },
  other_group = "Other",
}
```

Defaults: Tests, CI, Config/Build, Docs, Source, Other (see `lua/structdiff/config.lua`).

In very-magic mode, `< > = @ % { }` are special. Escape them with `\` when you mean them literally.

**Per-repo groups:** put a committable `.structdiff.json` at the repo root:

```json
{ "groups": [ { "name": "Migrations", "patterns": ["^db/migrate/"] } ] }
```

Groups are displayed in config order. Once a narrative exists, the group holding the story's first file (the root cause) moves to the top, and files within each group follow the narrative's reading order.

## The narrative contract

Everything lives in `<repo>/.structdiff/`, which the plugin adds to `.git/info/exclude`, so it never shows up in `git status`.

- `groups.json` (written by the viewer on every open and refresh): the range (`spec`, resolved `base`/`target` SHAs), the `output` file name, the groups and their files, and a `fingerprint` of the change set.
- `narrative.json` for the working tree, or `narrative-<range>.json` for a range (characters outside `A-Za-z0-9._-` become `_`, so `origin/main...HEAD` → `narrative-origin_main...HEAD.json`). The skill writes it in this shape:

```json
{
  "version": 1,
  "fingerprint": "<copied from groups.json>",
  "overall": "markdown story",
  "groups": { "Source": "why this group changed" },
  "files": { "src/http.py": "one-line reason" },
  "order": ["src/backoff.py", "src/http.py"]
}
```

Each range has its own file, so reviewing a branch and then going back to your uncommitted work never overwrites either narrative. The file is keyed by what you typed, so `main...HEAD` and `main...feature` get separate narratives even when they point at the same commits.

The fingerprint is a sha256 of the range's diff, plus the contents of untracked files when comparing against the working tree. If the diff changes after the narrative was written, the sidebar shows **narrative stale**. The old narrative stays visible until you regenerate it.

Any tool can produce this file. The skill is just the default producer.

## Configuration

```lua
opts = {
  sidebar_width = 40,
  narrative_height = 15,
  show_reasons = true,
  -- "{range}" is replaced by the range ("" for the working tree)
  generate_cmd = { "claude", "-p", "/diff-narrative {range}", "--permission-mode", "acceptEdits",
                   "--allowedTools", "Bash(git:*)", "Read", "Write" },
  keymaps = { next_group = "]g", prev_group = "[g", next_file = "]f", prev_file = "[f",
              select = "<CR>", toggle_fold = { "za", "<Tab>" }, toggle_narrative = "gn",
              toggle_reasons = "gr", refresh = "R", close = "q" },
}
```

Highlight groups (all `default` links, so you can override them): `StructDiffTitle`, `StructDiffGroup`, `StructDiffCount`, `StructDiffDir`, `StructDiffReason`, `StructDiffWhy`, `StructDiffCurrent`, `StructDiffAdded`, `StructDiffChanged`, `StructDiffRemoved`, `StructDiffFresh`, `StructDiffStale`, `StructDiffNone`.

## Tests

```sh
nvim --headless -l tests/run.lua
```
