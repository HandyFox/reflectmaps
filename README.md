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

### Portable version (external drive)

`ReflectMaps_<version>_portable_x64.zip` runs from an external SSD or USB
drive on any Windows 10/11 PC, with nothing installed. Unzip it onto the
drive and double-click `ReflectMaps.exe`. Map data, settings and the web
view's storage all live in the `ReflectMaps Data` folder next to the
program; Microsoft's WebView2 runtime is bundled, so it also runs on PCs
that lack it.

To fill a drive: **Downloads > Download all** queues every state (about
51 GB to download, about 100 GB installed). Downloads run two states at a
time, can be paused, and survive closing the app or unplugging the drive.
To hand out copies, close the app and copy the whole `ReflectMaps` folder to
the other drive, then use **Check data** on the copy to confirm every file
is intact. Use exFAT or NTFS drives (FAT32 can't hold some of the files).

### Computer without internet

The installer itself works offline. To add states, use another computer:

1. Open the newest **Map data** release on the
   [Releases page](https://github.com/HandyFox/reflectmaps/releases) and
   download `catalog.json` plus, for each state you want, its three files
   named by the state's two-letter code: `oh-map.pmtiles`,
   `oh-search.tar.gz` and `oh-routing.tar.gz`. Files too big for GitHub come
   in parts (`ca-search.tar.gz.part00`, `.part01`, ...): download every part.
2. Copy them all into one folder on a USB drive.
3. In ReflectMaps open **Downloads**, choose **Import folder...** and pick
   that folder, or drag the files onto the window.

The app joins split files, checks every file against `catalog.json` (so a
damaged copy is caught), and uses the catalog as its list of states. Without
`catalog.json` the packs still import, unchecked. Directions work across
state lines when all the directions packs come from the same release.

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
  turn-by-turn steps.
- **Printing**: print the map view (Print map button or Ctrl+P) or a route's
  directions, with a preview, paper size and orientation, a sharp map image,
  scale bar and north arrow. Directions print with an overview map and a
  numbered step table. Pick a PDF printer to save a PDF instead.
- **Search area**: a "Search in" picker under the search box: *Near the map*
  (states in view and next to it, following the map), states you tick, or
  all states. Fewer states make searching faster. The Directions boxes use
  the same choice, with a "Search all states" fallback; routes themselves
  always cross any downloaded states.
- **Results on the map**: press Enter to see up to 20 results as numbered
  pins; click a pin or a row for the address, phone number and website.
- **Stops**: up to 10 points per route; drag the grip beside any stop
  (start and destination included) to reorder. Right-click the map to start,
  end or *Add to route as a stop* (it goes where it adds the least driving).
- **History and address book**: places you open or route to are remembered
  (last 50) and offered when you click an empty search or From/To box. Save
  any place under your own name (star button, or right-click > Save this
  place); saved places show as stars on the map, match as you type, and
  import/export as CSV (rows without coordinates are looked up by address).
  *Downloads > Clear personal data* deletes both, e.g. before copying a
  portable drive.
- **Downloads**: pick states one by one or select several (or all) and
  download them in a queue with overall progress, pause/resume and Update
  all; each state has map, search and directions parts that download,
  verify and update separately. **Check data** re-reads every installed file
  against its checksum. Packs copied from GitHub onto a
  USB drive can be imported on computers without internet.

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

Run **Data packs** with `all` to rebuild every state (about 3.5 hours,
mostly the national routing graph), or with a list of ids such as `oh,ky`.
Packs over GitHub's 2 GiB file limit are published in parts and joined by
the app. The routing graph is always built for every requested state at
once so routes cross state lines. If only the routing job fails (for
example a slow OpenStreetMap download), run **Routing packs**
(`.github/workflows/routing.yml`) with the build's tag and run id: it
rebuilds the routing packs over several jobs and publishes that build.

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
