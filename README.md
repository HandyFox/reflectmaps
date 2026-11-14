# ReflectMaps

An offline maps app for Windows, macOS and Linux. Download the states you
need once; then browse the map, search places and addresses, and plan
driving routes with no internet connection.

Map data covers **all 50 states and Washington, DC**. Each state downloads separately.

## Install

Download the installer for your system from the newest `v*` release on the
[Releases page](https://github.com/HandyFox/reflectmaps/releases):

| System | File |
| --- | --- |
| Windows 10/11 | `ReflectMaps_*_x64-setup.exe` (or the `.msi`) |
| macOS 14+ (Apple Silicon) | `ReflectMaps_*_aarch64.dmg`. Not notarized yet: right-click the app and choose Open the first time |
| Linux | `.AppImage` (any distribution), `.deb` (Ubuntu/Debian) or `.rpm` (Fedora) |

Open the app, go to **Downloads** and download a state. After that,
everything works with the internet off.

## What it does

- **Map**: street map with light and dark styles, offline from the first
  launch (a world overview ships in the installer; states add full detail).
- **Search**: businesses, addresses, streets, cities, counties, ZIP codes and
  coordinates, with autocomplete, typo tolerance and ranking by distance from
  the map view. Category words ("coffee", "gas", "pharmacy") find the nearest
  places of that kind. A house number missing from the data is estimated from
  its neighbors on the street and marked *approximate*.
- **Directions**: driving routes with alternatives (fastest, other routes,
  shortest, no highways), avoid tolls, highways or ferries, extra stops,
  turn-by-turn steps, printing.
- **Downloads**: pick states; each has map, search and directions parts that
  download, verify and update separately. Packs can also be imported from a
  file for computers without internet.

Search accuracy is measured on every data build against
[`eval/cases.json`](eval/cases.json). The current Ohio + Kentucky build
passes 58 of 59 cases in the top five results.

## How it is built

Everything is built on GitHub's free runners and hosted as GitHub release
files; nothing else is hosted.

| Workflow | What it does |
| --- | --- |
| **Data packs** (`.github/workflows/data.yml`, run from the Actions tab) | Builds map, search and routing packs for the chosen states, scores search accuracy (fails below 90%), and publishes a `data-YYYYMMDD-N` release plus `catalog.json` on the `data-current` release |
| **App** (`.github/workflows/app.yml`, runs on every push) | Builds and tests the app on Windows, macOS and Linux. Run it with *publish* ticked, or push a `v*` tag, to publish installers |

### Map packs

| Part | Source | Built with |
| --- | --- | --- |
| Map | Protomaps basemap (OpenStreetMap) | `pmtiles extract`, cut to the state outline |
| Search | Overture Maps places and addresses | DuckDB, then `crates/rm-search` (Tantivy) |
| Routing | OpenStreetMap road network | Valhalla, one graph for all states, split per state so packs join at state lines |
| US cities and ZIPs (in the installer) | Overture divisions, Census ZCTA | same search engine |

Run **Data packs** with `all` to rebuild every state (about 3 to 5 hours,
mostly the national routing graph), or with a list of ids such as `oh,ky`.
Packs over GitHub's 2 GiB file limit are published in parts and joined by
the app. The routing graph is always built for every requested state at
once so routes cross state lines.

## Repository

| Path | What it is |
| --- | --- |
| `app/` | Desktop app: Tauri 2 (Rust) with a React + MapLibre interface |
| `app/src-tauri/` | Rust core: tiles, packs, search, routing (Valhalla runs per request, no server) |
| `crates/rm-search/` | Offline search engine; `rm-search-query` and `rm-search-eval` are command-line tools for it |
| `pipeline/` | Scripts that build the downloadable packs |
| `eval/` | Search test cases |

### Develop

```sh
cd app
npm ci
node scripts/fetch-assets.mjs                      # map fonts and icons
python scripts/fetch-resources.py --platform linux # routing engine, world map, city list (windows / macos / linux)
npx tauri dev
```

Linux also needs `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`.

## Known limitations

- No live traffic; travel times assume typical free-flow speeds.
- No satellite view yet.
- Overture's data contains some duplicate or misplaced business listings;
  search shows them with their addresses so they can be told apart.
- Estimated house numbers can be off by a block or more where the address
  data is sparse.
- Search takes roughly 100 to 200 ms per keystroke with two states installed.
- Installers are not code-signed yet, so Windows SmartScreen and macOS
  Gatekeeper will warn on first launch.

## Data sources and credit

© OpenStreetMap contributors (ODbL). Overture Maps Foundation (licenses by
source). Protomaps basemap. US Census Bureau (public domain). Routing by
Valhalla (MIT). Map fonts: Noto Sans (SIL Open Font License).

## License

Code: Apache-2.0. Map data keeps its own licenses, listed above.
