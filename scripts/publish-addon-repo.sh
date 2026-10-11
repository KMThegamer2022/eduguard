#!/usr/bin/env bash
# Sync packaging/homeassistant/ into the standalone GitHub add-on repository
# and push it, so Home Assistant's Add-on Store can fetch a new version.
#
#   ./scripts/build-all.sh && ./scripts/package-all.sh   # refresh binaries
#   ./scripts/publish-addon-repo.sh                      # this script
#
# Home Assistant detects add-on updates by the `version:` field, so the
# patch level is bumped automatically on every publish.
set -euo pipefail

cd "$(dirname "$0")/.."
SRC=packaging/homeassistant
REPO="$(cd .. && pwd)/eduguard-addons"

for arch in x86_64 aarch64; do
  if [ ! -f "$SRC/eduguard-server/bin/$arch/eduguard-server" ]; then
    echo "missing $arch server binary — run scripts/package-all.sh first" >&2
    exit 1
  fi
done

if [ ! -d "$REPO/.git" ]; then
  echo "add-on repository clone not found at $REPO" >&2
  echo "create it once with: gh repo create KMThegamer2022/eduguard-addons --public --source $REPO --push" >&2
  exit 1
fi

# Refresh tracked content (the repository's own .git is left alone).
rsync -a --delete --exclude '.git' "$SRC/" "$REPO/"

cfg="$REPO/eduguard-server/config.yaml"
ver=$(sed -n 's/^version: "\(.*\)"/\1/p' "$cfg")
IFS=. read -r major minor patch <<<"$ver"
new="$major.$minor.$((patch + 1))"
sed -i '' "s/^version: \"$ver\"/version: \"$new\"/" "$cfg"

git -C "$REPO" add -A
git -C "$REPO" commit -q -m "EduGuard server add-on $new"
git -C "$REPO" push

echo "pushed eduguard-addons $new — in Home Assistant: Add-on Store → ⋮ → Check for updates"
