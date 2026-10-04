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
- **Narrative:** the agent that made the changes writes `.structdiff/narrative.json` (with the `diff-narrative` skill), and the view shows it:
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
  "ltran1612/structured-git-diff", -- or dir = "/path/to/a/local/clone"
  name = "structdiff.nvim",
  main = "structdiff",
  build = "./build.sh",
  cmd = { "StructDiff", "StructDiffClose", "StructDiffRefresh", "StructDiffNarrative", "StructDiffGenerate", "StructDiffCancel" },
  keys = { { "<leader>gv", "<cmd>StructDiff<cr>", desc = "StructDiff" } },
  opts = {},
}
```

`build.sh` runs `cargo build --release` and:
- installs the plugin as `lua/structdiff.so`, where `require("structdiff")` finds it;
- installs the `structdiff` command (used by the skill) into `~/.local/bin`, or into `$STRUCTDIFF_BIN_DIR` if set. That directory must be on your `PATH`.

`main` is needed because lazy.nvim only auto-detects Lua modules. `name` keeps the plugin's name (and its directory) as `structdiff.nvim` instead of the repo name. `:Lazy update` rebuilds automatically; to rebuild by hand, run `:Lazy build structdiff.nvim`, then restart Neovim.

### The narrative skill

`skill/diff-narrative/SKILL.md` is a plain agent skill. Claude Code, Codex and GitHub Copilot CLI all load that format, so the same file works in each. `install-skill.sh` copies it into each agent's skills folder:

```sh
./install-skill.sh                 # every agent CLI found on PATH
./install-skill.sh claude codex    # or just these
./install-skill.sh --dry-run       # preview
```

| Agent | Installed to |
|---|---|
| Claude Code | `~/.claude/skills/diff-narrative` (or `$CLAUDE_CONFIG_DIR/skills`) |
| Codex | `~/.agents/skills/diff-narrative` |
| Copilot CLI | `~/.copilot/skills/diff-narrative` (or `$COPILOT_HOME/skills`) |

It copies rather than links, so you can run it from any checkout (with lazy.nvim, that's `~/.local/share/nvim/lazy/structdiff.nvim`) and then delete or move the checkout. Re-run it to update. A previous install of this skill is replaced; anything else at that path is moved to a `.bak` folder first. Restart running agent sessions afterwards.

The skill works without the plugin. With the `structdiff` command on `PATH` (`./build.sh` installs it), its narratives match what the viewer computes and show as up to date. Without it they show as unverified.

## Usage

| Command | |
|---|---|
| `:StructDiff [range]` | Open the view (or refresh it if it's already open with the same range) |
| `:StructDiffGenerate [range]` | Fallback: have an agent CLI (Claude, Codex or Copilot) write the narrative for that range, and reload when it finishes |
| `:StructDiffCancel` | Stop a running `:StructDiffGenerate` |
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

## Where narratives come from

The viewer only reads `.structdiff/narrative*.json`; it never talks to an AI itself. The open view checks the file every 300ms and reloads as soon as it changes.

**Best: the agent that made the changes writes it.** When an agent finishes a change, it runs `/diff-narrative [range]` (the skill in `skill/diff-narrative`). That agent knows the request, the decisions and the alternatives it rejected, which no one reading the diff later can recover.

**Fallback: `:StructDiffGenerate`** is for changes nobody narrated, such as your own edits, someone else's branch, or a session that's gone. It starts a fresh agent that reconstructs the "why" from the diff, the commit messages and the surrounding code.

Because that might be someone else's branch, the agent is treated as untrusted: it can read the checkout, but it can't run commands or write anything. structdiff gathers the evidence itself (the changed files, the commit messages, the diff up to about 90 KB, untracked files' contents) and puts it in the prompt, marked as data rather than instructions. The agent replies with the narrative as JSON. structdiff then checks the reply, drops anything about files outside the change set, sets the fingerprint itself and writes the file. A prompt injection in the branch can at worst produce a misleading narrative. Pick the agent CLI with `generator`:

| `generator` | Runs | What the agent can do |
|---|---|---|
| `"claude"` (default) | `claude -p --restricted --strict-mcp-config --tools Read,Grep,Glob --permission-mode dontAsk` | read files only. `--restricted` also ignores the checkout's settings files, so a branch's `.claude/settings.json` hooks don't run, and `--strict-mcp-config` skips its `.mcp.json` servers |
| `"codex"` | `codex exec --sandbox read-only --ephemeral` | run commands in a read-only sandbox with no network |
| `"copilot"` | `copilot -p -s --no-ask-user` | nothing that needs approval (no tool grants) |

To run something else, set `generate_cmd` to a full command; it overrides `generator`. In it, `{prompt}` becomes the prompt above, `{range}` the range, and `{output}` the narrative file's path. The command should print the narrative JSON. A command that writes `{output}` itself also works: it then gets `.structdiff/groups.json` as for the skill. Either way, it runs with your permissions, so only use commands you trust.

`:StructDiffGenerate` gives up after `generate_timeout` seconds (default 600), and `:StructDiffCancel` stops it sooner. Either way the command and anything it started are stopped. On Linux it's also stopped if you quit Neovim; on other systems it keeps running, and the next `:StructDiff` on that range picks up whatever it writes.

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

Default match order: Tests, CI, Config/Build, Docs, Source, Other (see `default_groups` in `crates/core/src/group.rs`). Tests comes before Source so `foo_test.go` lands in Tests.

**Per-repo groups:** put a committable `.structdiff.json` at the repo root:

```json
{ "groups": [ { "name": "Migrations", "patterns": ["^db/migrate/"] } ], "display_order": ["Migrations"] }
```

**Display order is separate from match order.** `display_order` lists group names in the order they are shown (default: Source, Tests, Docs, Config/Build, CI). Groups it doesn't name follow in match order, and `other_group` comes last unless named. `display_order` in `.structdiff.json` is optional.

Once a narrative exists, its reading order wins: the group holding the story's first file (the root cause) moves to the top, and files within each group follow the narrative.

## The narrative contract

Everything lives in `<repo>/.structdiff/`. Viewing a diff writes nothing to your repo. The directory is only created by `:StructDiffGenerate` or `structdiff export`, and both add it to `.git/info/exclude`, so it never shows up in `git status`.

- `groups.json` (written by `structdiff export`, which the skill runs first, and by `:StructDiffGenerate`): the range (`spec`, resolved `base`/`target` SHAs), the `output` file name, the groups and their files, a `fingerprint` of the change set, and the `grouping` (patterns) that produced the groups.
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

## The `structdiff` command

```sh
structdiff export [RANGE]   # write .structdiff/groups.json for RANGE and print its path
```

`RANGE` works as in `:StructDiff`. The command computes the change set exactly as the viewer does, so a narrative written from any Claude Code session (`/diff-narrative main...HEAD`) shows as up to date in Neovim. Groups come from the repo's `.structdiff.json` if present, otherwise from the grouping recorded by the last export (so your Neovim `groups` config carries over), otherwise the defaults.

## Configuration

```lua
opts = {
  sidebar_width = 40,
  narrative_height = 15,
  show_reasons = true,
  display_order = { "Source", "Tests", "Docs", "Config/Build", "CI" },
  generator = "claude",    -- fallback agent: "claude", "codex" or "copilot"
  generate_cmd = {},       -- optional full override; {prompt}, {range}, {output} are filled in
  generate_timeout = 600,  -- seconds; 0 waits forever
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
| `crates/core` (`structdiff-core`) | Everything that doesn't need Neovim: git and ranges, grouping, the narrative file contract, and the `Model` the view displays. Plain Rust; loading a model only reads. |
| `crates/cli` (`structdiff-cli`) | The `structdiff` command, for the skill. |
| `crates/nvim` (`structdiff`) | The plugin: a `cdylib` loaded by Neovim. Git work runs on background threads; Neovim's main loop only draws. |
| `crates/nvim-tests` | Tests that run inside a real Neovim through nvim-oxi's test harness. |

Inside `crates/nvim`, dependencies only point down: `register` (version guard, commands, module table) → `actions` / `generate` / `watch` (the controller and keymap wiring) → `keys` → `view` (draws only; takes the config as an argument and owns none) → the leaves `state`, `config`, `hl`, `ui`, `bg`.

`plugin/structdiff.lua` is a single `require("structdiff")`. Neovim can only load native modules through `require`, so that line is the whole Lua side.

## Tests

```sh
./check.sh
```

This runs strict clippy (`-D warnings`, which includes the ban on broken nvim-oxi bindings), the core and CLI tests, and the tests that run inside Neovim (they need the supported Neovim on `PATH`).

## Compatibility

nvim-oxi is pinned to an upstream commit with the `neovim-0-12` feature, which predates some 0.12.x changes. Checked against the v0.12.5 sources, three bindings don't match, and one more is unsafe to use directly. The plugin avoids all four:

| nvim-oxi binding | Problem on 0.12.5 | Replacement |
|---|---|---|
| `api::create_autocmd` | `Dict(create_autocmd)` gained a `buf` key, so the options struct is misaligned | no autocmds; the 300ms watcher timer also notices when the tab is closed |
| `api::set_hl` | `Dict(highlight)` was reordered and extended | `:highlight default link`, re-applied on every redraw (survives colorscheme changes) |
| `api::list_tabpages` | `nvim_list_tabpages` now takes an `Arena*`; calling it corrupts the heap | `tabpagenr('$')` |
| `api::Window::call` | not an ABI issue: an `Err` returned by its closure is raised as a Lua error through an `extern "C"` frame, which aborts Neovim | `ui::in_win`, which catches the error inside and returns it normally |

Two guards keep this from happening again:

- **Exact version allowlist.** `TESTED_NEOVIM` in `crates/nvim/src/register.rs` lists the exact versions checked (currently 0.12.5). These bindings broke within the 0.12 series, so a newer 0.12.x is rejected too. On any other version the plugin shows an error and does nothing. If you accept the risk, set `vim.g.structdiff_allow_untested_nvim = true` before it loads.
- **Compile-time ban.** `clippy.toml` lists the four bindings as `disallowed-methods`, and the plugin crate denies that lint, so `./check.sh` fails if anyone calls them.

The details are in the doc comment at the top of `crates/nvim/src/lib.rs`. Before supporting a new Neovim or nvim-oxi version, re-check every binding the plugin uses against that version's sources, then add it to `TESTED_NEOVIM`.
