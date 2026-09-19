#!/usr/bin/env bash
# Build the local `wayle` release binary and produce the wayle-local Arch
# package (.pkg.tar.zst). Self-contained: regenerates the PKGBUILD if it is
# missing (it is untracked and easily wiped by a git reset/reclone).
#
# Usage:
#   packaging/arch/build.sh            # build binary + package
#   packaging/arch/build.sh --no-build # skip cargo, repackage existing binary
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"

if [[ "${1:-}" != "--no-build" ]]; then
    echo "==> Building release binary (LTO — this takes ~10 min)..."
    ( cd "$REPO_ROOT" && cargo build --release -p wayle )
fi

if [[ ! -x "$REPO_ROOT/target/release/wayle" ]]; then
    echo "error: $REPO_ROOT/target/release/wayle not found (run without --no-build)" >&2
    exit 1
fi

echo "==> Staging binary into packaging/arch/"
install -m755 "$REPO_ROOT/target/release/wayle" "$SCRIPT_DIR/wayle"

if [[ ! -f "$SCRIPT_DIR/PKGBUILD" ]]; then
    echo "==> PKGBUILD missing — regenerating"
    cat > "$SCRIPT_DIR/PKGBUILD" <<'PKGBUILD'
# Maintainer: local build
# Prebuilt, locally-compiled `wayle` binary (patched with battery OSD events).
# Reuses every other file from the currently installed wayle-bin package and
# swaps in the local `wayle` binary, so it is a complete drop-in replacement
# and needs no recompilation of wayle-settings or completion regeneration.
pkgname=wayle-local
pkgver=0.7.0
pkgrel=1
pkgdesc="Wayland shell (locally-built wayle binary with battery OSD events)"
arch=('x86_64')
url="https://wayle.app/"
license=('MIT')
provides=('wayle' 'wayle-bin')
conflicts=('wayle-bin')
replaces=('wayle-bin')
# Reuse whatever the installed wayle-bin already pulls in at runtime.
depends=('hicolor-icon-theme')
optdepends=('pipewire-pulseaudio' 'wireplumber' 'networkmanager' 'bluez'
            'upower' 'power-profiles-daemon')
options=('!strip')  # binary is already built without debuginfo
source=('wayle')
sha256sums=('SKIP')

package() {
    local _srcpkg=wayle-bin

    # Copy every regular file the installed wayle-bin owns, preserving mode.
    while read -r _pkg _path; do
        [ -f "$_path" ] || continue
        install -Dm"$(stat -c%a "$_path")" "$_path" "$pkgdir$_path"
    done < <(pacman -Ql "$_srcpkg")

    # Overwrite the shell binary with our patched build.
    install -Dm755 "$srcdir/wayle" "$pkgdir/usr/bin/wayle"
}
PKGBUILD
fi

echo "==> Running makepkg"
( cd "$SCRIPT_DIR" && makepkg -f )

pkg="$(ls -t "$SCRIPT_DIR"/*.pkg.tar.zst | head -1)"

echo "==> Verifying packaged binary matches the build"
built="$(sha256sum "$REPO_ROOT/target/release/wayle" | cut -d' ' -f1)"
packed="$(tar --use-compress-program=unzstd -xO -f "$pkg" usr/bin/wayle | sha256sum | cut -d' ' -f1)"
if [[ "$built" != "$packed" ]]; then
    echo "error: packaged binary ($packed) != build ($built)" >&2
    exit 1
fi

echo "==> Done: $pkg"
echo "    Install with: sudo pacman -U '$pkg'"
