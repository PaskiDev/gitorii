#!/usr/bin/env bash
#
# Refresh the AUR recipes in this directory for a released version:
# rewrite pkgver, re-download every source, recompute the checksums and
# regenerate .SRCINFO.
#
#   ./update.sh              # version from the workspace Cargo.toml
#   ./update.sh 0.15.1       # explicit version
#   ./update.sh 0.15.1 gitorii-bin   # one package only
#
# Sources are fetched, so this needs network. gitorii-bin additionally
# needs the CI `release` job to have uploaded the binaries for the tag —
# it fails loudly, on purpose, if they are not there yet.

set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(git -C "$here" rev-parse --show-toplevel)

if [[ $# -gt 0 ]]; then
	version=$1
	shift
else
	version=$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$repo_root/Cargo.toml")
fi
if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
	echo "update.sh: '$version' is not a x.y.z version" >&2
	exit 1
fi

packages=("$@")
if [[ ${#packages[@]} -eq 0 ]]; then
	packages=(gitorii gitorii-bin)
fi

workdir=$(mktemp -d)
trap 'rm -rf -- "${workdir:?}"' EXIT

for pkg in "${packages[@]}"; do
	pkgdir="$here/$pkg"
	if [[ ! -f "$pkgdir/PKGBUILD" ]]; then
		echo "update.sh: no PKGBUILD in $pkgdir" >&2
		exit 1
	fi

	echo "==> $pkg $version"
	stage="$workdir/$pkg"
	mkdir -p "$stage"
	cp "$pkgdir/PKGBUILD" "$stage/PKGBUILD"

	sed -i "s/^pkgver=.*/pkgver=$version/" "$stage/PKGBUILD"
	sed -i "s/^pkgrel=.*/pkgrel=1/" "$stage/PKGBUILD"

	# updpkgsums covers the arch-independent `source` array and the
	# `source_$CARCH` one for the machine we happen to be running on.
	( cd "$stage" && updpkgsums && makepkg --printsrcinfo > .SRCINFO )

	# Every *other* architecture is invisible to makepkg -g here: it only
	# ever emits sums for the host CARCH, so `sha256sums_aarch64` would
	# silently keep the previous release's value on an x86_64 box. Take
	# the URLs from the .SRCINFO makepkg just wrote — that way they stay
	# defined in one place, the PKGBUILD — and fill those in by hand.
	host_arch=$(uname -m)
	while read -r arch url; do
		if [[ $arch == "$host_arch" ]]; then
			continue
		fi
		echo "    $arch: $url"
		sum=$(curl -fsSL --retry 3 -- "$url" | sha256sum | cut -d' ' -f1)
		sed -i "s|^sha256sums_$arch=(.*|sha256sums_$arch=('$sum')|" "$stage/PKGBUILD"
		# A sed that matches nothing leaves the old sum in place and says
		# so to nobody — the one failure here that would ship a wrong
		# checksum. Single-line array assumed; check it actually landed.
		if ! grep -q "^sha256sums_$arch=('$sum')\$" "$stage/PKGBUILD"; then
			echo "update.sh: could not write sha256sums_$arch in $pkg/PKGBUILD." >&2
			echo "  Expected a single-line 'sha256sums_$arch=(...)' assignment." >&2
			exit 1
		fi
	done < <(awk -F' = ' '
		/^[[:space:]]*source_[a-z0-9_]+ = / {
			arch = $1; sub(/^[[:space:]]*source_/, "", arch)
			url = $2; sub(/^[^:]*::/, "", url)
			print arch, url
		}' "$stage/.SRCINFO")

	# Regenerate so .SRCINFO carries the sums we just filled in.
	( cd "$stage" && makepkg --printsrcinfo > .SRCINFO )

	cp "$stage/PKGBUILD" "$pkgdir/PKGBUILD"
	cp "$stage/.SRCINFO" "$pkgdir/.SRCINFO"
done

echo "==> done. Review the diff before committing:"
echo "    git -C $repo_root diff -- packaging/aur"
