#!/bin/sh
set -eu

REPO="${AGENTDIFF_REPO:-codeprakhar25/agentdiff}"
INSTALL_DIR="${AGENTDIFF_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${AGENTDIFF_VERSION:-}"

while [ $# -gt 0 ]; do
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
      echo "Install agentdiff from GitHub Releases."
      echo ""
      echo "Usage: install.sh [--version vX.Y.Z] [--dir /install/path]"
      echo ""
      echo "Env: AGENTDIFF_REPO, AGENTDIFF_INSTALL_DIR, AGENTDIFF_VERSION"
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
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
  Linux)  os_part="unknown-linux-gnu" ;;
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

api_base="https://api.github.com/repos/${REPO}/releases"

if [ -z "$VERSION" ]; then
  VERSION="$(curl -fsSL "${api_base}/latest" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)"
  if [ -z "$VERSION" ]; then
    echo "Unable to resolve latest release tag for ${REPO}" >&2
    exit 1
  fi
fi

asset="agentdiff-${VERSION}-${target}.tar.gz"
checksums="SHA256SUMS"
download_base="https://github.com/${REPO}/releases/download/${VERSION}"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Installing ${REPO} ${VERSION} for ${target}..."
curl -fL --retry 3 --retry-delay 1 -o "${tmp}/${asset}" "${download_base}/${asset}"
curl -fL --retry 3 --retry-delay 1 -o "${tmp}/${checksums}" "${download_base}/${checksums}"

line="$(grep " ${asset}\$" "${tmp}/${checksums}" || true)"
if [ -z "$line" ]; then
  echo "Checksum for ${asset} not found in ${checksums}" >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$tmp" && printf '%s\n' "$line" | sha256sum -c - >/dev/null)
elif command -v shasum >/dev/null 2>&1; then
  expected="$(printf '%s\n' "$line" | awk '{print $1}')"
  actual="$(shasum -a 256 "${tmp}/${asset}" | awk '{print $1}')"
  if [ "$expected" != "$actual" ]; then
    echo "Checksum mismatch for ${asset}" >&2
    exit 1
  fi
else
  echo "Missing sha256 verifier (need sha256sum or shasum)" >&2
  exit 1
fi

tar -xzf "${tmp}/${asset}" -C "$tmp"
mkdir -p "$INSTALL_DIR"
install -m 0755 "${tmp}/agentdiff" "${INSTALL_DIR}/agentdiff"
if [ -f "${tmp}/agentdiff-mcp" ]; then
  install -m 0755 "${tmp}/agentdiff-mcp" "${INSTALL_DIR}/agentdiff-mcp"
fi

SCRIPTS_SRC="${tmp}/scripts"
SCRIPTS_DST="${HOME}/.agentdiff/scripts"
if [ -d "$SCRIPTS_SRC" ] && ls "${SCRIPTS_SRC}"/*.py >/dev/null 2>&1; then
  mkdir -p "$SCRIPTS_DST"
  cp "${SCRIPTS_SRC}"/*.py "$SCRIPTS_DST/"
  echo "Capture scripts installed -> ${SCRIPTS_DST}/"
else
  echo "Note: capture scripts not bundled in this release."
  echo "Run 'agentdiff configure' to fetch them."
fi

echo ""
echo "Installed: ${INSTALL_DIR}/agentdiff $("${INSTALL_DIR}/agentdiff" --version 2>/dev/null || echo '')"
echo ""
echo "Next steps:"
echo "  1. cd your-git-repo"
echo "  2. agentdiff configure      -- installs git hooks"
echo "  3. agentdiff.site           -- install the GitHub App"
echo ""
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo "Add to PATH if needed:"
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac
