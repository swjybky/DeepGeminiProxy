#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C

fail() { echo "::error::$*" >&2; exit 1; }

mkdir release-files
[[ -d all-artifacts ]] || fail 'No workflow artifacts downloaded.'
while IFS= read -r -d '' file; do
  name=${file##*/}
  case "$name" in
    *.dmg|*.deb|*.rpm|*.AppImage|*.exe|*.msi)
      [[ -s "$file" ]] || fail "Empty installer: $file"
      # GitHub rewrites spaces and special characters in asset names.
      [[ "$name" =~ ^[a-zA-Z0-9_.-]+$ ]] || fail "Unsupported release asset name: $name"
      [[ ! -e "release-files/$name" ]] || fail "Duplicate release asset: $name"
      cp -- "$file" "release-files/$name"
      ;;
  esac
done < <(find all-artifacts -type f -print0)

shopt -s nullglob
installers=(release-files/*)
(( ${#installers[@]} > 0 )) || fail 'No installers downloaded.'

# Extract the exact tag's entry from both changelogs, without matching adjacent beta versions.
for changelog in CHANGELOG_EN.md CHANGELOG.md; do
  [[ -f "$changelog" ]] || continue
  notes=$(awk -v tag="$RELEASE_TAG" '
    BEGIN { title = "**" tag " " }
    /^[[:space:]]*[*+-][[:space:]]+\*\*v/ {
      if (capture) exit
      heading = $0
      sub(/^[[:space:]]*[*+-][[:space:]]+/, "", heading)
      if (index(heading, title) == 1) capture = 1
      next
    }
    capture { sub(/^        /, ""); print }
  ' "$changelog")
  if [[ -n "$notes" ]]; then
    if [[ -s release_notes.md ]]; then printf '\n---\n\n'; fi
    printf '%s\n' "$notes"
  fi
done > release_notes.md
if [[ ! -s release_notes.md ]]; then
  printf 'Download the installer for your platform from Assets.\n' > release_notes.md
fi

(cd release-files && sha256sum -- * > SHA256SUMS.txt)
