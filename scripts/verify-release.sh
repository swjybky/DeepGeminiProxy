#!/usr/bin/env bash
set -euo pipefail

fail() { echo "::error::$*" >&2; exit 1; }

version=$(jq -er '.version' package.json)
number='(0|[1-9][0-9]*)'
identifier='(0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)'
if [[ ! "$version" =~ ^${number}\.${number}\.${number}(-${identifier}(\.${identifier})*)?$ ]]; then
  fail 'package.json must use X.Y.Z or X.Y.Z-prerelease (no build metadata).'
fi
cargo_version=$(awk -F '"' '/^version[[:space:]]*=/ { print $2; exit }' src-tauri/Cargo.toml)
if [[ $(jq -er '.version' src-tauri/tauri.conf.json) != "$version" || "$cargo_version" != "$version" ]] ||
   ! jq -e --arg version "$version" '.version == $version and .packages[""].version == $version' package-lock.json >/dev/null; then
  fail 'Version mismatch. Sync package.json, package-lock.json, Cargo.toml and tauri.conf.json before building.'
fi

tag="v$version"
if [[ "$GITHUB_REF" == refs/tags/* && "$GITHUB_REF" != "refs/tags/$tag" ]]; then
  fail "Git tag must match the checked-out version ($tag)."
fi

prerelease=false
branch=main
if [[ "$version" == *-* ]]; then
  prerelease=true
  branch=beta
fi

if [[ "$PUBLISH" == true ]]; then
  commit=$(git rev-parse HEAD)
  if git show-ref --verify --quiet "refs/tags/$tag" && [[ $(git rev-parse "refs/tags/$tag^{commit}") != "$commit" ]]; then
    fail "Tag $tag already points to a different commit. Bump the version instead of moving the tag."
  fi
  case "$GITHUB_REF" in
    refs/heads/"$branch") ;;
    refs/tags/*)
      git merge-base --is-ancestor "$commit" "refs/remotes/origin/$branch" ||
        fail "$tag must point to a commit reachable from origin/$branch."
      ;;
    *) fail "Publish $tag from the $branch branch." ;;
  esac
fi

printf 'release_tag=%s\nis_prerelease=%s\npublish=%s\n' "$tag" "$prerelease" "$PUBLISH" >> "$GITHUB_OUTPUT"
