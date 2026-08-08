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

## Distribution

### Release artifacts stopped being published after v0.7.15

Found: 2026-08-08.

The `release` job in `.gitlab-ci.yml` uploads the three binaries to the
Generic Package Registry and then creates the GitLab release. Neither has
happened since `v0.7.15` (25-05-2026), while tags run to `v0.13.0`.

Measured against the API on 08-08-2026:

```
packages/generic/gitorii/v0.13.0/torii-linux-x86_64 -> 404
packages/generic/gitorii/v0.10.0/torii-linux-x86_64 -> 404
packages/generic/gitorii/v0.7.15/torii-linux-x86_64 -> 200

/releases -> 12 objects, newest v0.7.15
```

What this breaks, in the order a new user hits it:

- The README's first install method — the prebuilt binary, described as
  "recommended, no compiler needed" — 404s for the current version.
- `gitorii.com` offers the same download; its install pane points at the
  releases page, whose newest entry is five months old.
- `cargo install gitorii` still works, so the fallback is the path that
  can hit the rustc SIGSEGV the README documents. The escape hatch is the
  one that is broken.

Diagnose the job before anything downstream of it: whether the pipeline
runs at all on a stable tag (the `workflow.rules` regex), whether the
three build jobs still land their artifacts, and whether the self-hosted
runner is up. Everything in the next entry depends on this one.

### Publish to the AUR from the repository

Found: 2026-08-08. Blocked on the entry above.

`gitorii` on the AUR is at `0.7.11-1` while the crate is at `0.13.0`.
The recipe lives outside the project, which is why it fell behind, and
`yay -S gitorii` — recommended by both the README and the website —
installs a version from May.

Shape agreed on 08-08-2026:

- `packaging/aur/gitorii-bin/PKGBUILD` as the package most people want:
  it installs the binary CI already builds, so no compiler, no
  `RUST_MIN_STACK`, and no exposure to the rustc SIGSEGV.
- `packaging/aur/gitorii/PKGBUILD` kept current for people who prefer to
  build from source.
- A `publish-aur` stage on stable tags: regenerate `.SRCINFO` with
  `makepkg --printsrcinfo`, checksum the artifacts already uploaded to
  the package registry, and push to
  `ssh://aur@aur.archlinux.org/<package>.git`.

The AUR repository is then simply another push target of this project —
which is what `torii mirror` is for.

Needs an SSH key registered on the aur.archlinux.org account, stored as a
protected CI variable. Nothing here can be automated before the release
job publishes artifacts again.

The ROADMAP dropped a second AUR package in May to keep the maintenance
surface small for one developer. With the push automated, the second one
costs what the first one costs.
