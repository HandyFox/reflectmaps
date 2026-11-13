#!/usr/bin/env python3
"""Extract search records from Overture Maps (and Census ZIP areas).

Two modes:
  state  places, addresses and streets inside one state's outline
  areas  the national list of states, counties, cities, neighborhoods and ZIP codes

Output is newline-delimited JSON in the shape `rm-search-build` reads.
"""
import argparse
import io
import json
import sys
import urllib.request
import zipfile
from pathlib import Path

import duckdb

import os

# Overridable so the extraction can be tested against local parquet files.
S3 = os.environ.get("RM_OVERTURE_ROOT", "s3://overturemaps-us-west-2/release/{rel}") + "/theme={theme}/type={type}/*"
UA = {"User-Agent": "reflectmaps-data-build (+https://github.com/HandyFox/reflectmaps)"}

STATE_NAMES = {
    "AL": "Alabama", "AK": "Alaska", "AZ": "Arizona", "AR": "Arkansas", "CA": "California",
    "CO": "Colorado", "CT": "Connecticut", "DE": "Delaware", "DC": "District of Columbia",
    "FL": "Florida", "GA": "Georgia", "HI": "Hawaii", "ID": "Idaho", "IL": "Illinois",
    "IN": "Indiana", "IA": "Iowa", "KS": "Kansas", "KY": "Kentucky", "LA": "Louisiana",
    "ME": "Maine", "MD": "Maryland", "MA": "Massachusetts", "MI": "Michigan", "MN": "Minnesota",
    "MS": "Mississippi", "MO": "Missouri", "MT": "Montana", "NE": "Nebraska", "NV": "Nevada",
    "NH": "New Hampshire", "NJ": "New Jersey", "NM": "New Mexico", "NY": "New York",
    "NC": "North Carolina", "ND": "North Dakota", "OH": "Ohio", "OK": "Oklahoma", "OR": "Oregon",
    "PA": "Pennsylvania", "RI": "Rhode Island", "SC": "South Carolina", "SD": "South Dakota",
    "TN": "Tennessee", "TX": "Texas", "UT": "Utah", "VT": "Vermont", "VA": "Virginia",
    "WA": "Washington", "WV": "West Virginia", "WI": "Wisconsin", "WY": "Wyoming",
    "PR": "Puerto Rico",
}


sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import connect  # noqa: E402


def columns(con, path: str) -> dict:
    rows = con.execute(f"DESCRIBE SELECT * FROM read_parquet('{path}', hive_partitioning=1) LIMIT 0").fetchall()
    return {r[0]: r[1] for r in rows}


def bbox_where(b):
    w, s, e, n = b
    return (f"bbox.xmin <= {e} AND bbox.xmax >= {w} AND bbox.ymin <= {n} AND bbox.ymax >= {s}")


