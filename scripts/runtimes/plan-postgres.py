#!/usr/bin/env python3
"""Print the PostgreSQL versions to build, as a JSON list.

    plan-postgres.py manifest.json 16 17 18

For each major, takes its newest release from postgresql.org's versions.json
and leaves it out when the manifest already has that version for every
platform RED builds.
"""

import json
import sys
import urllib.request

PLATFORMS = ["macos-aarch64", "macos-x86_64", "linux-aarch64", "linux-x86_64"]
VERSIONS_URL = "https://www.postgresql.org/versions.json"


def latest() -> dict[str, str]:
    with urllib.request.urlopen(VERSIONS_URL, timeout=30) as response:
        majors = json.load(response)
    return {str(m["major"]): f'{m["major"]}.{m["latestMinor"]}' for m in majors}


def main() -> None:
    manifest_path, *majors = sys.argv[1:]
    if not majors:
        print("[]")
        return
    with open(manifest_path) as f:
        built = {
            (r["version"], r["platform"])
            for r in json.load(f).get("runtimes", [])
            if r.get("kind") == "postgres"
        }
    newest = latest()
    wanted = []
    for major in majors:
        version = newest.get(major)
        if version is None:
            print(f"PostgreSQL {major}: unknown major", file=sys.stderr)
            continue
        if all((version, p) in built for p in PLATFORMS):
            print(f"PostgreSQL {version}: already built", file=sys.stderr)
            continue
        wanted.append(version)
    print(json.dumps(wanted))


if __name__ == "__main__":
    main()
