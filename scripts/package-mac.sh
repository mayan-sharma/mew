#!/usr/bin/env bash
# Builds dist/Mew.app and, if the Velopack CLI is installed, packs it: dist/releases/
# Requires: Rust, Xcode command line tools, Node.js; `dotnet tool install -g vpk` for the installer.
# Signs ad-hoc unless MEW_SIGN_IDENTITY names a certificate. macOS ties the Accessibility and Input Monitoring
# permissions to the signature, so a stable identity (a self-signed one works) keeps them across rebuilds.
set -euo pipefail
cd "$(dirname "$0")/.."
version=${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)}
identity=${MEW_SIGN_IDENTITY:--}
bundle_id=io.github.mayan-sharma.mew

# Pinned llama.cpp release (Metal build), and what llama-server loads (otool -L) under the names it asks for.
llama_build=b11714
llama_sha256=063f58b7d8458d89eacb2f28a10a6d6e0efa2a81866690b0f6f8ebac6e3846bb
llama_archive=.cache/llama-$llama_build-macos-arm64.tar.gz
llama_files=(llama-server libllama-server-impl.dylib libllama-common.0.dylib libllama.0.dylib libmtmd.0.dylib
    libggml.0.dylib libggml-base.0.dylib libggml-blas.0.dylib libggml-cpu.0.dylib libggml-metal.0.dylib
    libggml-rpc.0.dylib)

echo "Building the Mew window (ui/)"
[ -d ui/node_modules ] || npm --prefix ui ci
npm --prefix ui run build

echo "Building Mew $version"
cargo build --release

if [ ! -f "$llama_archive" ]; then
    mkdir -p .cache
    curl -fL -o "$llama_archive" \
        "https://github.com/ggml-org/llama.cpp/releases/download/$llama_build/llama-$llama_build-bin-macos-arm64.tar.gz"
fi
echo "$llama_sha256  $llama_archive" | shasum -a 256 -c --status - || { echo "llama.cpp archive checksum mismatch" >&2; exit 1; }

app=dist/Mew.app
unpacked=dist/llama-unpacked
rm -rf "$app" "$unpacked"
mkdir -p "$app/Contents/MacOS/llama" "$app/Contents/Resources" "$unpacked"
tar -xzf "$llama_archive" -C "$unpacked"
for f in "${llama_files[@]}"; do cp -L "$unpacked/llama-$llama_build/$f" "$app/Contents/MacOS/llama/"; done
rm -rf "$unpacked"
cp target/release/mew "$app/Contents/MacOS/"
cp assets/NOTICE.txt "$app/Contents/Resources/"
cp assets/mew.icns "$app/Contents/Resources/Mew.icns"
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>mew</string>
    <key>CFBundleIdentifier</key><string>$bundle_id</string>
    <key>CFBundleName</key><string>Mew</string>
    <key>CFBundleDisplayName</key><string>Mew</string>
    <key>CFBundleIconFile</key><string>Mew</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
    <key>LSMinimumSystemVersion</key><string>13.4</string>
    <key>LSUIElement</key><true/>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSMicrophoneUsageDescription</key>
    <string>Mew listens only while you hold the talk key, and turns what you say into text on this Mac.</string>
</dict>
</plist>
EOF
plutil -lint -s "$app/Contents/Info.plist"

echo "Signing with ${MEW_SIGN_IDENTITY:-an ad-hoc signature}"
codesign --force --sign "$identity" "$app/Contents/MacOS/llama/"*
codesign --force --sign "$identity" "$app"
codesign --verify --deep --strict "$app"
echo "Staged $(du -sh "$app" | cut -f1) in $app"

if ! command -v vpk >/dev/null; then
    echo "vpk not found (dotnet tool install -g vpk), so no installer was made. Run the app with: open $app"
    exit 0
fi
signing=()
[ -n "${MEW_SIGN_IDENTITY:-}" ] && signing=(--signAppIdentity "$MEW_SIGN_IDENTITY")
vpk pack --packId Mew --packVersion "$version" --packDir "$app" --mainExe mew --packTitle Mew --packAuthors Mew \
    --outputDir dist/releases ${signing[@]+"${signing[@]}"}
ls -lh dist/releases
