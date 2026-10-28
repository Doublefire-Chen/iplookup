#!/usr/bin/env bash
# Packages a release: scripts/package.sh vX.Y.Z OUT_DIR
#
# Expects the musl binaries already built (cargo zigbuild --release
# --target x86_64-unknown-linux-musl --target aarch64-unknown-linux-musl,
# in server/) and the web pages in web/dist (pnpm build). Writes to OUT_DIR
# one tarball per architecture, iplookup-vX.Y.Z-ARCH-linux-musl.tar.gz,
# unpacking to a directory of the same name:
#
#   iplookup                  the static binary
#   web/                      the web build (index.html, assets/)
#   iplookup.toml             example config
#   iplookup.service          systemd unit
#   nginx-iplookup.conf       nginx site
#   unbound-iplookup.conf     unbound snippet
#   DEPLOY.md                 deploy/README.md
#   README.md                 API documentation
#   LICENSE
#
# plus SHA256SUMS. Run by the release job in .github/workflows/ci.yml; GNU
# tar (gtar on macOS) is needed for --owner/--group.
set -euo pipefail

version=${1:?usage: scripts/package.sh vX.Y.Z OUT_DIR}
out=${2:?usage: scripts/package.sh vX.Y.Z OUT_DIR}
root=$(cd "$(dirname "$0")/.." && pwd)
tar=$(command -v gtar || command -v tar)

test -f "$root/web/dist/index.html" || { echo "no web/dist/index.html: run pnpm build" >&2; exit 1; }

mkdir -p "$out"
out=$(cd "$out" && pwd)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

for arch in x86_64 aarch64; do
    name="iplookup-$version-$arch-linux-musl"
    dir="$stage/$name"
    mkdir -p "$dir"
    install -m 0755 "$root/server/target/$arch-unknown-linux-musl/release/iplookup" "$dir/iplookup"
    cp -R "$root/web/dist" "$dir/web"
    cp "$root/deploy/iplookup.toml" "$root/deploy/iplookup.service" \
        "$root/deploy/nginx-iplookup.conf" "$root/deploy/unbound-iplookup.conf" \
        "$root/README.md" "$root/LICENSE" "$dir/"
    cp "$root/deploy/README.md" "$dir/DEPLOY.md"
    "$tar" --owner=0 --group=0 --sort=name -czf "$out/$name.tar.gz" -C "$stage" "$name"
done

cd "$out"
sha256sum -- iplookup-"$version"-*.tar.gz > SHA256SUMS
ls -l "$out"
