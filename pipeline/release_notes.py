#!/usr/bin/env python3
"""Write the release notes for a data build: what is in it and which files
to download for each state, for people copying packs to a computer with no
internet (the app's Downloads > Import folder).

Usage: release_notes.py catalog.json > notes.md
"""
import json
import sys


def size(n: int) -> str:
    for unit, div in (("GB", 1e9), ("MB", 1e6)):
        if n >= div:
            return f"{n / div:.1f} {unit}"
    return f"{n / 1e3:.0f} KB"


def links(c: dict) -> str:
    parts = c.get("parts") or []
    if parts:
        return " + ".join(f"[{p['file']}]({p['url']})" for p in parts)
    return f"[{c['file']}]({c['url']})"


def main():
    cat = json.load(open(sys.argv[1]))
    src = cat.get("sources", {})
    base = f"https://github.com/HandyFox/reflectmaps/releases/download/{cat['tag']}"
    out = [
        f"Map, search and directions packs for {len(cat['regions'])} states, built {cat['built']}.",
        f"Overture {src.get('overture_release', '?')}, Protomaps {src.get('protomaps_build', '?')}, OpenStreetMap via Geofabrik.",
        "",
        "The app downloads these by itself from **Downloads**. To install them on a computer without internet:",
        "",
        f"1. Download [catalog.json]({base}/catalog.json) and the files for the states you want (table below).",
        "   Files over GitHub's 2 GB limit come in parts (`.part00`, `.part01`, ...): download every part.",
        "2. Copy them all into one folder on a USB drive.",
        "3. In ReflectMaps open **Downloads**, choose **Import folder...** and pick that folder (or drag the files onto the window).",
        "   catalog.json lets the app check each file for damage.",
        "",
        "| State | Map | Search | Directions | Total |",
        "|---|---|---|---|---|",
    ]
    for r in sorted(cat["regions"], key=lambda r: r["name"]):
        comps = r["components"]
        cells = []
        for k in ("map", "search", "routing"):
            c = comps.get(k)
            cells.append(f"{links(c)} ({size(c['size'])})" if c else "")
        total = sum(c["size"] for c in comps.values())
        out.append(f"| {r['name']} | " + " | ".join(cells) + f" | {size(total)} |")
    print("\n".join(out))


if __name__ == "__main__":
    main()
