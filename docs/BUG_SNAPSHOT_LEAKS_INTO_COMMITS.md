# Bug: snapshot files from history-rewriting commands leak into the next commit

> **STATUS: FIXED in v0.7.7** (2026-05-18). Two changes landed:
>
> 1. **Snapshots moved out of the working tree** (the report's
>    recommended fix #1). `SnapshotManager::new` now writes to
>    `.git/torii/snapshots/<id>/` — inside the gitdir, where `git add`
>    never reaches. The location is resolved through
>    `git2::Repository::discover().path()` so it works for worktrees
>    and submodules too.
> 2. **`torii save -a` skips `.torii/`** (report's fix #3) as a
>    defense-in-depth measure so anything else under `.torii/`
>    (`config.json`, `mirrors.json`) stays out of `-a` too. Explicit
>    paths still work: `torii save .torii/config.json -m "..."`.
>
> First-run migration: when 0.7.7 finds an old `.torii/snapshots/`
> directory in a working tree, it moves every snapshot inside to the
> new location and removes the now-empty `.torii/snapshots/` (and the
> `.torii/` parent if it's empty too). Idempotent, prints a one-line
> notice; cross-FS rename failures fall back to a recursive copy.
>
> Fix #4 (warn on suspicious commit size) was deferred. The root
> cause is gone; a generic large-commit warning is general hardening
> worth doing on its own merit and out of scope for the bug.

## Severity

**High.** Silently inflates the next commit by hundreds of MB, can
break pushes (zlib aborts, server-side size rejections), and pollutes
the permanent history with a copy of `.git`. If the push *does*
succeed, the repository is permanently bloated and the maintainer
likely doesn't notice until much later.

Hit in the wild on `syrakon/tramuntana` (Servo fork): a single
`torii history reauthor` + `torii save -am ...` sequence produced a
commit carrying **10,269 unrelated objects (~681 MB)**. Push died
with `failed to finish zlib inflation: stream aborted prematurely`.

## Symptoms

After running any history-rewriting command (`torii history reauthor`,
likely also `rebase`, `mailmap apply` — anything that takes a safety
snapshot), the very next `torii save -am` produces a commit that:

- Adds hundreds-to-thousands of files under
  `.torii/snapshots/<timestamp>/git_backup/`.
- Includes a full `.git` clone inside the working tree: `FETCH_HEAD`,
  `packed-refs`, every `refs/remotes/upstream/*`, `index` (often
  multi-MB binary), the reflog, hooks, config.
- `git push --dry-run` reports just "1 commit ahead, fast-forward",
  hiding the size of the payload (dry-run lists *refs*, not the
  packfile contents).
- The actual push transfers a packfile that's orders of magnitude
  larger than what the commits look like in `torii log --stat`.

Concrete numbers from the tramuntana case:

| Metric                             | Value         |
| ---------------------------------- | ------------- |
| commits ahead of `origin/main`     | 1             |
| `.gitlab-ci.yml` lines changed     | 4             |
| objects in packfile                | 10,269        |
| packfile size on the wire          | ~681 MB       |
| outcome                            | push aborted  |

## Root cause

`torii history reauthor` (and other destructive operations) write a
safety snapshot to **`.torii/snapshots/<id>/git_backup/`** inside the
working tree. The snapshot is a full `.git` directory copy: refs,
objects directory metadata, index, logs, the lot.

`.torii/` is **not** added to `.gitignore` by torii — neither by
`torii init` nor by the command that creates the snapshot. So the
working tree now contains thousands of untracked files that look like
real project content.

When the user then runs `torii save -am "<msg>"`, the `-a` flag stages
**everything** including `.torii/`, and the commit silently absorbs
the entire snapshot. There is no warning, no prompt, no diff summary
that flags the size jump.

The combination of:

1. Snapshot written *inside* the working tree (instead of
   `.git/torii/snapshots/` or `$XDG_DATA_HOME/torii/`),
2. No automatic `.gitignore` entry,
3. `torii save -a` blindly staging everything new,

is what turns a "safety net" feature into a footgun.

## Reproduction

```sh
# Any repo with a tracked branch and pushable remote.
torii history reauthor --old old@example.com --new "New <new@example.com>"
# Snapshot now lives at .torii/snapshots/<timestamp>/git_backup/

# Make a tiny change.
echo "# hello" >> README.md

# Commit with -a.
torii save -am "docs: hello"

# Inspect:
torii log -1 --stat   # appears small
git rev-list --objects HEAD ^origin/main | wc -l   # actually huge
```

## Why `git push --dry-run` misled us during debugging

```
$ git push --dry-run
   942965ed5a..87da43182  main -> main
```

Reads as "1 commit, fast-forward, nothing weird". But the commit's
*tree* references the snapshot blobs, so the packfile that `git push`
has to assemble pulls in every blob the destination doesn't already
have — 10k+ objects. Dry-run only enumerates refs, never the
packfile, so the discrepancy is invisible until the real push starts.

## Recommended fix

In rough order of correctness:

1. **Move snapshots out of the working tree.** Write to
   `.git/torii/snapshots/<id>/` (inside the git dir, never traversed
   by `git add`) or to `$XDG_DATA_HOME/torii/snapshots/<repo-id>/<id>/`
   (outside the repo entirely). This is the canonical convention for
   tooling state that must never enter a commit. Fixes the root
   cause; no `.gitignore` magic needed.

2. **If snapshots must stay in the working tree, manage `.gitignore`
   automatically.** When `torii history reauthor` (or any
   snapshot-emitting command) runs, ensure `.gitignore` contains a
   `/.torii/` entry. If `.gitignore` doesn't exist or doesn't have
   the entry, append it (and commit it as part of the same operation,
   or print a warning).

3. **Make `torii save -a` refuse to stage `.torii/`.** Independent of
   `.gitignore`, treat `.torii/` as a reserved internal directory
   that `-a` never touches, the same way `git add .` skips `.git/`.

4. **Warn on suspicious commit size.** If `torii save` is about to
   create a commit whose diff is >50 MB or >500 files, print a yellow
   warning with the path summary and require `--force-large` (or
   confirm interactively). This wouldn't have prevented the bug but
   would have surfaced it before the push attempt.

Fix #1 is the right answer. Fixes #2 and #3 are defensive backstops
in case #1 isn't feasible (e.g. someone wants snapshots inspectable
from a file manager). Fix #4 is a general hardening worth doing
regardless.

## Workaround (downstream — what to do until this is fixed)

Add this to the repo's `.gitignore` **before** running any
history-rewriting torii command:

```gitignore
# Torii safety snapshots (reauthor, rebase, mailmap, ...): contain a
# full .git copy, must never enter a commit.
/.torii/
```

If the leak already happened and was committed but not pushed:

```sh
torii save --reset HEAD~1 --reset-mode soft
torii save --unstage .torii/
# add .torii/ to .gitignore, then:
torii save -am "<original message>" <real-files>
```

If it was already pushed: re-do the above and then
`torii sync --push --force` (assumes the branch isn't protected
against force-push, or that protections can be lifted temporarily).

## Cross-check on gitorii itself

Worth verifying whether the same pattern has hit any gitorii commit
in the past. Quick check:

```sh
cd /path/to/gitorii
git log --all --diff-filter=A --name-only --pretty=format: \
  | grep -E '\.torii/snapshots/' | head
# Any output = at least one historical commit absorbed a snapshot.
```

If gitorii's own history has clean commits, this confirms the bug is
purely environmental (the snapshot wasn't there during the gitorii
commit because the maintainer hadn't run reauthor recently); the fix
is still required because *future* users will hit it.
