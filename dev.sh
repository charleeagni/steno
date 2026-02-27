#!/bin/bash

# Move to the root of the project to ensure correct relative paths
cd "$(dirname "$0")"

LOCK_FILE="src-tauri/target/debug/.cargo-lock"

# 1. Clean up stale cargo locks if unused
if [ -f "$LOCK_FILE" ]; then
    # Check if a process is using it on macOS/Linux
    if lsof "$LOCK_FILE" > /dev/null 2>&1; then
        echo "⚠️  Cargo lock is currently active. Another dev session or build might be running."
    else
        echo "🧹 Found a stale cargo lock. Removing it to prevent bun/tauri timeouts..."
        rm -f "$LOCK_FILE"
    fi
fi

# 2. Run the Tauri dev server explicitly with bun
echo "🚀 Starting Tauri dev via Bun..."
exec bun run tauri dev
