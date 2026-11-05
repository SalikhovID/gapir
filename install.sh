#!/usr/bin/env bash
# Installs or updates Gapir: fetches the app and the speech model, then opens the app.
# Safe to run again: an interrupted model download resumes, settings and permissions stay.
set -euo pipefail

REPO="SalikhovID/gapir"
BUNDLE_ID="uz.salikhov.gapir"
APP="Gapir.app"
ARCHIVE="Gapir-macos-arm64.zip"
RELEASE_URL="https://github.com/$REPO/releases/latest/download"

MODEL_FILE="rubaistt-v2-medium-Q5_K_M.gguf"
MODEL_URL="https://github.com/$REPO/releases/download/model-rubaistt-v2-q5km/$MODEL_FILE"
MODEL_SIZE=582746144
MODEL_SHA256="2ed32a824e45feb8ee84c57576068e0b90dbf92f5781271b17bb8dc5ac9d77b9"
MODEL_DIR="$HOME/Library/Application Support/$BUNDLE_ID/models"

WORK_DIR=""

cleanup() {
    if [ -n "$WORK_DIR" ]; then
        rm -rf "$WORK_DIR"
    fi
}
trap cleanup EXIT

fail() {
    printf '\nXato: %s\n' "$1" >&2
    exit 1
}

file_size() {
    stat -f %z "$1" 2>/dev/null || echo 0
}

check_system() {
    [ "$(uname -s)" = "Darwin" ] || fail "Gapir hozircha faqat macOS'da ishlaydi."
    [ "$(uname -m)" = "arm64" ] || fail "Gapir faqat Apple Silicon (M1 va undan yangi) Mac'larda ishlaydi."
    local major
    major="$(sw_vers -productVersion | cut -d. -f1)"
    [ "$major" -ge 13 ] || fail "macOS 13 yoki undan yangisi kerak."
}

install_app() {
    local target
    WORK_DIR="$(mktemp -d)"

    echo "Ilova yuklanmoqda..."
    (
        cd "$WORK_DIR"
        curl -fL --retry 3 --progress-bar -o "$ARCHIVE" "$RELEASE_URL/$ARCHIVE"
        curl -fsSL --retry 3 -o "$ARCHIVE.sha256" "$RELEASE_URL/$ARCHIVE.sha256"
        shasum -a 256 -c "$ARCHIVE.sha256" >/dev/null
    ) || fail "Ilovani yuklab boʻlmadi. Internetni tekshirib, commandni qayta ishga tushiring."
    ditto -x -k "$WORK_DIR/$ARCHIVE" "$WORK_DIR/unpacked"

    target="/Applications"
    if [ ! -w "$target" ]; then
        target="$HOME/Applications"
        mkdir -p "$target"
    fi

    pkill -x gapir 2>/dev/null || true
    for _ in 1 2 3 4 5 6; do
        pgrep -x gapir >/dev/null || break
        sleep 0.5
    done

    rm -rf "${target:?}/$APP"
    mv "$WORK_DIR/unpacked/$APP" "$target/$APP"
    xattr -dr com.apple.quarantine "$target/$APP" 2>/dev/null || true
    APP_PATH="$target/$APP"
}

install_model() {
    local model="$MODEL_DIR/$MODEL_FILE" part="$MODEL_DIR/$MODEL_FILE.part"
    mkdir -p "$MODEL_DIR"
    if [ "$(file_size "$model")" = "$MODEL_SIZE" ]; then
        echo "Model joyida."
        return
    fi

    if [ "$(file_size "$part")" -gt "$MODEL_SIZE" ]; then
        rm -f "$part"
    fi
    if [ "$(file_size "$part")" != "$MODEL_SIZE" ]; then
        echo "Model yuklanmoqda, taxminan $((MODEL_SIZE / 1000000)) MB..."
        curl -fL --retry 5 --retry-delay 3 -C - --progress-bar -o "$part" "$MODEL_URL" ||
            fail "Model toʻliq yuklanmadi. Commandni qayta ishga tushiring, yuklash davom etadi."
    fi

    echo "Model tekshirilmoqda..."
    if ! echo "$MODEL_SHA256  $part" | shasum -a 256 -c - >/dev/null 2>&1; then
        rm -f "$part"
        fail "Model fayli buzilgan. Commandni qayta ishga tushiring."
    fi
    mv "$part" "$model"
}

main() {
    check_system
    install_app
    install_model
    open "$APP_PATH"
    cat <<MESSAGE

Gapir oʻrnatildi: $APP_PATH

Ochilgan oynada mikrofon va Accessibility ruxsatini bering.
Keyin istalgan matn maydonida fn tugmasini bosib turing va gapiring.
Oʻchirish uchun: curl -fsSL https://raw.githubusercontent.com/$REPO/main/uninstall.sh | bash
MESSAGE
}

main
