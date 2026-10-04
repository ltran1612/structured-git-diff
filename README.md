# structdiff.nvim

A grouped view of your working-tree diff for Neovim, with an AI-written narrative of *why* each file changed. Written in Rust with [nvim-oxi](https://github.com/noib3/nvim-oxi).

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

- **Neovim 0.12.5.** The plugin binds Neovim's C API directly and only loads on Neovim versions it was checked against (see [Compatibility](#compatibility)).
- A Rust toolchain (`cargo`) to build it, and git.
- For narratives, [Claude Code](https://claude.com/claude-code).

## Install

lazy.nvim:

```lua
{
  "…/structdiff.nvim", -- or dir = "~/path/to/organizeddiffview"
  main = "structdiff",
  build = "./build.sh",
  cmd = { "StructDiff", "StructDiffClose", "StructDiffRefresh", "StructDiffNarrative", "StructDiffGenerate" },
  keys = { { "<leader>gv", "<cmd>StructDiff<cr>", desc = "StructDiff" } },
  opts = {},
}
```

`build.sh` runs `cargo build --release` and installs the library as `lua/structdiff.so`, where `require("structdiff")` finds it. `main` is needed because lazy.nvim only auto-detects Lua modules. After pulling changes, rebuild with `:Lazy build structdiff.nvim` and restart Neovim.

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

You can also run `/diff-narrative [range]` in any Claude Code session in the repo. The open view checks the narrative file every 300ms and reloads as soon as it changes.

If you quit Neovim while `:StructDiffGenerate` is running, the `claude` run keeps going and still writes the narrative. The next `:StructDiff` on that range picks it up.

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

Groups are an ordered list. Each pattern is a [Rust `regex`](https://docs.rs/regex/latest/regex/#syntax) (the usual PCRE-like syntax, without lookaround), matched anywhere in the repo-relative path, so anchor with `^` and `$`. The **first** matching group wins, and anything unmatched goes to `other_group`. Empty groups are hidden. Invalid patterns are reported and skipped.

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

Defaults: Tests, CI, Config/Build, Docs, Source, Other (see `default_groups` in `crates/core/src/group.rs`).

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

The fingerprint is a sha256 over each changed file's status, modes and content hashes (from `git diff --raw`, plus `git hash-object` for working-tree and untracked files), so it never builds the patch itself. Staging a change doesn't alter it; editing content does. If the diff changes after the narrative was written, the sidebar shows **narrative stale**. The old narrative stays visible until you regenerate it.

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

Options you don't set keep their defaults. Lists such as `groups` and `generate_cmd` replace the default list rather than merging with it.

Highlight groups (all `default` links, so you can override them): `StructDiffTitle`, `StructDiffGroup`, `StructDiffCount`, `StructDiffDir`, `StructDiffReason`, `StructDiffWhy`, `StructDiffCurrent`, `StructDiffAdded`, `StructDiffChanged`, `StructDiffRemoved`, `StructDiffFresh`, `StructDiffStale`, `StructDiffNone`.

## Layout

| Crate | What it is |
|---|---|
| `crates/core` (`structdiff-core`) | Everything that doesn't need Neovim: git and ranges, grouping, the narrative file contract, and the `Model` the view displays. Plain Rust, no Neovim dependency. |
| `crates/nvim` (`structdiff`) | The plugin: a `cdylib` loaded by Neovim. Layout, diff panes, sidebar, keymaps, commands, the narrative watcher and background generation. |
| `crates/nvim-tests` | Tests that run inside a real Neovim through nvim-oxi's test harness. |

`plugin/structdiff.lua` is a single `require("structdiff")`. Neovim can only load native modules through `require`, so that line is the whole Lua side.

## Tests

```sh
./check.sh
```

This runs strict clippy (`-D warnings`, which includes the ban on broken nvim-oxi bindings), the core tests, and the tests that run inside Neovim (they need the supported Neovim on `PATH`).

## Compatibility

nvim-oxi is pinned to an upstream commit with the `neovim-0-12` feature, which predates some 0.12.x changes. Checked against the v0.12.5 sources, three bindings don't match and the plugin avoids them:

| nvim-oxi binding | Problem on 0.12.5 | Replacement |
|---|---|---|
| `api::create_autocmd` | `Dict(create_autocmd)` gained a `buf` key, so the options struct is misaligned | no autocmds; the 300ms watcher timer also notices when the tab is closed |
| `api::set_hl` | `Dict(highlight)` was reordered and extended | `:highlight default link`, re-applied on every redraw (survives colorscheme changes) |
| `api::list_tabpages` | `nvim_list_tabpages` now takes an `Arena*`; calling it corrupts the heap | `tabpagenr('$')` |

Two guards keep this from happening again:

- **Exact version allowlist.** `TESTED_NEOVIM` in `crates/nvim/src/lib.rs` lists the exact versions checked (currently 0.12.5). These bindings broke within the 0.12 series, so a newer 0.12.x is rejected too. On any other version the plugin shows an error and does nothing. If you accept the risk, set `vim.g.structdiff_allow_untested_nvim = true` before it loads.
- **Compile-time ban.** `clippy.toml` lists the three bindings as `disallowed-methods`, and the plugin crate denies that lint, so `./check.sh` fails if anyone calls them.

The details are in the doc comment at the top of `crates/nvim/src/lib.rs`. Before supporting a new Neovim or nvim-oxi version, re-check every binding the plugin uses against that version's sources, then add it to `TESTED_NEOVIM`.
