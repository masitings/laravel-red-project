#!/usr/bin/env python3
"""Merge freshly built runtime entries into RED's runtime manifest.

    merge-manifest.py manifest.json <base-url> entries/*.json

Each entry file holds one JSON object written by build-php.sh or
build-valkey.sh. An entry replaces the one with the same kind, line and
platform, unless that one is a newer version (a build that picked up an
older source must not downgrade the manifest). The result is written back to manifest.json.
"""
import json
import re
import sys


def version_key(version: str) -> list[int]:
    return [int(p) for p in re.findall(r"\d+", version)]


def main() -> None:
    manifest_path, base_url, *entry_paths = sys.argv[1:]
    try:
        with open(manifest_path) as f:
            manifest = json.load(f)
    except (FileNotFoundError, json.JSONDecodeError):
        manifest = {"schema": 1, "runtimes": []}

    runtimes = manifest.get("runtimes", [])
    for path in entry_paths:
        with open(path) as f:
            entry = json.load(f)
        entry["url"] = f"{base_url.rstrip('/')}/{entry.pop('file')}"
        key = (entry["kind"], entry["line"], entry["platform"])
        current = next((r for r in runtimes if (r["kind"], r["line"], r["platform"]) == key), None)
        if current and version_key(current["version"]) > version_key(entry["version"]):
            print(f"{entry['kind']} {entry['version']} {entry['platform']}: keeping {current['version']}")
            continue
        runtimes = [r for r in runtimes if r is not current]
        runtimes.append(entry)
        print(f"{entry['kind']} {entry['version']} {entry['platform']}")

    runtimes.sort(key=lambda r: (r["kind"], [int(p) for p in r["line"].split(".")], r["platform"]))
    manifest = {"schema": 1, "runtimes": runtimes}
    with open(manifest_path, "w") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")


if __name__ == "__main__":
    main()
