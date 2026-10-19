#!/usr/bin/env python3
"""Write latest.json, the update manifest the desktop app polls.

    updater-manifest.py <version> <notes.md> <download-base-url> <dist-dir> > latest.json

Looks in <dist-dir> for the update archives `tauri build` makes with
createUpdaterArtifacts (RED-<tag>-macos-<arch>.app.tar.gz, and the Linux
.AppImage and .deb themselves), each with its .sig, and lists one
platform per archive in the format of Tauri's updater plugin.
"""
import datetime
import json
import pathlib
import re
import sys

# File name pattern → updater platform key.
# Linux keys carry the bundle type: the updater looks for
# `{os}-{arch}-{bundle_type}` first, so a .deb install never gets the
# AppImage.
PATTERNS = [
    (re.compile(r"-macos-(aarch64|x86_64)\.app\.tar\.gz$"), "darwin-{}"),
    (re.compile(r"-linux-(aarch64|x86_64)\.AppImage$"), "linux-{}-appimage"),
    (re.compile(r"-linux-(aarch64|x86_64)\.deb$"), "linux-{}-deb"),
]


def main() -> None:
    version, notes_path, base_url, dist = sys.argv[1:]
    platforms = {}
    for archive in sorted(pathlib.Path(dist).glob("RED-*")):
        platform = None
        for pattern, key in PATTERNS:
            match = pattern.search(archive.name)
            if match:
                platform = key.format(match.group(1))
        if platform is None:
            continue
        signature = archive.with_name(archive.name + ".sig")
        if not signature.is_file():
            # Built without an updater key (or a bundle Tauri does not sign).
            continue
        platforms[platform] = {
            "signature": signature.read_text().strip(),
            "url": f"{base_url.rstrip('/')}/{archive.name}",
        }
    if not platforms:
        sys.exit(f"no updater archives in {dist}")
    notes = pathlib.Path(notes_path).read_text().strip()
    json.dump(
        {
            "version": version.removeprefix("v"),
            "notes": notes,
            "pub_date": datetime.datetime.now(datetime.timezone.utc)
            .replace(microsecond=0)
            .isoformat()
            .replace("+00:00", "Z"),
            "platforms": platforms,
        },
        sys.stdout,
        indent=2,
    )
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
