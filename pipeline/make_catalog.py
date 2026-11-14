#!/usr/bin/env python3
"""Assemble catalog.json from a build's metadata and file checksums.

Every pack job writes <file>.meta.json ({"file", "size", "sha256"}) next to
its upload; this script turns those into the catalog the app reads.
"""
import argparse
import json
import os
from pathlib import Path

ATTRIBUTION = [
    {"text": "© OpenStreetMap contributors", "url": "https://www.openstreetmap.org/copyright", "license": "ODbL-1.0"},
    {"text": "Overture Maps Foundation", "url": "https://docs.overturemaps.org/attribution/", "license": "CDLA-Permissive-2.0 / Apache-2.0 / CC0 / ODbL by source"},
    {"text": "Protomaps", "url": "https://protomaps.com", "license": "basemap: ODbL-1.0 produced work"},
    {"text": "US Census Bureau", "url": "https://www.census.gov", "license": "public domain"},
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--build", default="build/build.json")
    ap.add_argument("--meta", required=True, help="directory of *.meta.json files")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    build = json.loads(Path(args.build).read_text())
    repo = os.environ.get("GITHUB_REPOSITORY", "HandyFox/reflectmaps")
    base_url = f"https://github.com/{repo}/releases/download/{build['tag']}"

    files = {}
    for p in Path(args.meta).rglob("*.meta.json"):
        m = json.loads(p.read_text())
        m["url"] = f"{base_url}/{m['file']}"
        for part in m.get("parts", []):
            part["url"] = f"{base_url}/{part['file']}"
        if m.get("parts"):
            m["url"] = m["parts"][0]["url"]
        files[m["file"]] = m

    def comp(name, kind):
        m = files.get(name)
        if not m:
            return None
        return {**m, "kind": kind}

    regions = []
    for r in build["regions"]:
        components = {}
        for kind, name in (
            ("map", f"{r['id']}-map.pmtiles"),
            ("search", f"{r['id']}-search.tar.gz"),
            ("routing", f"{r['id']}-routing.tar.gz"),
        ):
            c = comp(name, kind)
            if c:
                components[kind] = c
        regions.append({"id": r["id"], "name": r["name"], "bbox": r["bbox"], "components": components})

    catalog = {
        "schema": 1,
        "tag": build["tag"],
        "built": build["date"],
        "sources": {
            "overture_release": build["overture_release"],
            "protomaps_build": build["protomaps_build"],
            "openstreetmap": "Geofabrik state extracts, " + build["date"],
        },
        "routing_graph": build["tag"],
        "base": {k: v for k, v in {
            "world": comp("world-base.pmtiles", "map"),
            "areas": comp("us-areas.tar.gz", "search"),
        }.items() if v},
        "regions": regions,
        "attribution": ATTRIBUTION,
    }
    Path(args.out).write_text(json.dumps(catalog, indent=2))
    # A state is listed only when all three of its packs built.
    incomplete = [r for r in regions if len(r["components"]) < 3]
    catalog["regions"] = [r for r in regions if len(r["components"]) == 3]
    Path(args.out).write_text(json.dumps(catalog, indent=2))
    print(f"{len(catalog['regions'])} complete regions")
    for r in incomplete:
        have = ", ".join(r["components"]) or "nothing"
        print(f"::warning::{r['name']} left out of the catalog (built: {have})")
        summary = os.environ.get("GITHUB_STEP_SUMMARY")
        if summary:
            with open(summary, "a") as f:
                f.write(f"- {r['name']} left out (built: {have})\n")
    if not catalog["regions"]:
        raise SystemExit("no complete regions")


if __name__ == "__main__":
    main()
