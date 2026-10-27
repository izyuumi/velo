#!/bin/zsh
# Start the video_hosted server detached, listening on 127.0.0.1:35773.
# Run this from a Terminal so the process inherits Terminal's TCC grant for
# ~/Downloads -- a launchd agent cannot get that grant and hangs at startup.
set -e

cd "$(dirname "$0")"
ROOT="$PWD"

if lsof -nP -iTCP:35773 -sTCP:LISTEN >/dev/null 2>&1; then
  echo "Already listening on 35773; nothing to do."
  exit 0
fi

mkdir -p "$ROOT/data/logs"

ROCKET_PROFILE=release \
ROCKET_ADDRESS=127.0.0.1 \
ROCKET_PORT=35773 \
DATA_DIR="$ROOT/data" \
FRONTEND_DIST_DIR="$ROOT/frontend/dist" \
FFMPEG_MAX_WORKERS=2 \
PATH="/opt/homebrew/bin:$PATH" \
nohup "$ROOT/target/release/video_hosted" \
  >> "$ROOT/data/logs/server.out.log" 2>&1 &

echo "Started pid $! -- logs at data/logs/server.out.log"
