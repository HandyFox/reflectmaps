# ReflectMaps

An offline maps app for Windows, macOS and Linux. Download the states you
need once; search, browse and plan driving routes with no internet
connection.

Work in progress. The first test build covers **Ohio** and **Kentucky**.

## What is in this repository

| Path | What it is |
| --- | --- |
| `app/` | Desktop app: Tauri 2 (Rust) with a React + MapLibre interface |
| `crates/rm-search/` | Offline place and address search engine (Rust, Tantivy) |
| `pipeline/` | Scripts that build the downloadable map packs |
| `eval/` | Search test cases; the data build scores itself against them |
| `.github/workflows/` | Data builds and app builds on GitHub's free runners |

## Map packs

Each state has three downloadable parts, published as files on a GitHub
release:

- **Map**: street map tiles (Protomaps basemap built from OpenStreetMap)
- **Search**: places and addresses from Overture Maps
- **Routing**: driving road graph for Valhalla, built from OpenStreetMap

The app reads `catalog.json` from the release tagged `data-current`.
Run the **Data packs** workflow (Actions tab) to rebuild; add states in
`pipeline/regions.json`.

## Data sources and credit

© OpenStreetMap contributors (ODbL). Overture Maps Foundation (licenses by
source). Protomaps basemap. US Census Bureau (public domain). Routing by
Valhalla (MIT).

## License

Code: Apache-2.0. Map data keeps its own licenses, listed above.
