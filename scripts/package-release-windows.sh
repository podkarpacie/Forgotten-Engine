#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <release-tag>" >&2
  exit 2
fi

release_tag="$1"
archive="forgotten-engine-${release_tag}-windows-x86_64.zip"
checksum="SHA256SUMS-${release_tag}-windows-x86_64.txt"
executable="target/x86_64-pc-windows-gnu/release/forgotten-engine.exe"
staging_dir="dist/staging-windows-x86_64"

if [[ ! -f "$executable" ]]; then
  echo "missing $executable; build with the x86_64-pc-windows-gnu target first" >&2
  exit 1
fi

rm -rf "$staging_dir"
mkdir -p "$staging_dir"

cp "$executable" "$staging_dir/forgotten-engine.exe"
cp README.md VERSIONING.md RELEASE_ASSETS.md "$staging_dir/"
cat > "$staging_dir/INSTALL.txt" <<'EOF'
Forgotten Engine precompiled Windows archive

1. Run: .\forgotten-engine.exe init .\my-world --profile fe-7.4
2. Run: .\forgotten-engine.exe validate .\my-world
3. Run: .\forgotten-engine.exe version

This archive contains no Tibia client assets, maps, item databases, or game content.
EOF

mkdir -p dist
rm -f "dist/$archive" "dist/$checksum"
(cd "$staging_dir" && zip -q -r "../../dist/$archive" .)
(cd dist && sha256sum "$archive" > "$checksum")

printf '%s\n' "dist/$archive"
