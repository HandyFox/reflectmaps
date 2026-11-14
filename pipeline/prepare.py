#!/usr/bin/env python3
"""Resolve source versions and state outlines for a data build.

Writes:
  <out>/build.json            versions and region list used by every later step
  <out>/boundaries/<id>.geojson   state outline, slightly buffered
  <out>/boundaries/us_states.parquet  all state outlines (for ZIP code lookup)
and, when running in GitHub Actions, step outputs for the workflow.
"""
import argparse
import datetime as dt
import io
import json
import os
import subprocess
import sys
import urllib.request
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
UA = {"User-Agent": "reflectmaps-data-build (+https://github.com/HandyFox/reflectmaps)"}


def http_get(url: str, timeout=120) -> bytes:
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read()


def http_ok(url: str) -> bool:
    try:
        req = urllib.request.Request(url, method="HEAD", headers=UA)
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status == 200
    except Exception:
        return False


def latest_overture_release() -> str:
    try:
        data = json.loads(http_get("https://labs.overturemaps.org/data/releases.json"))
        rel = data.get("latest") or data["releases"][0]
        if rel:
            return rel
    except Exception as e:  # fall back to listing the bucket
        print(f"releases.json unavailable ({e}); listing the bucket", file=sys.stderr)
    out = subprocess.run(
        ["aws", "s3", "ls", "--no-sign-request", "s3://overturemaps-us-west-2/release/"],
        check=True, capture_output=True, text=True,
    ).stdout
    rels = sorted(line.split()[-1].strip("/") for line in out.splitlines() if "PRE" in line)
    return rels[-1]


def latest_protomaps_build() -> str:
    today = dt.date.today()
    for back in range(0, 21):
        d = (today - dt.timedelta(days=back)).strftime("%Y%m%d")
        if http_ok(f"https://build.protomaps.com/{d}.pmtiles"):
            return d
    raise SystemExit("no Protomaps daily build found in the last 21 days")


def census_states(dest: Path) -> Path:
    """Download the Census cartographic state outlines (public domain)."""
    for year in (2024, 2023, 2022):
        url = f"https://www2.census.gov/geo/tiger/GENZ{year}/shp/cb_{year}_us_state_500k.zip"
        try:
            blob = http_get(url, timeout=300)
        except Exception as e:
            print(f"{url}: {e}", file=sys.stderr)
            continue
        zipfile.ZipFile(io.BytesIO(blob)).extractall(dest)
        shp = next(dest.glob(f"cb_{year}_us_state_500k.shp"))
        print(f"state outlines: {url}")
        return shp
    raise SystemExit("could not download Census state outlines")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--regions", default="oh,ky")
    ap.add_argument("--out", default="build")
    args = ap.parse_args()

    out = Path(args.out)
    (out / "boundaries").mkdir(parents=True, exist_ok=True)
    catalog = json.loads((HERE / "regions.json").read_text())["regions"]
    by_id = {r["id"]: r for r in catalog}
    if args.regions.strip().lower() == "all":
        ids = [r["id"] for r in catalog]
    else:
        ids = [x.strip().lower() for x in args.regions.split(",") if x.strip()]
    missing = [i for i in ids if i not in by_id]
    if missing:
        raise SystemExit(f"unknown regions {missing}; add them to pipeline/regions.json")
    regions = [by_id[i] for i in ids]

    overture = latest_overture_release()
    protomaps = latest_protomaps_build()
    print(f"Overture release {overture}, Protomaps build {protomaps}")

    sys.path.insert(0, str(HERE))
    from common import connect

    con = connect()
    shp = census_states(out / "boundaries" / "census")
    con.execute(
        f"CREATE TABLE states AS SELECT STUSPS AS usps, NAME AS name, geom FROM ST_Read('{shp}')"
    )
    con.execute(
        f"COPY (SELECT usps, name, ST_AsHEXWKB(geom) AS wkb_hex FROM states) TO '{out}/boundaries/us_states.parquet' (FORMAT PARQUET)"
    )
    for r in regions:
        # Buffered slightly so packs overlap at state lines. Clipped to the
        # western hemisphere: Alaska's outermost Aleutian islands lie past
        # 180 degrees and would otherwise make its box span the globe.
        row = con.execute(
            """
            WITH g AS (
              SELECT ST_Intersection(ST_Buffer(geom, 0.02), ST_MakeEnvelope(-180, -90, 0, 90)) AS geom
              FROM states WHERE usps = ?
            )
            SELECT ST_AsGeoJSON(geom), ST_XMin(geom), ST_YMin(geom), ST_XMax(geom), ST_YMax(geom) FROM g
            """,
            [r["usps"]],
        ).fetchone()
        if not row:
            raise SystemExit(f"no Census outline for {r['usps']}")
        r["bbox"] = [round(row[1], 3), round(row[2], 3), round(row[3], 3), round(row[4], 3)]
        r.setdefault("geofabrik", "north-america/us/" + r["name"].lower().replace(" ", "-"))
        feature = {"type": "Feature", "properties": {"id": r["id"]}, "geometry": json.loads(row[0])}
        (out / "boundaries" / f"{r['id']}.geojson").write_text(json.dumps(feature))

    stamp = dt.datetime.utcnow().strftime("%Y%m%d")
    run = os.environ.get("GITHUB_RUN_NUMBER", "local")
    build = {
        "tag": f"data-{stamp}-{run}",
        "date": dt.date.today().isoformat(),
        "overture_release": overture,
        "protomaps_build": protomaps,
        "regions": regions,
    }
    (out / "build.json").write_text(json.dumps(build, indent=2))
    gh_out = os.environ.get("GITHUB_OUTPUT")
    if gh_out:
        with open(gh_out, "a") as f:
            f.write(f"tag={build['tag']}\n")
            f.write(f"overture={overture}\n")
            f.write(f"protomaps={protomaps}\n")
            f.write(f"regions={json.dumps(ids)}\n")
    print(json.dumps(build, indent=2))


if __name__ == "__main__":
    main()
