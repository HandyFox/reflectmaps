#!/usr/bin/env bash
# Write <file>.meta.json with the size and SHA-256 of each given file.
set -euo pipefail
for f in "$@"; do
  name=$(basename "$f")
  size=$(stat -c %s "$f")
  sha=$(sha256sum "$f" | cut -d' ' -f1)
  printf '{"file":"%s","size":%s,"sha256":"%s"}\n' "$name" "$size" "$sha" > "$f.meta.json"
  echo "$name $size $sha"
done