def state_mode(con, rel, region, boundary: Path, out: Path):
    usps = region["usps"]
    con.execute(
        f"CREATE TABLE outline AS SELECT ST_GeomFromGeoJSON(json_extract(content, '$.geometry')) AS geom "
        f"FROM read_text('{boundary}')"
    )
    where = bbox_where(region["bbox"])
    inside = "ST_Within(ST_Point(lon, lat), (SELECT geom FROM outline))"

    # ---- places ----
    ppath = S3.format(rel=rel, theme="places", type="place")
    pc = columns(con, ppath)
    print(f"places columns: {sorted(pc)}", file=sys.stderr)
    if "taxonomy" in pc:
        # The specific category ("pizza_restaurant") reads better and lets
        # "pizza" find pizzerias; the broad basic_category is the fallback.
        cat = "coalesce(taxonomy.primary, basic_category)" if "basic_category" in pc else "taxonomy.primary"
        words = "array_to_string(taxonomy.hierarchy, ' ') || ' ' || coalesce(taxonomy.primary, '')"
    elif "categories" in pc:
        cat = "categories.primary"
        words = "coalesce(categories.primary, '') || ' ' || coalesce(array_to_string(categories.alternate, ' '), '')"
    else:
        cat, words = "NULL", "''"
    status_filter = "TRUE"
    if "operating_status" in pc:
        status_filter = "coalesce(operating_status, 'open') NOT IN ('permanently_closed', 'closed')"
    brand = "brand.names.primary" if "brand" in pc else "NULL"
    con.execute(f"""
        CREATE TABLE places AS
        SELECT id, names.primary AS name, {cat} AS category, {words} AS category_words,
               confidence, websites[1] AS website, phones[1] AS phone, {brand} AS brand,
               addresses[1].freeform AS street_addr, addresses[1].locality AS city,
               addresses[1].postcode AS zip, addresses[1].region AS region,
               (bbox.ymin + bbox.ymax) / 2 AS lat, (bbox.xmin + bbox.xmax) / 2 AS lon
        FROM read_parquet('{ppath}', hive_partitioning=1)
        WHERE {where} AND names.primary IS NOT NULL AND coalesce(confidence, 1) >= 0.35 AND {status_filter}
    """)
    con.execute(f"DELETE FROM places WHERE NOT {inside}")
    n_places = con.execute("SELECT count(*) FROM places").fetchone()[0]
    print(f"{n_places} places", file=sys.stderr)

    # ---- addresses ----
    apath = S3.format(rel=rel, theme="addresses", type="address")
    ac = columns(con, apath)
    print(f"address columns: {sorted(ac)}", file=sys.stderr)
    city = "address_levels[2].value" if "address_levels" in ac else "NULL"
    if "postal_city" in ac:
        city = f"coalesce(postal_city, {city})"
    # Each address keeps its own state: packs reach a little past the state
    # line, and "Covington, KY" must not become "Covington, OH".
    astate = f"coalesce(upper(nullif(trim(address_levels[1].value), '')), '{usps}')" if "address_levels" in ac else f"'{usps}'"
    con.execute(f"""
        CREATE TABLE addr AS
        SELECT id, trim(number) AS number, trim(street) AS street, postcode AS zip, {city} AS city, {astate} AS state,
               (bbox.ymin + bbox.ymax) / 2 AS lat, (bbox.xmin + bbox.xmax) / 2 AS lon
        FROM read_parquet('{apath}', hive_partitioning=1)
        WHERE country = 'US' AND {where} AND street IS NOT NULL AND number IS NOT NULL
    """)
    con.execute(f"DELETE FROM addr WHERE NOT {inside}")
    # One record per building address; units of the same building collapse.
    con.execute("""
        CREATE TABLE addr1 AS SELECT * FROM addr
        QUALIFY row_number() OVER (PARTITION BY lower(number), lower(street), lower(coalesce(city, '')), coalesce(zip, '') ORDER BY id) = 1
    """)
    n_addr = con.execute("SELECT count(*) FROM addr1").fetchone()[0]
    print(f"{n_addr} addresses", file=sys.stderr)

    st = usps
    con.execute(f"""
        COPY (
          SELECT 'ov:' || id AS id, 'place' AS kind, name,
                 concat_ws(', ', nullif(street_addr, ''), nullif(city, ''), nullif(concat_ws(' ', coalesce(region, '{st}'), zip), '')) AS label,
                 coalesce(category, '') AS category, coalesce(category_words, '') AS category_words,
                 coalesce(city, '') AS city, '{st}' AS state, coalesce(zip, '') AS zip,
                 lat, lon,
                 least(1.0, 0.55 * coalesce(confidence, 0.5) + CASE WHEN brand IS NOT NULL THEN 0.2 ELSE 0 END
                       + CASE WHEN website IS NOT NULL THEN 0.1 ELSE 0 END + CASE WHEN phone IS NOT NULL THEN 0.1 ELSE 0 END) AS importance,
                 coalesce(phone, '') AS phone, coalesce(website, '') AS website,
                 '' AS street, 'overture-places' AS source,
                 CASE WHEN brand IS NOT NULL AND lower(brand) <> lower(name) THEN [brand] ELSE []::VARCHAR[] END AS aliases
          FROM places
          UNION ALL
          SELECT 'ova:' || id, 'address', number || ' ' || street,
                 concat_ws(', ', nullif(city, ''), concat_ws(' ', state, zip)),
                 '', '', coalesce(city, ''), state, coalesce(zip, ''), lat, lon, 0.3, '', '', street, 'overture-addresses', []::VARCHAR[]
          FROM addr1
          UNION ALL
          SELECT 'st:' || md5(lower(street) || '|' || lower(coalesce(city, ''))), 'street', any_value(street),
                 concat_ws(', ', nullif(any_value(city), ''), any_value(state)),
                 '', '', coalesce(any_value(city), ''), any_value(state), '',
                 median(lat), median(lon), least(0.6, 0.2 + ln(count(*)) / 20), '', '', any_value(street), 'overture-addresses', []::VARCHAR[]
          FROM addr1 GROUP BY lower(street), lower(coalesce(city, ''))
        ) TO '{out}' (FORMAT JSON)
    """)
    print(f"wrote {out}", file=sys.stderr)


