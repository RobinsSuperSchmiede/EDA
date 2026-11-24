#!/bin/sh
# OpenShell installer for macOS and Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/RobinsSuperSchmiede/EDA/main/scripts/install.sh | sh
#
# Installs `openshell` and `openshell-desktop` from the latest GitHub release into
# $OPENSHELL_HOME/bin (default: ~/.openshell/bin). Set OPENSHELL_VERSION=v0.1.0 to pin a version.
set -eu

REPO="RobinsSuperSchmiede/EDA"
HOME_DIR="${OPENSHELL_HOME:-$HOME/.openshell}"
BIN_DIR="$HOME_DIR/bin"

os="$(uname -s)"
arch="$(uname -m)"
# Termux (Android): the CLI runs there and creates, develops, runs and bundles apps on the phone
if [ "$(uname -o 2>/dev/null)" = "Android" ] || [ -n "${TERMUX_VERSION:-}" ]; then
  os="Android"
fi
case "$os-$arch" in
  Android-aarch64) target="aarch64-linux-android" ;;
  Linux-aarch64) target="aarch64-unknown-linux-gnu" ;;
  Darwin-arm64) target="aarch64-apple-darwin" ;;
  Darwin-x86_64) target="x86_64-apple-darwin" ;;
  Linux-x86_64) target="x86_64-unknown-linux-gnu" ;;
  *) echo "openshell: no prebuilt binaries for $os/$arch – build from source: cargo build --release" >&2; exit 1 ;;
esac

if [ -n "${OPENSHELL_VERSION:-}" ]; then
  url="https://github.com/$REPO/releases/download/$OPENSHELL_VERSION/openshell-$target.tar.gz"
else
  url="https://github.com/$REPO/releases/latest/download/openshell-$target.tar.gz"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
echo "Downloading $url"
curl -fsSL "$url" -o "$tmp/openshell.tar.gz"
tar xzf "$tmp/openshell.tar.gz" -C "$tmp"
mkdir -p "$BIN_DIR"
cp "$tmp/openshell-$target/openshell" "$BIN_DIR/"
chmod +x "$BIN_DIR/openshell"
# the desktop window is not part of the CLI-only builds (Android / Termux, Linux arm64)
if [ -f "$tmp/openshell-$target/openshell-desktop" ]; then
  cp "$tmp/openshell-$target/openshell-desktop" "$BIN_DIR/"
  chmod +x "$BIN_DIR/openshell-desktop"
fi

echo
echo "OpenShell installed to $BIN_DIR"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "Add it to your PATH, e.g.:  echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.profile" ;;
esac
if [ "$os" = "Android" ]; then
  echo "Termux: install runtimes with 'pkg install python nodejs openjdk-17 rust golang'."
  echo "Apps run headless; open the frontend URL in the phone's browser. 'termux-wake-lock' is taken automatically."
elif [ "$os" = "Linux" ]; then
  echo "Linux needs WebKitGTK 4.1 for the desktop window (Debian/Ubuntu: sudo apt install libwebkit2gtk-4.1-0)."
fi
echo "Next: openshell create my-app && cd my-app && openshell dev"
