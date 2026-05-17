# Bug: `torii save` falls back to "Torii User" instead of using configured identity

> **STATUS: FIXED in v0.7.3** (commit landed 2026-05-17). Implementation
> followed the report's recommended Option 2: a single
> `crate::core::resolve_signature(&repo)` function looks at the torii
> config first (`~/.config/torii/config.toml [user]`), then the git
> config chain as a fallback, then errors out with a clear fix-it
> message. **No silent "Torii User" placeholder anywhere.** All seven
> call sites (`save`, `tag`, `cherry-pick`, `revert`, `rebase`,
> `stash`, TUI commit) now route through it. Empty strings are treated
> as "not configured" so `torii config set user.name ""` triggers the
> same fix-it error rather than producing an invalid commit.

## Severity

Medium. Produces commits with bogus author/committer information,
breaking attribution chains, signed-commit verification, and any
downstream consumer that filters by author email.

## Symptoms

Commits created via `torii save` are attributed to:

```
Author: Torii User <torii@example.com>
```

(or similar placeholder) even though `torii config list --local`
shows the user's real identity:

```
⚙️  Local Configuration:

  user.name = Pasqual Peñalver
  user.email = paski@paski.dev
  ...
```

## Reproduction

```sh
# In a fresh repo with no [user] section in .git/config:
torii init demo
cd demo
torii config set user.name "Real Name"
torii config set user.email "real@example.com"

torii config list --local | grep user.
# user.name  = Real Name
# user.email = real@example.com

echo hello > file
torii save -am "initial"

torii log -n 1
# Author: Torii User <torii@example.com>   ← BUG
```

## Root cause

Torii stores user identity in its own TOML config (probably
`torii.toml` or per-repo `.torii/config`). `torii config list`
reads from there and reports the right values.

At commit time, however, `torii save` calls into libgit2 (`git2-rs`)
to create the commit object. The path used probably resembles:

```rust
let sig = repo.signature()?;            // ← reads .git/config only
repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)?;
```

`git2::Repository::signature()` only consults `git`'s config chain
(`.git/config` → `~/.gitconfig` → `/etc/gitconfig`). It does **not**
know about torii's TOML config. When none of those have `[user]
name = ...` / `email = ...`, libgit2 returns a placeholder signature
or errors out, and somewhere along the way torii substitutes
`"Torii User" <torii@example.com>` as a hardcoded fallback.

Confirmed empirically by adding a `[user]` section to a repo's
`.git/config` and re-running `torii save --amend`: the resulting
commit then carries the correct author.

## Workaround for users

Add a `[user]` section to each repo's `.git/config` manually:

```ini
[user]
    name = Real Name
    email = real@example.com
```

Or globally in `~/.gitconfig`. After that, `torii save` produces
correct authorship. Existing commits made under the wrong identity
can be amended with `torii save --amend -m "<same message>"` once
the config is fixed (single commits) or rewritten with
`torii history rebase` (history with multiple bad commits).

## Suggested fixes

Two viable approaches, both worth doing in some form:

### Option 1 — Mirror torii config into `.git/config`

When `torii config set user.name <value>` (or `user.email`) is
called, also write the corresponding key to the repo's
`.git/config` under `[user]`. Keeps libgit2 happy and makes
`git`-aware tools (IDEs, hooks, `gh`, etc.) see the same identity.

Pros: minimal change in the commit code path; one source of truth
visible to both `torii` and `git`.

Cons: requires writing through libgit2 / a manual `.git/config`
parser; needs to handle the case where the user edits one side
manually and the two diverge.

### Option 2 — Pass identity explicitly to `Repository::commit()`

Construct the `Signature` from torii's own config rather than
relying on `repo.signature()`:

```rust
let cfg = torii_config::load_for_repo(&repo)?;
let name = cfg.user.name.as_deref().ok_or(MissingIdentity)?;
let email = cfg.user.email.as_deref().ok_or(MissingIdentity)?;
let sig = git2::Signature::now(name, email)?;
repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)?;
```

This bypasses libgit2's config lookup entirely and makes torii's
own config the single source of truth.

Pros: clean separation — `torii` identity is independent of git
identity. No `.git/config` writes. Behaves correctly even if
`.git/config` is broken or readonly.

Cons: `git log` viewed via plain `git` outside torii will still
show whatever identity is in `.git/config` for *subsequent*
commits made with plain `git` — but those aren't torii's
problem. If the user wants visual consistency across tools, they
still need option 1.

### Recommendation

Implement option 2 immediately (eliminates the fallback to "Torii
User" — that's the bug). Layer option 1 on top later as a
convenience for users who also run plain `git`.

Also: if torii has no identity configured (neither in its own
config nor in `.git/config`), it should **error out** instead of
silently substituting a placeholder. Bogus commits are worse than
a fail-fast error that prompts the user to set their identity.

## Tests to add

1. `torii save -am ...` with `[user]` in `.git/config` only → author
   matches `.git/config`.
2. `torii save -am ...` with torii TOML config only (no `.git/config
   [user]`) → author matches torii config. **This case is the bug
   today.**
3. `torii save -am ...` with both, values matching → author matches.
4. `torii save -am ...` with both, values differing → behavior must
   be documented and deterministic (torii config wins, by option 2).
5. `torii save -am ...` with no identity configured anywhere →
   exit non-zero with a clear error message; do not write a commit.

## Related context

- Observed in torii v0.6.7 (gitlab/github tagged, not yet on
  crates.io).
- The "Torii User" fallback string is likely hardcoded; grep the
  source for that literal to locate the substitution site.
- libgit2's signature lookup chain:
  <https://libgit2.org/libgit2/#HEAD/group/signature/git_signature_default>
