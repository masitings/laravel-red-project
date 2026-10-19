#!/usr/bin/env python3
"""Print the MariaDB versions to build, as a JSON list.

    plan-mariadb.py manifest.json 11.4 11.8

For each series, asks mariadb.org for its newest release and leaves it out
when the manifest already has that version for every platform RED builds.
"""

import json
import sys
import urllib.request

PLATFORMS = ["macos-aarch64", "macos-x86_64", "linux-aarch64"]


def latest(series: str) -> str | None:
    url = f"https://downloads.mariadb.org/rest-api/mariadb/{series}/latest/"
    try:
        with urllib.request.urlopen(url, timeout=30) as response:
            releases = json.load(response).get("releases", {})
    except Exception as err:  # noqa: BLE001 - report and skip the series
        print(f"{series}: {err}", file=sys.stderr)
        return None
    return next(iter(releases), None)


def main() -> None:
    manifest_path, *series = sys.argv[1:]
    with open(manifest_path) as f:
        built = {
            (r["version"], r["platform"])
            for r in json.load(f).get("runtimes", [])
            if r.get("kind") == "mariadb"
        }
    wanted = []
    for line in series:
        version = latest(line)
        if version is None:
            continue
        if all((version, p) in built for p in PLATFORMS):
            print(f"MariaDB {version}: already built", file=sys.stderr)
            continue
        wanted.append(version)
    print(json.dumps(wanted))


if __name__ == "__main__":
    main()
