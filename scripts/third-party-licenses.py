#!/usr/bin/env python3
"""Writes THIRD-PARTY-LICENSES.txt: the license text of everything compiled into Gapir.

MIT and BSD ask for their text to travel with binary copies, and MPL-2.0 asks recipients to be
told where the source is. The list comes from `cargo metadata`, so it stays correct as
dependencies change. Identical texts are merged to keep the file readable.

Usage: python3 scripts/third-party-licenses.py > THIRD-PARTY-LICENSES.txt
"""

import json
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

TARGET = "aarch64-apple-darwin"
LICENSE_FILE = re.compile(r"^(licen[cs]e|copying|unlicense)", re.IGNORECASE)
# Multi-licensed packages are used under the first of these they offer.
PREFERRED = ("mit", "apache", "bsd", "zlib", "isc")

MIT_TEXT = """MIT License

Copyright (c) {authors}

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
"""


def shipped_packages():
    """External packages reachable through normal dependencies on the release target."""
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--format-version", "1", "--filter-platform", TARGET]
        )
    )
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    seen = set()
    pending = list(metadata["workspace_members"])
    while pending:
        package_id = pending.pop()
        if package_id in seen:
            continue
        seen.add(package_id)
        for dependency in nodes[package_id]["deps"]:
            if any(kind["kind"] is None for kind in dependency["dep_kinds"]):
                pending.append(dependency["pkg"])
    shipped = [packages[package_id] for package_id in seen if packages[package_id]["source"]]
    return sorted(shipped, key=lambda package: (package["name"], package["version"]))


def license_files(package):
    root = Path(package["manifest_path"]).parent
    return sorted(
        path for path in root.iterdir() if path.is_file() and LICENSE_FILE.match(path.name)
    )


def chosen_files(package):
    """One file for "A OR B" licenses, every file for "A AND B"."""
    files = license_files(package)
    expression = package.get("license") or ""
    if " AND " in expression or "OR" not in expression.replace("/", " OR "):
        return files
    for wanted in PREFERRED:
        matching = [path for path in files if wanted in path.name.lower()]
        if matching:
            return matching[:1]
    return files


def fallback_text(package, shipped):
    """Text for packages published without a license file."""
    expression = package.get("license") or ""
    if "MIT" in expression:
        authors = ", ".join(re.sub(r"\s*<.*?>", "", a) for a in package["authors"])
        return MIT_TEXT.format(authors=authors or f"the {package['name']} authors")
    for other in shipped:
        same_repo = other.get("repository") == package.get("repository")
        same_license = other.get("license") == expression
        if other is not package and (same_repo or same_license) and license_files(other):
            return "\n\n".join(p.read_text(errors="replace") for p in chosen_files(other))
    raise SystemExit(f"no license text for {package['name']} ({expression})")


def license_text(package, shipped):
    files = chosen_files(package)
    if files:
        return "\n\n".join(path.read_text(errors="replace").strip() for path in files)
    return fallback_text(package, shipped).strip()


def main():
    shipped = shipped_packages()
    groups = defaultdict(list)
    for package in shipped:
        groups[license_text(package, shipped)].append(package)

    line = "=" * 78
    print("Third-party software included in Gapir")
    print()
    print("The source code of every package below is available from the repository shown")
    print("next to it, and of Rust packages also from https://crates.io.")
    print(f"Packages: {len(shipped)}")

    for text, packages in sorted(groups.items(), key=lambda group: group[1][0]["name"]):
        print()
        print(line)
        for package in packages:
            where = package.get("repository") or "https://crates.io"
            print(f"{package['name']} {package['version']} ({package.get('license')}) {where}")
        print(line)
        print()
        print(text)

    bundled = next(package for package in shipped if package["name"] == "transcribe-cpp-sys")
    notices = Path(bundled["manifest_path"]).parent / "THIRD-PARTY-LICENSES.md"
    print()
    print(line)
    print("C and C++ components compiled into transcribe.cpp")
    print(line)
    print()
    print(notices.read_text())


if __name__ == "__main__":
    sys.exit(main())
