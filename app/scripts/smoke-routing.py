#!/usr/bin/env python3
"""Checks that the bundled Valhalla runtime works on this machine.

Builds a tiny road graph (Utrecht test data from the Valhalla repository)
with the pyvalhalla tools, then asks the *bundled* valhalla_service for a
route exactly the way the app does: one-shot mode, libraries on the search
path. Fails the build if no route comes back.
"""
import json
import os
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path

RES = Path(__file__).resolve().parent.parent / "src-tauri" / "resources"
TEST_PBF = "https://raw.githubusercontent.com/valhalla/valhalla/master/test/data/utrecht_netherlands.osm.pbf"


def main():
    runtime = RES / "valhalla-runtime"
    exe = runtime / "valhalla" / "bin" / ("valhalla_service.exe" if os.name == "nt" else "valhalla_service")
    if not exe.exists():
        raise SystemExit(f"missing {exe}")
    tmp = Path(tempfile.mkdtemp())
    pbf = tmp / "utrecht.osm.pbf"
    pbf.write_bytes(urllib.request.urlopen(TEST_PBF, timeout=120).read())
    cfg = json.loads((RES / "valhalla-defaults.json").read_text())
    cfg["mjolnir"]["tile_dir"] = str(tmp / "tiles")
    cfg_path = tmp / "valhalla.json"
    cfg_path.write_text(json.dumps(cfg))
    subprocess.run([sys.executable, "-m", "valhalla", "valhalla_build_tiles", "-c", str(cfg_path), str(pbf)], check=True)

    libs = [runtime / "pyvalhalla.libs", runtime / "valhalla" / ".dylibs", runtime / "valhalla" / "bin"]
    joined = os.pathsep.join(str(p) for p in libs if p.exists())
    env = dict(os.environ)
    if os.name == "nt":
        env["PATH"] = joined + os.pathsep + env.get("PATH", "")
    elif sys.platform == "darwin":
        env["DYLD_LIBRARY_PATH"] = joined
    else:
        env["LD_LIBRARY_PATH"] = joined
    req = {
        "locations": [{"lat": 52.0938, "lon": 5.1100}, {"lat": 52.0786, "lon": 5.1325}],
        "costing": "auto",
        "alternates": 2,
        "directions_options": {"units": "miles"},
    }
    out = subprocess.run([str(exe), str(cfg_path), "route", json.dumps(req)], capture_output=True, env=env)
    text = out.stdout.decode(errors="replace")
    if out.returncode != 0 or '"trip"' not in text:
        print(text[-2000:])
        print(out.stderr.decode(errors="replace")[-2000:])
        raise SystemExit(f"bundled routing engine failed (exit {out.returncode})")
    trip = json.loads(text[text.index("{"):])["trip"]
    print(f"bundled routing OK: {trip['summary']['length']:.2f} mi, {len(trip['legs'][0]['maneuvers'])} steps")


if __name__ == "__main__":
    main()
