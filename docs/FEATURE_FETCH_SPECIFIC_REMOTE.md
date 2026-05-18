# Feature request: `torii sync --fetch <remote>`

## Why this matters

`torii sync --fetch` today fetches only the remote tracked by the
current branch (typically `origin`). For repositories that follow the
**fork workflow** — where there's a separate upstream remote for
read-only sync — there is no torii command to fetch from anything
other than the configured tracking remote.

Discovered 2026-05-18 while setting up a Servo upstream sync on the
tramuntana fork (gitlab.com/syrakon/tramuntana). The intended layout:

```
remotes:
  origin   → gitlab.com/syrakon/tramuntana   (our work, R/W)
  upstream → github.com/servo/servo          (mirror upstream, RO)

branches:
  main           → tracks origin/main, has our patches
  servo-upstream → tracks upstream/main, pure mirror
```

To populate `servo-upstream`, we need to fetch `upstream` first. There
is currently no way to do that through torii without bypassing it
with `git fetch upstream` (against the gitorii-skill rule that forbids
direct git for VCS ops).

## Current behaviour

```sh
torii sync --fetch
```

Output: `✅ Fetched from remote` (only origin). Adding a `[remote
"upstream"]` section to `.git/config` doesn't change anything — torii
still fetches whichever remote the current branch's `branch.<name>.remote`
config points to, ignoring others.

`torii sync --help` confirms no `<remote>` argument is accepted:

```
Options:
  -p, --pull        Pull only
  -P, --push        Push only
  -f, --force       Force push
      --fetch       Fetch remote refs without merging
      ...
```

`torii remote create` exists but is for **creating a new remote
repository on a hosting platform** (GitHub / GitLab / Codeberg) — not
for adding an entry to local `.git/config` to point at an existing
external upstream.

## Proposal — extend `torii sync --fetch`

**Decision (user, 2026-05-18):** extend the existing `sync` command
rather than add a new top-level `fetch`. Keeps the CLI surface
small; the user already reaches for `sync` for every remote
operation.

```sh
torii sync --fetch                       # default: tracking remote (current behaviour)
torii sync --fetch upstream              # explicit remote
torii sync --fetch --all                 # every configured remote
```

`<remote>` is a positional argument; mutually exclusive with `--all`.
If `--fetch` is not present, the positional argument retains its
current "integrate this branch" meaning, so the change is purely
additive and doesn't break any existing call shape.

Rejected: dedicated `torii fetch` subcommand. More discoverable but
adds a top-level command for one operation the user already
associates with `sync`.

## Also missing: adding remote pointers locally

Companion gap: there's no way today to add a `[remote "name"]` entry
to `.git/config` through torii. We worked around it by editing
`.git/config` directly:

```ini
[remote "upstream"]
    url = https://github.com/servo/servo.git
    fetch = +refs/heads/*:refs/remotes/upstream/*
    tagopt = --no-tags
```

A `torii remote add <name> <url>` (separate from
`torii remote create`) would close this gap without overlapping the
create-on-platform semantics. Suggestion:

```sh
torii remote add upstream https://github.com/servo/servo.git
torii remote add upstream github servo/servo    # platform shorthand
torii remote remove upstream
torii remote list                                # already exists?
```

## Suggested implementation notes

- libgit2 (via `git2-rs`) exposes
  `Remote::download` /
  `Repository::find_remote("upstream")?.fetch(&[...], None, None)`.
  No need to call out to the `git` CLI.
- For `--all`, iterate `Repository::remotes()?` and fetch each.
  Print one line per remote with the result so failures don't go
  silent.
- Respect `tagopt = --no-tags` from `.git/config` (`FetchOptions::download_tags`).
- Default refspec is whatever's in the remote config; if missing,
  use `+refs/heads/*:refs/remotes/<name>/*`.

## Tests to add

1. `torii sync --fetch <remote>` with a configured remote → fetches
   it, creates `refs/remotes/<remote>/*`. Doesn't touch other
   remotes.
2. `torii sync --fetch` (no positional arg) → same as today
   (tracking remote of current branch).
3. `torii sync --fetch <remote>` with a non-existent remote → exit
   non-zero with a clear error (e.g. "no remote 'foo' configured;
   `torii config list --local` shows the configured remotes").
4. `torii sync --fetch --all` → fetches every `[remote "*"]` in
   `.git/config`, reports per-remote status, exits zero only if all
   succeeded.
5. `torii sync --fetch upstream --all` → conflict; reject with
   `--all and explicit remote are mutually exclusive`.
6. `torii remote add <name> <url>` → writes the entry, idempotent
   on second call if URL matches, errors if URL differs.

## Why now

Without this, the fork workflow for tramuntana (and any future fork
that wants periodic upstream sync) requires either editing
`.git/config` by hand and reaching for raw `git fetch upstream`, or
giving up on torii for that one operation. Both break the
gitorii-skill invariant of "every VCS operation goes through torii."

## Related context

- Observed in torii v0.7.3 (current rumb dev environment).
- Doesn't block anything in the gitorii repo itself — it surfaced on
  tramuntana, but the same gap exists for any fork. Likely affects
  anyone wanting to mirror Linux kernel, Firefox, or similar large
  upstream into a personal fork.
- Companion to BUG_COMMIT_AUTHOR_FALLBACK (also exposed by the
  tramuntana / rumb work, fixed in v0.7.3).
