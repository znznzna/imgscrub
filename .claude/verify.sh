#!/bin/sh
# imgscrub 検証契約
#   fast : 書式・lint・ビルド（Stop hook が機械的に強制する）
#   full : fast + テスト（コミット前に実行する）
set -eu

MODE="${1:-fast}"
cd "$(dirname "$0")/.."

CARGO="${CARGO:-cargo}"
command -v "$CARGO" >/dev/null 2>&1 || CARGO=/opt/homebrew/bin/cargo

echo "== fmt =="
"$CARGO" fmt --check
echo "== clippy =="
"$CARGO" clippy --all-targets -- -D warnings
echo "== build =="
"$CARGO" build

if [ "$MODE" = "full" ]; then
  echo "== test =="
  "$CARGO" test
fi

echo "verify.sh $MODE: OK"
