#!/usr/bin/env python3
"""Puts Microsoft's WebView2 Fixed Version runtime (x64) in a folder, for the
portable Windows build, so it runs on PCs without WebView2 installed.

Runs on Windows (uses expand.exe to unpack the .cab).

Usage: fetch-webview2.py DEST_DIR

The newest Fixed Version .cab link is read from Microsoft's WebView2 page.
If that fails, the Evergreen Standalone Installer is saved next to DEST_DIR
instead, so a PC without WebView2 can still install it offline.
"""
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path

PAGES = [
    "https://developer.microsoft.com/en-us/microsoft-edge/webview2/",
    "https://developer.microsoft.com/microsoft-edge/webview2/",
]
CAB = re.compile(r"https://[^\"'\s<>]+?Microsoft\.WebView2\.FixedVersionRuntime\.([\d.]+)\.x64\.cab")
STANDALONE_X64 = "https://go.microsoft.com/fwlink/?linkid=2124701"
UA = {"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) ReflectMaps-build"}


def get(url: str) -> bytes:
    with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=120) as r:
        return r.read()


def find_cab() -> tuple[str, str] | None:
    for page in PAGES:
        try:
            html = get(page).decode("utf-8", "replace").replace("\\/", "/").replace("\\u002F", "/")
        except Exception as e:  # noqa: BLE001
            print(f"  {page}: {e}")
            continue
        found = {m.group(1): m.group(0) for m in CAB.finditer(html)}
        if found:
            ver = max(found, key=lambda v: [int(x) for x in v.split(".")])
            return ver, found[ver]
        print(f"  {page}: no Fixed Version link found")
    return None


def main():
    dest = Path(sys.argv[1]).resolve()
    hit = find_cab()
    if hit:
        ver, url = hit
        print(f"WebView2 Fixed Version {ver}: {url}")
        with tempfile.TemporaryDirectory() as tmp:
            cab = Path(tmp) / "wv2.cab"
            cab.write_bytes(get(url))
            out = Path(tmp) / "x"
            out.mkdir()
            subprocess.run(["expand", str(cab), "-F:*", str(out)], check=True, stdout=subprocess.DEVNULL)
            inner = next((p.parent for p in out.rglob("msedgewebview2.exe")), None)
            if not inner:
                sys.exit("msedgewebview2.exe not found in the cab")
            if dest.exists():
                shutil.rmtree(dest)
            shutil.copytree(inner, dest)
        print(f"Unpacked to {dest} ({sum(f.stat().st_size for f in dest.rglob('*') if f.is_file()) // 2**20} MB)")
        return
    # Fallback: the offline installer for PCs that lack WebView2.
    inst = dest.parent / "WebView2 installer (only if ReflectMaps won't start).exe"
    inst.write_bytes(get(STANDALONE_X64))
    print(f"::warning::Fixed Version runtime not found; bundled the standalone installer instead ({inst.name})")


if __name__ == "__main__":
    main()
