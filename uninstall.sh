#!/usr/bin/env bash
set -e

GIFT_DIR="$HOME/.bts-gift"

if [ -d "$GIFT_DIR" ]; then
    echo "Removing $GIFT_DIR..."
    rm -rf "$GIFT_DIR"
    echo -e "\033[1;32m✓ Birthday Gift card successfully uninstalled!\033[0m"
else
    echo "Birthday Gift card is not installed (~/.bts-gift not found)."
fi
