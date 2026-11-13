#!/usr/bin/env python3
"""Gather the files bundled inside the installer.

  resources/valhalla-runtime/   Valhalla's route service and its libraries,
                                taken from the pyvalhalla wheel on PyPI
  resources/world-base.pmtiles  world overview map (zoom 0-6)
  resources/us-areas/           national search index of states, counties,
                                cities and ZIP codes

The last two come from the newest data release. Missing data is a warning,
not an error: the app still runs, it just starts with an empty map.

Usage: fetch-resources.py --platform {windows,linux,macos} [--catalog URL]
"""
import argparse
import hashlib
import io
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import zipfile
from pathlib import Path

PYVALHALLA = "3.9.1"
WHEEL_PLATFORM = {
    "windows": "win_amd64",
    "linux": "manylinux_2_28_x86_64",
    "macos": "macosx_14_0_arm64",
}
CATALOG = "https://github.com/HandyFox/reflectmaps/releases/download/data-current/catalog.json"
HERE = Path(__file__).resolve().parent
RES = HERE.parent / "src-tauri" / "resources"


def valhalla(platform: str):
    dest = RES / "valhalla-runtime"
    if dest.exists():
        shutil.rmtree(dest)
    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run(
            [sys.executable, "-m", "pip", "download", "--no-deps", "--only-binary=:all:",
             "--platform", WHEEL_PLATFORM[platform], "--python-version", "3.12",
             f"pyvalhalla=={PYVALHALLA}", "-d", tmp],
            check=True,
        )
        wheel = next(Path(tmp).glob("pyvalhalla-*.whl"))
        exe = "valhalla/bin/valhalla_service" + (".exe" if platform == "windows" else "")
        # Close the wheel before the temporary folder is removed: Windows
        # refuses to delete a file that is still open.
        with zipfile.ZipFile(wheel) as z:
            keep = [n for n in z.namelist()
                    if n == exe
                    or n.startswith("pyvalhalla.libs/")
                    or n.startswith("valhalla/.dylibs/")]
            for n in keep:
                if n.endswith("/"):
                    continue
                target = dest / n
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(z.read(n))
        (dest / "valhalla" / "bin" / Path(exe).name).chmod(0o755)
    if platform == "windows":
        # valhalla_service.exe needs the Visual C++ runtime; ship it next to
        # the other DLLs so machines without the redistributable still work.
        sysdir = Path(os.environ.get("SystemRoot", r"C:\Windows")) / "System32"
        for dll in ("vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll"):
            src = sysdir / dll
            if src.exists():
                shutil.copy2(src, dest / "pyvalhalla.libs" / dll)
            else:
                print(f"warning: {src} not found", file=sys.stderr)
    (dest / "LICENSE-valhalla.txt").write_text(
        "Valhalla routing engine, MIT License, https://github.com/valhalla/valhalla\n"
        f"Binaries from the pyvalhalla {PYVALHALLA} wheel on PyPI; bundled libraries keep their own licenses.\n"
    )
    files = sum(1 for _ in dest.rglob("*") if _.is_file())
    size = sum(p.stat().st_size for p in dest.rglob("*") if p.is_file())
    print(f"valhalla runtime: {files} files, {size / 1e6:.1f} MB")


def get(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "reflectmaps-build"})
    with urllib.request.urlopen(req, timeout=600) as r:
        return r.read()


def base_data(catalog_url: str):
    try:
        cat = json.loads(get(catalog_url))
    except Exception as e:
        print(f"warning: no data catalog ({e}); building without world map and city list", file=sys.stderr)
        return
    base = cat.get("base", {})
    if "world" in base:
        blob = get(base["world"]["url"])
        assert hashlib.sha256(blob).hexdigest() == base["world"]["sha256"], "world-base checksum"
        (RES / "world-base.pmtiles").write_bytes(blob)
        print(f"world overview: {len(blob) / 1e6:.1f} MB from {cat['tag']}")
    if "areas" in base:
        blob = get(base["areas"]["url"])
        assert hashlib.sha256(blob).hexdigest() == base["areas"]["sha256"], "us-areas checksum"
        if (RES / "us-areas").exists():
            shutil.rmtree(RES / "us-areas")
        tarfile.open(fileobj=io.BytesIO(blob), mode="r:gz").extractall(RES)
        print(f"city and ZIP list: {len(blob) / 1e6:.1f} MB from {cat['tag']}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--platform", choices=sorted(WHEEL_PLATFORM), required=True)
    ap.add_argument("--catalog", default=os.environ.get("RM_CATALOG_URL", CATALOG))
    ap.add_argument("--skip-data", action="store_true")
    args = ap.parse_args()
    RES.mkdir(parents=True, exist_ok=True)
    valhalla(args.platform)
    if not args.skip_data:
        base_data(args.catalog)


if __name__ == "__main__":
    main()
