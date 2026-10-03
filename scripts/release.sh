#!/usr/bin/env bash
# Starts a fork's release build in GitHub Actions and follows it.
#
#   scripts/release.sh punktfunk v0.42.0.usbridge.2           # all platforms + release
#   scripts/release.sh punktfunk v0.42.0.usbridge.2 windows   # one platform + release
#   scripts/release.sh sunshine  v2026.1003.1.usbridge linux
#   scripts/release.sh punktfunk "" windows                   # build only, no release
#
# Platforms: all (default), linux, windows; sunshine also macos.
#
# Same as Actions -> "<Fork> Release" -> Run workflow. Needs `gh` logged in.
set -euo pipefail

fork="${1:-}"
tag="${2:-}"
platform="${3:-all}"
case "$fork" in
  punktfunk) workflow=punktfunk-release.yml ;;
  sunshine) workflow=sunshine-release.yml ;;
  *)
    echo "usage: $0 punktfunk|sunshine [tag] [all|linux|windows|macos]" >&2
    exit 2
    ;;
esac

repo=USBridge-Technologies/Streamers-Forks
if [[ -n "$tag" ]] && gh release view "$tag" -R "$repo" >/dev/null 2>&1; then
  echo "release $tag already exists" >&2
  exit 1
fi

since="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
gh workflow run "$workflow" -R "$repo" --ref main -f tag="$tag" -f platform="$platform"

# The run shows up a few seconds after the dispatch.
run=""
for _ in $(seq 30); do
  run="$(gh run list -R "$repo" -w "$workflow" -e workflow_dispatch -L 1 \
    --json databaseId,createdAt -q ".[] | select(.createdAt >= \"$since\") | .databaseId")"
  [[ -n "$run" ]] && break
  sleep 2
done
if [[ -z "$run" ]]; then
  echo "dispatched, but the run didn't show up; see https://github.com/$repo/actions" >&2
  exit 1
fi

echo "https://github.com/$repo/actions/runs/$run"
gh run watch "$run" -R "$repo" --exit-status
if [[ -n "$tag" ]]; then
  echo "https://github.com/$repo/releases/tag/$tag"
fi
