#!/usr/bin/env sh
# ontosys installer — fetches one binary from GitHub Releases and puts it on PATH.
#
#   curl -fsSL .../install.sh | sh                 # latest
#   VERSION=v0.1.0 sh install.sh                   # pin a tag
#   PREFIX=$HOME/.local/bin sh install.sh          # choose the install dir
#
# NOTE ON PRIVATE REPOS: while styk-tv/ontosys is private, release assets are NOT
# anonymously downloadable — the plain releases/download URL returns 404 without
# credentials. This script therefore authenticates, in order of preference:
#   1. the `gh` CLI, if installed and logged in
#   2. $GH_TOKEN / $GITHUB_TOKEN, via the REST asset endpoint
# Once the repo is public, the unauthenticated path below starts working as-is.
set -eu

REPO="${REPO:-styk-tv/ontosys}"
VERSION="${VERSION:-latest}"
PREFIX="${PREFIX:-/usr/local/bin}"
BIN="ontosys"

die() { printf 'install: %s\n' "$1" >&2; exit 1; }

os="$(uname -s)"
case "$os" in
  Linux) ;;
  Darwin) die "no macOS binary is published yet — build locally with: cargo install --path ." ;;
  *) die "unsupported OS: $os" ;;
esac

# Accept both spellings a machine might report, and normalise to the release's.
case "$(uname -m)" in
  x86_64 | amd64) arch=amd64 ;;
  aarch64 | arm64) arch=arm64 ;;
  *) die "unsupported architecture: $(uname -m) (published: amd64, arm64)" ;;
esac

asset="${BIN}-linux-${arch}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

printf 'install: %s %s (%s)\n' "$REPO" "$VERSION" "$asset"

if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  if [ "$VERSION" = latest ]; then
    gh release download --repo "$REPO" --pattern "$asset" --dir "$tmp"
  else
    gh release download "$VERSION" --repo "$REPO" --pattern "$asset" --dir "$tmp"
  fi
else
  token="${GH_TOKEN:-${GITHUB_TOKEN:-}}"
  api="https://api.github.com/repos/${REPO}/releases"
  [ "$VERSION" = latest ] && api="${api}/latest" || api="${api}/tags/${VERSION}"

  if [ -n "$token" ]; then
    # Resolve the numeric asset id, then stream the asset itself. The browser
    # download URL will not serve a private asset even with a token; this will.
    id="$(curl -fsSL -H "Authorization: Bearer $token" \
            -H 'Accept: application/vnd.github+json' "$api" \
          | python3 -c "import json,sys
r = json.load(sys.stdin)
m = [a['id'] for a in r.get('assets', []) if a['name'] == '$asset']
print(m[0] if m else '')")" || die "could not read release $VERSION"
    [ -n "$id" ] || die "release $VERSION has no asset named $asset"
    curl -fsSL -H "Authorization: Bearer $token" -H 'Accept: application/octet-stream' \
      -o "$tmp/$asset" "https://api.github.com/repos/${REPO}/releases/assets/${id}"
  else
    # Anonymous path — works only once the repo is public.
    if [ "$VERSION" = latest ]; then
      url="https://github.com/${REPO}/releases/latest/download/${asset}"
    else
      url="https://github.com/${REPO}/releases/download/${VERSION}/${asset}"
    fi
    curl -fsSL -o "$tmp/$asset" "$url" \
      || die "download failed. $REPO is private — install the gh CLI, or set GH_TOKEN."
  fi
fi

[ -s "$tmp/$asset" ] || die "downloaded file is empty"
chmod +x "$tmp/$asset"

if [ -w "$PREFIX" ]; then
  install -m 0755 "$tmp/$asset" "$PREFIX/$BIN"
else
  printf 'install: %s is not writable, using sudo\n' "$PREFIX"
  sudo install -m 0755 "$tmp/$asset" "$PREFIX/$BIN"
fi

printf 'install: %s -> %s/%s\n' "$asset" "$PREFIX" "$BIN"
"$PREFIX/$BIN" --version || true
