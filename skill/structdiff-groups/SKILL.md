---
name: structdiff-groups
description: Create or tune how structdiff.nvim groups changed files in a repository, by writing .structdiff.json (named regex groups plus a display order). Use when the user asks to set up, create, fix or improve structdiff grouping for a repo, or says files land in the wrong group or in "Other".
---

# structdiff-groups

Write a `.structdiff.json` for this repository so that structdiff's viewer groups changed files the way a reviewer would read them: one coherent group at a time ("API", "Migrations", "Frontend", "Tests") rather than one long list.

## How grouping works

Read this first. Most bad groupings come from getting one of these wrong.

- **The file:** `.structdiff.json` at the repo root, meant to be committed so everyone shares it:
  ```json
  {
    "groups": [
      { "name": "Migrations", "patterns": ["^db/migrate/"] },
      { "name": "Tests", "patterns": ["(^|/)tests?/", "_test\\.go$"] },
      { "name": "Backend", "patterns": ["^server/"] }
    ],
    "display_order": ["Backend", "Migrations", "Tests"]
  }
  ```
- **It replaces the defaults entirely.** Groups you leave out (tests, docs, config, CI) no longer exist, and their files fall into the fallback group, "Other". Include every kind of file this repo has.
- **Patterns** are Rust `regex` syntax (PCRE-like, **no lookaround or backreferences**), matched against the path relative to the repo root with `/` separators, such as `src/api/server.py`. They match anywhere in the path, so anchor with `^` and `$` and escape dots (`\.py$`). In JSON, backslashes are doubled (`"\\.py$"`).
- **First match wins:** each file goes to the first group (in `groups` order) that has a matching pattern. Put narrow groups (tests, generated code, migrations) before broad ones (source by file extension), or the broad group swallows them.
- **`display_order`** lists group names in the order a reviewer should read them. Groups it doesn't name come next in match order, then "Other". It's separate from match order: Tests usually has to *match* before Source but reads better *after* it. When a narrative exists, its reading order takes precedence in the viewer.
- **"Other"** is the fallback. Its name can't be changed here. A few odd files there (`.gitignore`, `LICENSE`) are fine; anything a reviewer cares about shouldn't be.

For reference, the built-in defaults are: Tests, CI, Config/Build, Docs, Source (matched in that order), displayed as Source, Tests, Docs, Config/Build, CI.

## 1. Survey the repo

Run from the repo root (`git rev-parse --show-toplevel`):

```sh
git ls-files | wc -l
git ls-files | cut -d/ -f1 | sort | uniq -c | sort -rn | head -30            # top-level folders (and root files)
git ls-files | grep / | cut -d/ -f1-2 | sort | uniq -c | sort -rn | head -40  # second level
git ls-files | sed -n 's/.*\.\([A-Za-z0-9]*\)$/\1/p' | sort | uniq -c | sort -rn | head -20
```

Look for:
- test conventions: folders (`tests/`, `test/`, `spec/`, `__tests__/`, `*-tests/`) and file names (`_test.go`, `.spec.ts`, `test_*.py`);
- separately reviewable areas: packages in a monorepo (`crates/*`, `packages/*`, `apps/*`, `services/*`), frontend vs backend, public API vs internals;
- migrations, schemas, generated or vendored code (`*.pb.go`, `vendor/`, `dist/`, snapshots), lockfiles;
- infrastructure (Dockerfile, Terraform, Kubernetes, CI workflows), docs, configuration.

## 2. Learn what changes together

Groups should be the units people review together. Recent history shows where changes concentrate:

```sh
git log --name-only --format= -n 300 | grep . | cut -d/ -f1-2 | sort | uniq -c | sort -rn | head -25
```

Areas that churn often and independently deserve their own group. Small areas that only ever change alongside another can share its group. If the natural split is ambiguous, such as by package versus by layer in a monorepo, briefly propose both and ask the user which matches how they review.

## 3. See the current grouping

If `.structdiff.json` already exists, it's your starting point: keep the user's groups unless asked to replace them, and fix what's wrong. Then see how the current grouping (or the defaults) classifies every file:

```sh
structdiff groups          # .structdiff.json, else the Neovim config recorded by the last export, else the defaults
structdiff groups --all    # every file, not just samples
```

Note what's misfiled, what's in Other, and which groups are too big to review in one go.

## 4. Draft and check

Aim for about 4 to 9 groups with short, reviewer-facing names. Write the draft to `.structdiff/groups-draft.json`, not to `.structdiff.json`. It's inside the repo, so agents that restrict writes outside the project can create it, and structdiff never counts `.structdiff/` as part of the repo. Check it:

```sh
structdiff groups --config .structdiff/groups-draft.json
```

The output starts with any `problem:` lines: invalid patterns, unknown keys (such as `"pattern"` for `"patterns"`), `display_order` names with no group, duplicate groups, a group named like the fallback. The exit code is 1 when there is any problem. Fix and re-run until:
- the exit code is 0, with no `problem:` lines;
- no group is reported as matching nothing, unless it's deliberately there for future files (say so in your report);
- Other holds only files you've deliberately left there;
- spot-checked files land in the group a reviewer would expect, especially ones near a boundary (tests inside source folders, config inside packages).

If `structdiff` isn't installed, check with Python instead. It classifies files the same way, but Python's `re` isn't Rust's `regex`: it also accepts lookaround and backreferences (which structdiff rejects), so keep to the plain syntax described above.

```sh
python3 - .structdiff/groups-draft.json <<'EOF'
import json, re, subprocess, sys
cfg = json.load(open(sys.argv[1], encoding="utf-8-sig"))
other = "Other"
groups = [(g["name"], [re.compile(p) for p in g["patterns"]]) for g in cfg["groups"]]
names = [n for n, _ in groups]
display = cfg.get("display_order", [])
raw = subprocess.run(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
                     capture_output=True).stdout.decode("utf-8", "replace")
files = sorted({p for p in raw.split("\0") if p and p != ".structdiff" and not p.startswith(".structdiff/")})
found = {}
for f in files:
    found.setdefault(next((n for n, ps in groups if any(p.search(f) for p in ps)), other), []).append(f)
for n in display:
    if n != other and n not in names:
        print(f"problem: display_order names {n!r}, but no group has that name")
print("match nothing:", ", ".join(n for n in names if n not in found) or "none")
shown = [n for n in display if n in found]
shown += [n for n in names + [other] if n in found and n not in shown]
for n in shown:
    print(f"\n{n} ({len(found[n])})", *(found[n] if n == other else found[n][:5]), sep="\n  ")
EOF
```

## 5. Write it

Write the final grouping to `.structdiff.json` at the repo root, pretty-printed with 2-space indentation, and delete `.structdiff/groups-draft.json`. If `.structdiff/` didn't exist before you started, delete the folder too: without `structdiff` installed, nothing has excluded it from git. Don't commit it unless the user asks; suggest committing it so the team shares the grouping.

## 6. Report

Reply with each group, its file count and one line on what it holds, in display order. Then list what you deliberately left in Other, and any judgment calls (such as grouping by package rather than by layer). Mention that an open structdiff view picks the change up with `R`, or `:StructDiff` to reopen. Don't paste the JSON unless asked.