def areas_mode(con, rel, states_parquet: Path, out: Path):
    dpath = S3.format(rel=rel, theme="divisions", type="division")
    dc = columns(con, dpath)
    print(f"division columns: {sorted(dc)}", file=sys.stderr)
    cls = "class" if "class" in dc else "NULL"
    pop = "population" if "population" in dc else "NULL"
    con.execute(f"""
        CREATE TABLE div AS
        SELECT id, names.primary AS name, subtype, {cls} AS cls, {pop} AS population, region,
               (bbox.ymin + bbox.ymax) / 2 AS lat, (bbox.xmin + bbox.xmax) / 2 AS lon
        FROM read_parquet('{dpath}', hive_partitioning=1)
        WHERE country = 'US' AND names.primary IS NOT NULL
          AND subtype IN ('region', 'county', 'localadmin', 'locality', 'borough', 'macrohood', 'neighborhood')
    """)
    names = ", ".join(f"('{k}', '{v.replace(chr(39), chr(39) * 2)}')" for k, v in STATE_NAMES.items())
    con.execute(f"CREATE TABLE sn(usps VARCHAR, sname VARCHAR); INSERT INTO sn VALUES {names}")

    zcta = zcta_table(con, states_parquet)
    con.execute(f"""
        COPY (
          SELECT 'ovd:' || d.id AS id,
                 CASE d.subtype
                   WHEN 'region' THEN 'state'
                   WHEN 'county' THEN 'county'
                   WHEN 'locality' THEN CASE coalesce(d.cls, 'city') WHEN 'town' THEN 'town' WHEN 'village' THEN 'village' WHEN 'hamlet' THEN 'village' ELSE 'city' END
                   WHEN 'localadmin' THEN 'town'
                   ELSE 'neighborhood' END AS kind,
                 d.name,
                 CASE WHEN d.subtype = 'region' THEN 'United States' ELSE concat_ws(', ', sn.sname, 'United States') END AS label,
                 '' AS category, '' AS category_words, '' AS city,
                 coalesce(sn.usps, '') || ' ' || coalesce(sn.sname, '') AS state, '' AS zip,
                 d.lat, d.lon,
                 CASE WHEN d.subtype = 'region' THEN 1.0
                      WHEN d.subtype = 'county' THEN 0.55
                      ELSE least(1.0, greatest(0.15, ln(coalesce(d.population, 300) + 1) / ln(10) / 6.8)) END AS importance,
                 '' AS phone, '' AS website, '' AS street, 'overture-divisions' AS source
          FROM div d LEFT JOIN sn ON sn.usps = replace(d.region, 'US-', '')
          {"UNION ALL SELECT 'zip:' || zip, 'zip', zip, concat_ws(', ', 'ZIP code', sname), '', '', '', usps || ' ' || coalesce(sname, ''), zip, lat, lon, 0.5, '', '', '', 'census-zcta' FROM zcta" if zcta else ""}
        ) TO '{out}' (FORMAT JSON)
    """)
    print(f"wrote {out}", file=sys.stderr)


