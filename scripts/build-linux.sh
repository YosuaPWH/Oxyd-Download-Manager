#!/usr/bin/env bash
# Builds the Linux packages (app + oxyd-bridge sidecar): .deb, .rpm and .AppImage.
#
# Needs the Tauri system libraries, e.g. on Ubuntu/Debian:
#   sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf
# Usage: bash scripts/build-linux.sh [bundles]   (default: deb,rpm,appimage)
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
bundles="${1:-deb,rpm,appimage}"

triple="$(rustc -vV | sed -n 's/^host: //p')"
echo "==> building oxyd-bridge ($triple)"
cargo build --release -p oxyd-bridge
target_dir="${CARGO_TARGET_DIR:-$root/target}"
mkdir -p app/src-tauri/binaries
cp "$target_dir/release/oxyd-bridge" "app/src-tauri/binaries/oxyd-bridge-$triple"

echo "==> building extension"
(cd extension && npm ci && npm run build)

# deb and rpm install this manifest for Chrome, Chromium and Edge under /etc, pointing at
# the bridge in /usr/bin (Linux requires an absolute path). The AppImage registers itself
# per user when the app starts instead.
mkdir -p app/src-tauri/native-host/linux
sed 's#__BRIDGE_PATH__#/usr/bin/oxyd-bridge#' installer/native-host/com.oxyd.bridge.json.template \
    > app/src-tauri/native-host/linux/com.oxyd.bridge.json

echo "==> building packages ($bundles)"
(cd app && npm ci && npx tauri build --bundles "$bundles" --config src-tauri/tauri.linux.bundle.json)

find "$target_dir/release/bundle" -maxdepth 2 -type f \
    \( -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' \) -exec ls -lh {} \;
