---
name: diff-narrative
description: Explain why the uncommitted changes in a git repo were made, as a narrative tying the changed files together. Writes .structdiff/narrative.json for the structdiff.nvim viewer. Use when the user runs /diff-narrative or asks for a narrative or explanation of their working-tree diff.
---

# diff-narrative

Write a narrative of the working-tree changes (everything differing from HEAD: staged, unstaged and untracked). The narrative explains **why** each file changed and how the changes connect. Save it to `.structdiff/narrative.json` at the repo root. The structdiff.nvim viewer reads that file and shows it next to the diff.

## 1. Gather the change set

Run everything from the repo root (`git rev-parse --show-toplevel`).

- Read `.structdiff/groups.json` if it exists. The viewer writes it, and it has this shape:
  ```json
  {"version":1,"fingerprint":"<sha256>","groups":[{"name":"Source","files":[{"path":"lua/a.lua","status":"M"}]}]}
  ```
  Status is one of `M A D R C U`, or `?` for untracked. `old_path` is set on renames.
  Use exactly these group names and files. Copy `fingerprint` unchanged into your output.
- If the file is missing, list the changes yourself with `git status --porcelain` and group the files sensibly (Source, Tests, Docs, Config/Build, CI, Other). Omit `fingerprint` in that case.
- Read the diff with `git diff HEAD` (on a repo with no commits, use `git diff --cached` plus the files themselves). Read untracked files directly. For large diffs, start with `git diff HEAD --stat`, then read per file.
- Where a hunk's purpose is unclear, open the surrounding code. Callers, definitions and tests usually explain the change.

## 2. Work out the story

Don't just summarise each file. Find the **intent** and the **dependencies between files**:
- Which change is the root cause? For example, a new function, a changed interface, a bug fix, or a new requirement.
- Which changes follow from it? For example, updated callers, adjusted config, new tests that exercise it, docs that describe it.
- Which changes are unrelated to the main story? For example, drive-by refactors, formatting, version bumps. Say so plainly.
- Note anything that looks unfinished or risky: a TODO left behind, a debug print, a test that no longer covers a changed path.

Only claim what the code shows. If the reason for a change can't be inferred, say what it does and mark the reason as unclear. Do not invent one.

## 3. Write `.structdiff/narrative.json`

Create the `.structdiff/` directory if needed. Write exactly this shape:

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

## 4. Report

Reply with the overall story in 2-3 sentences, and mention that `:StructDiff` (or `R` in an open view) shows the full narrative. Don't paste the JSON.
