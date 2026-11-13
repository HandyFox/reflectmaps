#!/usr/bin/env python3
"""Write a Valhalla configuration for building or serving tiles.

Usage: valhalla_config.py TILE_DIR [ADMIN_DB] > valhalla.json
"""
import copy
import json
import sys

from valhalla.config import _sanitize_config
from valhalla.valhalla_build_config import config as defaults


def main():
    tile_dir = sys.argv[1]
    admin = sys.argv[2] if len(sys.argv) > 2 else ""
    cfg = _sanitize_config(copy.deepcopy(defaults))
    cfg["mjolnir"]["tile_dir"] = tile_dir
    cfg["mjolnir"]["tile_extract"] = ""
    cfg["mjolnir"]["traffic_extract"] = ""
    cfg["mjolnir"]["admin"] = admin
    cfg["mjolnir"]["timezone"] = ""
    cfg["mjolnir"]["concurrency"] = 4
    cfg["additional_data"]["elevation"] = ""
    json.dump(cfg, sys.stdout, indent=1)


if __name__ == "__main__":
    main()
