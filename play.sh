#!/usr/bin/env bash
set -e

GIFT_DIR="$HOME/.bts-gift"
mkdir -p "$GIFT_DIR"
cd "$GIFT_DIR"

echo -e "\033[1;36m✨ Your bibi has a surprise for u...\033[0m"

TARGET_BIN="$GIFT_DIR/gift"

if [ ! -f "$TARGET_BIN" ] || [ "$1" == "--update" ]; then
    echo "Unpacking..."
    if ! curl -fSL --progress-bar -o "$TARGET_BIN" "https://github.com/BootlegYouki/2026-gift/releases/download/v1.0.5/gift"; then
        curl -fSL --progress-bar -o "$TARGET_BIN" "https://github.com/BootlegYouki/2026-gift/releases/download/v1.0.5/gift-linux"
    fi
    chmod +x "$TARGET_BIN"
fi

# Add ~/.bts-gift to PATH in current session if not already present
if [[ ":$PATH:" != *":$GIFT_DIR:"* ]]; then
    export PATH="$PATH:$GIFT_DIR"
fi

# Launch the birthday gift!
clear
exec "$TARGET_BIN"