def zcta_table(con, states_parquet: Path) -> bool:
    """Census ZIP Code Tabulation Areas (public domain), tagged with their state."""
    blob = None
    for year in (2025, 2024, 2023):
        url = f"https://www2.census.gov/geo/docs/maps-data/data/gazetteer/{year}_Gazetteer/{year}_Gaz_zcta_national.zip"
        try:
            req = urllib.request.Request(url, headers=UA)
            blob = urllib.request.urlopen(req, timeout=300).read()
            print(f"ZIP areas: {url}", file=sys.stderr)
            break
        except Exception as e:
            print(f"{url}: {e}", file=sys.stderr)
    if blob is None:
        return False
    z = zipfile.ZipFile(io.BytesIO(blob))
    name = next(n for n in z.namelist() if n.endswith(".txt"))
    Path("zcta.txt").write_bytes(z.read(name))
    con.execute("""
        CREATE TABLE zraw AS SELECT * FROM read_csv('zcta.txt', header=true, all_varchar=true, auto_detect=true)
    """)
    cols = [r[0] for r in con.execute("DESCRIBE zraw").fetchall()]
    print(f"ZIP file columns: {cols}", file=sys.stderr)

    def col(*keys):
        for c in cols:
            u = c.strip().lstrip("\ufeff").upper()
            if any(k in u for k in keys):
                return c
        raise SystemExit(f"ZIP file has no column like {keys}: {cols}")

    zip_c, lat_c, lon_c = col("GEOID", "ZCTA"), col("INTPTLAT"), col("INTPTLON")
    con.execute(f"""
        CREATE TABLE zpts AS SELECT right('00000' || trim("{zip_c}"), 5) AS zip,
               CAST(trim("{lat_c}") AS DOUBLE) AS lat, CAST(trim("{lon_c}") AS DOUBLE) AS lon
        FROM zraw
    """)
    con.execute(f"CREATE TABLE us_states AS SELECT usps, name AS sname, ST_GeomFromHEXWKB(wkb_hex) AS geom FROM read_parquet('{states_parquet}')")
    con.execute("""
        CREATE TABLE zcta AS SELECT z.zip, z.lat, z.lon, s.usps, s.sname
        FROM zpts z LEFT JOIN us_states s ON ST_Within(ST_Point(z.lon, z.lat), s.geom)
    """)
    n = con.execute("SELECT count(*) FROM zcta").fetchone()[0]
    print(f"{n} ZIP areas", file=sys.stderr)
    return True


def add_numbers(path: Path):
    """Copy the house number out of address names into the `number` field."""
    tmp = path.with_suffix(".tmp")
    with path.open() as src, tmp.open("w") as dst:
        for line in src:
            r = json.loads(line)
            if r.get("kind") == "address":
                num, _, street = r["name"].partition(" ")
                r["number"] = num
                r["street"] = street
            for k, v in list(r.items()):
                if v is None:
                    r[k] = "" if k not in ("lat", "lon", "importance") else 0
            dst.write(json.dumps(r, ensure_ascii=False) + "\n")
    tmp.replace(path)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["state", "areas"])
    ap.add_argument("--build", default="build/build.json")
    ap.add_argument("--region")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    build = json.loads(Path(args.build).read_text())
    rel = build["overture_release"]
    out = Path(args.out)
    con = connect()
    if args.mode == "state":
        region = next(r for r in build["regions"] if r["id"] == args.region)
        boundary = Path(args.build).parent / "boundaries" / f"{region['id']}.geojson"
        state_mode(con, rel, region, boundary, out)
    else:
        areas_mode(con, rel, Path(args.build).parent / "boundaries" / "us_states.parquet", out)
    add_numbers(out)
    n = sum(1 for _ in out.open())
    print(f"{n} records in {out}")


if __name__ == "__main__":
    main()
