---
name: diff-narrative
description: Explain why a git change set was made (uncommitted work, or a branch range like main...HEAD), as a narrative tying the changed files together. Writes .structdiff/narrative*.json for the structdiff.nvim viewer. Use when the user runs /diff-narrative [range] or asks for a narrative or explanation of their diff or branch.
---

# diff-narrative

Write a narrative of a change set. The narrative explains **why** each file changed and how the changes connect. The structdiff.nvim viewer reads it and shows it next to the diff.

Range argument: `$ARGUMENTS`

## 1. Work out the range and output file

Run everything from the repo root (`git rev-parse --show-toplevel`). The range argument (the text after `/diff-narrative`, possibly empty) picks what to explain:

| Argument | Compares | Diff command |
|---|---|---|
| *(empty)* | HEAD vs working tree, including staged, unstaged and untracked | `git diff HEAD` plus untracked files |
| `A` | A vs working tree, including untracked | `git diff A` plus untracked files |
| `A..B` | A vs B (committed only) | `git diff A B` |
| `A...B` | merge-base(A, B) vs B, i.e. what B adds on top of A (a PR) | `git diff A...B` |

An empty side of `..` or `...` means HEAD.

The output file is `.structdiff/narrative.json` when the argument is empty. Otherwise it is `.structdiff/narrative-<arg>.json`, where every character outside `A-Za-z0-9._-` is replaced by `_`. For example, `main...HEAD` gives `narrative-main...HEAD.json`, and `origin/main..HEAD~1` gives `narrative-origin_main..HEAD_1.json`.

## 2. Gather the change set

- Read `.structdiff/groups.json` if it exists. The viewer writes it, and it has this shape:
  ```json
  {"version":1,"fingerprint":"<sha256>","range":{"spec":"main...HEAD","base":"<sha>","target":"<sha or null>"},
   "output":".structdiff/narrative-main...HEAD.json",
   "groups":[{"name":"Source","files":[{"path":"lua/a.lua","status":"M"}]}]}
  ```
  Status is one of `M A D R C U`, or `?` for untracked. `old_path` is set on renames.
  - If `range.spec` equals your argument (both may be empty), groups.json is authoritative. Use exactly its group names and files, write to its `output` path, and copy `fingerprint` unchanged. Diff exactly `range.base` against `range.target` (`git diff <base> <target>`), or against the working tree when `target` is null.
  - If it describes a different range, ignore it.
- Without a matching groups.json, list the files yourself (`git diff --name-status` with the range from the table, plus `git ls-files --others --exclude-standard` when comparing against the working tree). Group them sensibly: Source, Tests, Docs, Config/Build, CI, Other. Omit `fingerprint`.
- For a commit range, also read `git log --format='%h %s%n%b' <base>..<target>`. Commit messages are evidence of intent, but check them against the code. In the narrative, prefer what the code shows.
- Read the diff. On a repo with no commits, use `git diff --cached` plus the files themselves. Read untracked files directly. For large diffs, start with `--stat`, then read per file. For commit ranges, read file contents with `git show <rev>:<path>`, not from the working tree, which may hold other, uncommitted edits.
- Where a hunk's purpose is unclear, open the surrounding code. Callers, definitions and tests usually explain the change.

## 3. Work out the story

Don't just summarise each file. Find the **intent** and the **dependencies between files**:
- Which change is the root cause? For example, a new function, a changed interface, a bug fix, or a new requirement.
- Which changes follow from it? For example, updated callers, adjusted config, new tests that exercise it, docs that describe it.
- Which changes are unrelated to the main story? For example, drive-by refactors, formatting, version bumps. Say so plainly.
- Note anything that looks unfinished or risky: a TODO left behind, a debug print, a test that no longer covers a changed path.

Only claim what the code shows. If the reason for a change can't be inferred, say what it does and mark the reason as unclear. Do not invent one.

## 4. Write the narrative file

Create the `.structdiff/` directory if needed. Write the output file from step 1 in exactly this shape:

```json
{
  "version": 1,
  "fingerprint": "<copied from groups.json, or omit>",
  "overall": "Markdown, 1-3 short paragraphs: the story of the change. Refer to files by path in backticks and in causal order (A adds X, so B calls it, so C tests it). End with a bullet list of loose ends or risks, if any.",
  "groups": {
    "<group name>": "1-3 sentences: what this group of changes does for the overall story."
  },
  "files": {
    "<repo-relative path>": "One line, at most 80 characters: why this file changed (not just what)."
  },
  "order": ["<path>", "..."]
}
```

Rules:
- Every changed file appears in `files` and in `order`, with paths exactly as in groups.json or git. For renames, use the new path.
- Every group name from groups.json appears in `groups`.
- `order` is the suggested reading order, root cause first. The viewer sorts files within each group by it.
- `files` entries start with a verb and give the reason: "adds backoff helper used by fetch retry", not "modified file".
- Write valid JSON. Escape newlines inside `overall` as `\n`.

## 5. Report

Reply with the overall story in 2-3 sentences, and mention that `:StructDiff <same range>` (or `R` in an open view) shows the full narrative. Don't paste the JSON.
