#!/usr/bin/env bash

set -euo pipefail

usage() {
  echo "usage: $0 <source.md> <dest.html>" >&2
}

# ----- args and setup -----

src="${1:-}"
dest="${2:-}"
if [ "$src" = "" ] || [ "$dest" = "" ]; then
  usage
  exit 1
fi

case "$src" in
  -h|--help)
    usage
    exit
    ;;
esac

dest_dir="$(dirname "$dest")"
mkdir -p "$dest_dir"

if [ -n "${MDR_BIN:-}" ]; then
  if command -v "${MDR_BIN}" >/dev/null 2>&1; then
    mdr_bin="$(command -v "${MDR_BIN}")"
  elif [ -x "${MDR_BIN}" ]; then
    mdr_bin="${MDR_BIN}"
  else
    echo "$0: MDR_BIN is set but not executable: ${MDR_BIN}" >&2
    exit 1
  fi
fi

if [ -z "${mdr_bin:-}" ]; then
  for candidate in "./target/debug/mdr" "./target/release/mdr" "mdr"; do
    if command -v "$candidate" >/dev/null 2>&1; then
      mdr_bin="$(command -v "$candidate")"
      break
    elif [ -x "$candidate" ]; then
      mdr_bin="$candidate"
      break
    fi
  done
fi

if [ -z "${mdr_bin:-}" ]; then
  echo "$0: mdr binary not found. Run 'make build' first or set MDR_BIN=/path/to/mdr" >&2
  exit 1
fi

resource_path="${PANDOC_RESOURCE_PATH:-site/public:$(dirname "$src")}"
PANDOC_RESOURCE_PATH="$resource_path" "$mdr_bin" "$src" "$dest"
