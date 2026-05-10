# Backlog

Bugs and small tasks to address when convenient. Open an issue when you
pick one up.

## Bugs

### ~~`torii sync` fails with "corrupted loose reference file: FETCH_HEAD"~~ — fixed in 0.6.2

Discovered: 2026-05-05. Fixed: 2026-05-10.

Resolution: `core::pull` now stat's `FETCH_HEAD` after fetch and treats a
0-byte / missing file as "nothing to pull" (same outcome as
`is_up_to_date()`), instead of letting libgit2 abort on
`find_reference("FETCH_HEAD")`.

Original report:

After a fresh `torii remote link <platform> <namespace>/<repo>` followed
by `torii sync`, the command aborts with:

```
Error: Git error: corrupted loose reference file: FETCH_HEAD; class=Reference (4)
```

The `.git/FETCH_HEAD` file exists but is empty (0 bytes). Deleting it
does not fix the next `torii sync` call — it recreates the empty file
and fails the same way.

**Workaround:** for the very first push of a freshly created remote, fall
back to `git push -u origin main`. Subsequent operations work once a real
fetch has populated `FETCH_HEAD`.

**Hypothesis:** `torii sync` opens `.git/FETCH_HEAD` for read before the
first fetch has written it. Should either skip the read on a 0-byte file,
or perform the fetch first and only then parse `FETCH_HEAD`.

Reproducer:

```sh
mkdir x && cd x && torii init
echo hi > a && torii save -am "init"
torii remote create gitlab x --private --push   # or remote link to an
                                                # empty remote
torii sync                                      # -> reproduces
```
