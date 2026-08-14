#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <release-tag>" >&2
  exit 2
fi

release_tag="$1"
archive="forgotten-engine-${release_tag}-linux-x86_64.zip"
checksum="SHA256SUMS-${release_tag}-linux-x86_64.txt"
staging_dir="dist/staging-linux-x86_64"

rm -rf "$staging_dir"
mkdir -p "$staging_dir"

cp target/release/forgotten-engine "$staging_dir/forgotten-engine"
cat > "$staging_dir/INSTALL.txt" <<'EOF'
Forgotten Engine precompiled Linux archive

Run ./forgotten-engine help for local commands.

This package contains the executable only. Documentation will be maintained in the Forgotten Engine GitBook.
EOF

mkdir -p dist
rm -f "dist/$archive" "dist/$checksum"
(cd "$staging_dir" && zip -q -r "../../dist/$archive" .)
(cd dist && sha256sum "$archive" > "$checksum")

printf '%s\n' "dist/$archive"
