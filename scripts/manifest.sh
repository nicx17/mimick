#!/usr/bin/env bash
set -euo pipefail

# Updates the git tag and commit hash for mimick in dev.nicx.mimick.yml.
# Usage:
#   scripts/manifest.sh [TAG] [COMMIT_HASH]
#
# If TAG is omitted, it defaults to the version in Cargo.toml (prefixed with 'v').
# If COMMIT_HASH is omitted, it resolves the tag from git, remote origin, or HEAD.

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$repo_root/dev.nicx.mimick.yml"

if [[ ! -f "$manifest" ]]; then
  echo "Manifest file not found: $manifest" >&2
  exit 1
fi

raw_tag="${1:-}"
if [[ -z "$raw_tag" ]]; then
  version=$(grep '^version' "$repo_root/Cargo.toml" | head -1 | cut -d '"' -f 2)
  raw_tag="v$version"
fi

if [[ "$raw_tag" =~ ^v ]]; then
  tag="$raw_tag"
else
  tag="v$raw_tag"
fi

commit="${2:-}"
if [[ -z "$commit" ]]; then
  # 1. Try local tag
  commit=$(git -C "$repo_root" rev-parse -q --verify "${tag}^{commit}" 2>/dev/null || true)

  # 2. Try remote tag on origin
  if [[ -z "$commit" ]]; then
    commit=$(git -C "$repo_root" ls-remote --tags origin "refs/tags/${tag}^{}" "refs/tags/${tag}" 2>/dev/null | awk 'NR==1 {print $1}')
  fi

  # 3. Fallback to HEAD
  if [[ -z "$commit" ]]; then
    commit=$(git -C "$repo_root" rev-parse HEAD)
    echo "Notice: Tag '$tag' not found locally or on origin; using HEAD ($commit)."
  fi
fi

sed -i -E "s/^( +tag: ).*/\1${tag}/" "$manifest"
sed -i -E "s/^( +commit: ).*/\1${commit}/" "$manifest"

echo "Updated $manifest:"
echo "  tag:    $tag"
echo "  commit: $commit"
