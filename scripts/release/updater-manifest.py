#!/usr/bin/env python3
"""Write latest.json, the update manifest the desktop app polls.

    updater-manifest.py <version> <notes.md> <download-base-url> <dist-dir> > latest.json

Looks in <dist-dir> for RED-<tag>-macos-<arch>.app.tar.gz plus its .sig
(made by `tauri build` with createUpdaterArtifacts) and lists one platform
per archive, in the format of Tauri's updater plugin.
"""
import datetime
import json
import pathlib
import re
import sys

ARCHES = {"aarch64": "darwin-aarch64", "x86_64": "darwin-x86_64"}


def main() -> None:
    version, notes_path, base_url, dist = sys.argv[1:]
    platforms = {}
    for archive in sorted(pathlib.Path(dist).glob("RED-*-macos-*.app.tar.gz")):
        match = re.search(r"-macos-([a-z0-9_]+)\.app\.tar\.gz$", archive.name)
        if not match or match.group(1) not in ARCHES:
            continue
        signature = archive.with_name(archive.name + ".sig")
        if not signature.is_file():
            sys.exit(f"{signature} is missing")
        platforms[ARCHES[match.group(1)]] = {
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
