"""Shared helpers for the data pipeline."""
import sys

import duckdb


def connect():
    """DuckDB with the spatial and httpfs extensions.

    On CI the extensions download normally. Where the extension server is not
    reachable, the PyPI-packaged copies (duckdb-extensions) are used instead.
    """
    con = duckdb.connect()
    for ext in ("spatial", "httpfs"):
        try:
            con.execute(f"INSTALL {ext}")
        except Exception:
            try:
                import duckdb_extensions

                duckdb_extensions.import_extension(ext)
            except Exception as e:  # httpfs is optional for local files
                print(f"extension {ext} unavailable: {e}", file=sys.stderr)
        try:
            con.execute(f"LOAD {ext}")
        except Exception:
            if ext == "spatial":
                raise
    try:
        con.execute("SET s3_region='us-west-2'")
    except Exception:
        pass
    con.execute("SET preserve_insertion_order=false")
    return con
