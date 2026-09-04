# AUR packaging

This directory is the upstream for the two AUR packages. The
`aur.archlinux.org` git repositories are mirrors of it: edit here, let
CI push, and never commit straight to the AUR — a change made there is
lost the next time a tag is released.

| Package       | Source                                              | For                                         |
| ------------- | --------------------------------------------------- | ------------------------------------------- |
| `gitorii`     | the tag tarball from GitLab, built with cargo        | people who prefer to build from source      |
| `gitorii-bin` | the binaries the `release` job uploads to the registry | everyone else — no compiler, no rust toolchain |

They declare `conflicts` on each other, so only one can be installed at
a time. Both install the binary as `/usr/bin/torii`.

## Releasing

Two jobs in `.gitlab-ci.yml` do this automatically on stable tags, one
per package. `publish-aur-src` waits for nothing — the tarball it hashes
is GitLab's archive of the tag. `publish-aur-bin` runs after `release`
has uploaded the binaries, because those are what it hashes. By hand:

```sh
./update.sh 0.15.1                  # both packages
./update.sh 0.15.1 gitorii-bin      # just one

git -C ../.. diff -- packaging/aur  # read it before committing
```

`update.sh` rewrites `pkgver`, resets `pkgrel` to 1, re-downloads every
source, recomputes the checksums and regenerates `.SRCINFO`. It needs
network, `makepkg` and `updpkgsums` (Arch package `pacman-contrib`).

To push a recipe to the AUR without waiting for CI:

```sh
git clone ssh://aur@aur.archlinux.org/gitorii-bin.git /tmp/aur-gitorii-bin
cp gitorii-bin/PKGBUILD gitorii-bin/.SRCINFO /tmp/aur-gitorii-bin/
cd /tmp/aur-gitorii-bin && git commit -am "gitorii-bin 0.15.1" && git push
```

## Things that will bite you

**`gitorii-bin` cannot be published until the release job runs again.**
Binary artifacts stopped being uploaded after `v0.7.15` (see
`BACKLOG.md`), so `sha256sums_x86_64` and `sha256sums_aarch64` are
sitting at 64 zeros: the recipe is complete but deliberately
unbuildable. `SKIP` is not the fix — an unverified binary download is
exactly what this package must not do. Run `update.sh` once the
registry has the artifacts for the tag and the real sums appear.

**`updpkgsums` only knows about the host's architecture.** `makepkg -g`
emits sums for `$CARCH` and nothing else, so running it on an x86_64 box
leaves `sha256sums_aarch64` pointing at whatever the previous release
hashed to — a wrong sum that looks plausible. `update.sh` handles the
other architectures itself, reading their URLs back out of the
`.SRCINFO` it just generated. If you ever update sums by hand, remember
this.

**GitLab generates the source tarball on demand.** The `gitorii`
package's checksum covers an archive GitLab compresses when asked, not a
file stored somewhere. It has been stable in practice, but if a future
GitLab release changes its compression the published sum stops matching
and the package needs a `pkgrel` bump with a fresh sum. `gitorii-bin`
has no such exposure — the package registry stores real files.

**A local `makepkg` does not build the way `cargo` does.** makepkg
replaces `RUSTFLAGS` with whatever the distro's config says, and the
environment cannot override it — `/etc/makepkg.conf.d/*.conf` is sourced
after `/etc/makepkg.conf`, and only `~/.makepkg.conf` comes later. On
CachyOS that means `-C opt-level=3 -C target-cpu=native` plus
`MAKEFLAGS=-j$(nproc)`, which on a Ryzen 3700X reproducibly SIGSEGVs
rustc inside LLVM's optimizer while compiling `syn` (kernel logs a plain
`error 4` segfault, no MCE, no OOM). Stock Arch does none of this, so it
is not a defect in the recipe — but to smoke-test locally:

```sh
printf 'RUSTFLAGS="-C opt-level=2"\nMAKEFLAGS="-j2"\n' > ~/.makepkg.conf
makepkg -fd     # -d: cargo comes from rustup here, pacman cannot see it
rm ~/.makepkg.conf
```

**The tarball ships the whole workspace.** `check()` therefore runs the
real test suite, and `--frozen` uses the committed `Cargo.lock`, so an
AUR build compiles the same dependency versions CI does.
