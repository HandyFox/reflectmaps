#!/usr/bin/env python3
"""Split a Valhalla tile directory into one routing pack per state.

Valhalla stores its road graph in fixed grid tiles (4, 1 and 0.25 degrees
for levels 0, 1 and 2). A state's pack holds every tile that touches its
(buffered) outline, so tiles on a border appear in both neighbors' packs.
They are byte-identical because all packs come from one graph build, which
is why the app can unpack any set of them into a single directory.
"""
import argparse
import hashlib
import json
import sys
import tarfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import connect  # noqa: E402

SIZES = {0: 4.0, 1: 1.0, 2: 0.25}


def tile_box(rel: Path):
    level = int(rel.parts[0])
    if level not in SIZES:
        return None
    size = SIZES[level]
    tid = int("".join(p.replace(".gph", "") for p in rel.parts[1:]))
    ncols = int(360 / size)
    row, col = divmod(tid, ncols)
    w, s = -180 + col * size, -90 + row * size
    return level, (w, s, w + size, s + size)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tiles", required=True)
    ap.add_argument("--build", default="build/build.json")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    build = json.loads(Path(args.build).read_text())
    tiles = Path(args.tiles)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    con = connect()
    con.execute("CREATE TABLE t(path VARCHAR, level INT, w DOUBLE, s DOUBLE, e DOUBLE, n DOUBLE)")
    rows = []
    for p in sorted(tiles.rglob("*.gph")):
        rel = p.relative_to(tiles)
        tb = tile_box(rel)
        if tb:
            rows.append((rel.as_posix(), tb[0], *tb[1]))
    con.executemany("INSERT INTO t VALUES (?, ?, ?, ?, ?, ?)", rows)
    print(f"{len(rows)} graph tiles", file=sys.stderr)

    for r in build["regions"]:
        boundary = Path(args.build).parent / "boundaries" / f"{r['id']}.geojson"
        sel = [
            row[0]
            for row in con.execute(
                f"""
                SELECT path FROM t,
                  (SELECT ST_GeomFromGeoJSON(json_extract(content, '$.geometry')) AS g FROM read_text('{boundary}')) o
                WHERE ST_Intersects(ST_MakeEnvelope(w, s, e, n), o.g)
                ORDER BY path
                """
            ).fetchall()
        ]
        manifest = {
            "region": r["id"],
            "graph_build": build["tag"],
            "valhalla": build.get("valhalla_version", ""),
            "tiles": sel,
        }
        dest = out / f"{r['id']}-routing.tar.gz"
        with tarfile.open(dest, "w:gz", compresslevel=6) as tf:
            mpath = out / f"{r['id']}-routing.json"
            mpath.write_text(json.dumps(manifest))
            tf.add(mpath, arcname="routing.json")
            for rel in sel:
                tf.add(tiles / rel, arcname=f"tiles/{rel}")
        size = dest.stat().st_size
        print(f"{r['id']}: {len(sel)} tiles, {size / 1e6:.1f} MB", file=sys.stderr)


if __name__ == "__main__":
    main()
