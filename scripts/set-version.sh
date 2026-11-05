#!/usr/bin/env bash
# Writes the version of a release into every file that carries it, so the app, Cargo.lock
# and the git tag agree. The app compares its own version with the newest release tag.
#
#   scripts/set-version.sh          the next release: one patch after the newest vX.Y.Z tag,
#                                   or the version in src-tauri/Cargo.toml when someone has
#                                   already raised it past that tag, for a minor or major bump
#   scripts/set-version.sh 0.2.0    exactly this version
#
# Prints the chosen version on stdout.
set -euo pipefail

cd "$(dirname "$0")/.."

# Orders X.Y.Z versions as numbers; every part is expected to stay below 1000.
weight() {
    local major minor patch
    IFS=. read -r major minor patch <<<"$1"
    echo $((10#$major * 1000000 + 10#$minor * 1000 + 10#$patch))
}

if [ $# -ge 1 ]; then
    version="$1"
else
    current="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' src-tauri/Cargo.toml | head -n 1)"
    latest="$(git tag --list 'v*' --sort=-v:refname | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | head -n 1 || true)"
    latest="${latest#v}"
    if [ -z "$latest" ] || [ "$(weight "$current")" -gt "$(weight "$latest")" ]; then
        version="$current"
    else
        IFS=. read -r major minor patch <<<"$latest"
        version="$major.$minor.$((patch + 1))"
    fi
fi

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
    echo "set-version: \"$version\" is not X.Y.Z" >&2
    exit 1
}

# The first `version = "..."` of a manifest is the package's own.
for manifest in core/Cargo.toml src-tauri/Cargo.toml; do
    VERSION="$version" perl -0777 -pi -e 's/^version = "[^"]*"/version = "$ENV{VERSION}"/m' "$manifest"
    grep -qx "version = \"$version\"" "$manifest" || {
        echo "set-version: $manifest was not updated" >&2
        exit 1
    }
done
VERSION="$version" perl -0777 -pi -e 's/^(\s*"version":\s*)"[^"]*"/$1"$ENV{VERSION}"/m' src-tauri/tauri.conf.json
grep -q "\"version\": \"$version\"" src-tauri/tauri.conf.json || {
    echo "set-version: src-tauri/tauri.conf.json was not updated" >&2
    exit 1
}

# Cargo.lock lists the workspace crates with their versions.
cargo update --workspace --quiet

echo "$version"
