#!/usr/bin/env bash
# Removes Gapir, its speech model, settings, logs and granted permissions.
set -euo pipefail

BUNDLE_ID="uz.salikhov.gapir"

pkill -x gapir 2>/dev/null || true

rm -rf "/Applications/Gapir.app" "$HOME/Applications/Gapir.app"
rm -f "$HOME/Library/LaunchAgents/Gapir.plist"
rm -rf "$HOME/Library/Application Support/$BUNDLE_ID"
rm -rf "$HOME/Library/Caches/$BUNDLE_ID"
rm -rf "$HOME/Library/WebKit/$BUNDLE_ID"
rm -rf "$HOME/Library/Logs/Gapir"
tccutil reset All "$BUNDLE_ID" >/dev/null 2>&1 || true

echo "Gapir oʻchirildi."
