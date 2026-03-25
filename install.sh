#!/usr/bin/env bash
set -euo pipefail

REPO="${AGENTDIFF_REPO:-codeprakhar25/agentdiff}"
INSTALL_DIR="${AGENTDIFF_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${AGENTDIFF_VERSION:-}"

usage() {
  cat <<'EOF'
Install agentdiff and agentdiff-mcp from GitHub Releases.

Usage:
  install.sh [--version vX.Y.Z] [--dir /install/path]

Environment:
  AGENTDIFF_REPO        GitHub repo (default: codeprakhar25/agentdiff)
  AGENTDIFF_INSTALL_DIR Install directory (default: ~/.local/bin)
  AGENTDIFF_VERSION     Release tag (default: latest)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      VERSION="${2:-}"
      shift 2
      ;;
    --dir)
      INSTALL_DIR="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

need_cmd curl
need_cmd tar

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Linux) os_part="unknown-linux-gnu" ;;
  Darwin) os_part="apple-darwin" ;;
  *)
    echo "Unsupported OS: $os" >&2
    exit 1
    ;;
esac

case "$arch" in
  x86_64|amd64) arch_part="x86_64" ;;
  arm64|aarch64) arch_part="aarch64" ;;
  *)
    echo "Unsupported architecture: $arch" >&2
    exit 1
    ;;
esac

target="${arch_part}-${os_part}"
case "$target" in
  x86_64-unknown-linux-gnu|x86_64-apple-darwin|aarch64-apple-darwin)
    ;;
  *)
    echo "No prebuilt artifact configured for target: $target" >&2
    exit 1
    ;;
esac

api_base="https://api.github.com/repos/${REPO}/releases"

if [[ -z "$VERSION" ]]; then
  VERSION="$(curl -fsSL "${api_base}/latest" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)"
  if [[ -z "$VERSION" ]]; then
    echo "Unable to resolve latest release tag for ${REPO}" >&2
    exit 1
  fi
fi

asset="agentdiff-${VERSION}-${target}.tar.gz"
checksums="SHA256SUMS"
download_base="https://github.com/${REPO}/releases/download/${VERSION}"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Installing ${REPO} ${VERSION} for ${target}"
curl -fL --retry 3 --retry-delay 1 -o "${tmp}/${asset}" "${download_base}/${asset}"
curl -fL --retry 3 --retry-delay 1 -o "${tmp}/${checksums}" "${download_base}/${checksums}"

line="$(grep " ${asset}\$" "${tmp}/${checksums}" || true)"
if [[ -z "$line" ]]; then
  echo "Checksum for ${asset} not found in ${checksums}" >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$tmp" && echo "$line" | sha256sum -c - >/dev/null)
elif command -v shasum >/dev/null 2>&1; then
  expected="$(echo "$line" | awk '{print $1}')"
  actual="$(shasum -a 256 "${tmp}/${asset}" | awk '{print $1}')"
  [[ "$expected" == "$actual" ]]
else
  echo "Missing sha256 verifier (need sha256sum or shasum)" >&2
  exit 1
fi

tar -xzf "${tmp}/${asset}" -C "$tmp"
mkdir -p "$INSTALL_DIR"
install -m 0755 "${tmp}/agentdiff" "${INSTALL_DIR}/agentdiff"
install -m 0755 "${tmp}/agentdiff-mcp" "${INSTALL_DIR}/agentdiff-mcp"

echo "Installed:"
echo "  ${INSTALL_DIR}/agentdiff"
echo "  ${INSTALL_DIR}/agentdiff-mcp"
echo
echo "If needed, add to PATH:"
echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
