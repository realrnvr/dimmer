#!/usr/bin/env python3
"""Generate Flatpak's offline Cargo vendor sources from Cargo.lock."""

from __future__ import annotations

import json
import sys
import tomllib
from pathlib import Path


PROJECT_DIR = Path(__file__).resolve().parent.parent
LOCKFILE = PROJECT_DIR / "Cargo.lock"
OUTPUT = PROJECT_DIR / "flatpak" / "cargo-sources.json"


def main() -> int:
    packages = tomllib.loads(LOCKFILE.read_text(encoding="utf-8")).get("package", [])
    sources: list[dict[str, str]] = []

    for package in packages:
        if not package.get("source", "").startswith("registry+"):
            continue
        name = package["name"]
        version = package["version"]
        checksum = package.get("checksum")
        if not checksum:
            raise SystemExit(f"missing registry checksum for {name} {version}")

        crate = f"{name}-{version}"
        sources.append(
            {
                "type": "archive",
                "archive-type": "tar-gzip",
                "url": f"https://static.crates.io/crates/{name}/{crate}.crate",
                "sha256": checksum,
                "dest": f"cargo/vendor/{crate}",
            }
        )
        sources.append(
            {
                "type": "inline",
                "contents": json.dumps({"package": checksum, "files": {}}),
                "dest": f"cargo/vendor/{crate}",
                "dest-filename": ".cargo-checksum.json",
            }
        )

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(json.dumps(sources, indent=2) + "\n", encoding="utf-8")
    print(f"Wrote {len(sources)} Flatpak Cargo sources to {OUTPUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
