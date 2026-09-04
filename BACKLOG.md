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

Diagnosed 04-09-2026: **the self-hosted runner does not exist.**
`torii runner list` returns a hundred runners for this project and every
one of them is `instance_type`, gitlab.com's shared pool. There is no
project-level runner and nothing tagged `gitorii`. The host matches: no
`gitlab-runner` package, no binary on PATH, no service, no
`~/.gitlab-runner/config.toml`, no container.

That is the whole of it. The three `build-*` jobs carry `tags: [gitorii]`
and have had nowhere to run, so no artifacts, no uploads, and the 404s
above.

To fix: `pacman -S gitlab-runner` — it is in `extra`, the AUR is not
needed — then `torii runner register --executor shell --tags gitorii`.
Shell executor rather than a container, because `.build_linux_self` uses
the host's `cargo`, its `rustup` targets and its cross-compilers
(`aarch64-linux-gnu-gcc`, `mingw-w64-gcc`, `nasm`). One wrinkle to settle
during the install: the package's service runs as the `gitlab-runner`
system user, which has no rustup toolchains, so the agent has to run as a
user that does or the job dies on its own `which cargo` guard.

The legacy registration-token path still works here — the project API
returned `runners_token` when asked on 04-09-2026 — so `torii runner
register` is a usable route once the binary is installed.

### Publish to the AUR from the repository

Found: 2026-08-08. Recipes landed 04-09-2026; the `-bin` half is still
blocked on the entry above.

`gitorii` on the AUR is at `0.7.11-1` while the crate is at `0.15.0`.
The recipe lived outside the project, which is why it fell behind, and
`yay -S gitorii` — recommended by both the README and the website —
installs a version from May.

The shape agreed on 08-08-2026 now exists in `packaging/aur/`, which is
the upstream for both AUR repositories:

- `gitorii-bin/PKGBUILD` — the package most people want: it installs the
  binary CI builds, so no compiler and no rust toolchain.
- `gitorii/PKGBUILD` — build from source, from the GitLab tag tarball.
- `update.sh` — rewrites `pkgver`, recomputes every checksum and
  regenerates `.SRCINFO`.
- `publish-aur-src` and `publish-aur-bin` in `.gitlab-ci.yml` — one job
  per package: each runs `update.sh` for its own recipe and pushes to
  `ssh://aur@aur.archlinux.org/<package>.git`.

`torii mirror` is not the tool for this push after all: it only takes
`github`, `gitlab`, `bitbucket` and `codeberg`, not an arbitrary SSH
remote. The jobs use plain git.

Split in two on 04-09-2026. The first draft was a single job on the
self-hosted runner, reasoning that it was the one machine in the pipeline
with `makepkg`. That was wrong twice: there is no such machine, and the
job never needed one. `update.sh` downloads sources, hashes them and
regenerates `.SRCINFO` — it compiles nothing. Both jobs now run in
`archlinux:base-devel` pinned by digest, so publishing to the AUR no
longer waits on the runner. Only `-bin` still does, through `release`.

Checked in that image on 04-09-2026: `makepkg` does refuse to run as
root, which is why the jobs create a `build` user and hand it
`packaging/aur`; `update.sh 0.15.0 gitorii` runs clean and leaves the
recipe byte-identical, re-confirming the recorded tarball checksum;
`update.sh 0.15.0 gitorii-bin` exits 1 on a 404 for `torii-linux-x86_64`,
which is exactly the loud failure the zero checksums exist to produce.
GitLab's own `ci/lint` accepts the file.

What is left:

- **An SSH key on the aur.archlinux.org account, as a file-type,
  protected CI variable named `AUR_SSH_PRIVATE_KEY`, and the tag pattern
  `v*` protected to go with it.** Checked on 04-09-2026: the project has
  no protected tag at all, only a protected `main`. A protected variable
  is handed out only on a protected ref, so marking the key protected
  without protecting `v*` first hides it from the exact pipelines that
  need it. Either half missing and neither `publish-aur-*` job runs, by
  rule — the pipeline stays green and nothing reaches the AUR. Masking is
  not on the table: it wants a single-line value with no spaces.
- **`gitorii-bin` needs the release job fixed first.** Its binary
  checksums are 64 zeros — a recipe that fails loudly rather than one
  that skips verification. They fill in on the first `update.sh` run
  after the registry has artifacts for the tag.
- **`gitorii` (from source) does not depend on any of that.** It builds
  from the tag tarball, whose checksum is already recorded and verified.
  `publish-aur-src` has `needs: []` for that reason and will ship it on
  the next stable tag once the key is configured; until then the same
  directory can be pushed by hand to take the AUR package from
  `0.7.11-1` to the current release.
- The README and gitorii.com should only advertise `gitorii-bin` once it
  is actually published.

The ROADMAP dropped a second AUR package in May to keep the maintenance
surface small for one developer. With the push automated, the second one
costs what the first one costs.
