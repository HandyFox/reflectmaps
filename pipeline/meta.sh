#!/usr/bin/env bash
# Usage: meta.sh TAG FILE...
#
# For each file: write <file>.meta.json with its size and SHA-256, then
# upload it to the GitHub release TAG. GitHub refuses release files of
# 2 GiB or more, so larger files are uploaded as numbered parts
# (<file>.part00, .part01, ...); the meta lists them and the app joins them
# back together before checking the SHA-256 of the whole file.
set -euo pipefail
tag="$1"
shift
limit=$((1900 * 1024 * 1024))
for f in "$@"; do
  name=$(basename "$f")
  size=$(stat -c %s "$f")
  sha=$(sha256sum "$f" | cut -d' ' -f1)
  if [ "$size" -gt "$limit" ]; then
    split -b "$limit" -d -a 2 "$f" "$f.part"
    rm -f "$f"
    parts=""
    for p in "$f".part*; do
      parts="$parts{\"file\":\"$(basename "$p")\",\"size\":$(stat -c %s "$p")},"
      gh release upload "$tag" "$p" --clobber
    done
    printf '{"file":"%s","size":%s,"sha256":"%s","parts":[%s]}\n' "$name" "$size" "$sha" "${parts%,}" > "$f.meta.json"
    echo "$name $size $sha (uploaded in parts)"
  else
    gh release upload "$tag" "$f" --clobber
    printf '{"file":"%s","size":%s,"sha256":"%s"}\n' "$name" "$size" "$sha" > "$f.meta.json"
    echo "$name $size $sha"
  fi
done
