#!/usr/bin/env bash

set -euo pipefail

make site

cd docs/
python3 -m http.server "${PORT:-8000}" &
http_server_pid="$!"
trap 'kill "$http_server_pid"' EXIT

cd -

if [ "$(uname)" = "Darwin" ]; then
  open "http://127.0.0.1:8000"
else
  xdg-open "http://127.0.0.1:8000"
fi

watchman-make \
  -p 'site/src/**' 'site/public/**' 'assets/**' 'Makefile' 'Cargo.toml' \
  -r 'make site'
