import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, type CatalogRegion, type Hit, type ImportReport, type Progress, type RouteOption, type Status } from "./api";
import { Directions, type AvoidOptions } from "./components/Directions";
import { Downloads } from "./components/Downloads";
import { DownloadIcon, MoonIcon, RouteIcon, SearchIcon, SunIcon } from "./components/Icons";
import { PlaceCard } from "./components/PlaceCard";
import { SearchBox } from "./components/SearchBox";
import { formatDuration, formatMiles, zoomFor } from "./format";
import { MapController, type View } from "./map/MapController";
import type { Flavor } from "./map/style";

type Tab = "search" | "directions" | "downloads";
type Menu = { lat: number; lon: number; x: number; y: number };

const US_VIEW: View = { center: [-96.5, 38.6], zoom: 3.6 };

function loadView(): View {
  try {
    const v = JSON.parse(localStorage.getItem("rm.view") ?? "");
    if (Array.isArray(v.center) && typeof v.zoom === "number") return v;
  } catch {
    /* first run */
  }
  return US_VIEW;
}

function pointHit(lat: number, lon: number, name = "Dropped pin"): Hit {
  return {
    id: `pt:${lat},${lon}`,
    kind: "coordinates",
    name,
    label: `${lat.toFixed(5)}, ${lon.toFixed(5)}`,
    category: "",
    lat,
    lon,
    source: "",
    score: 0,
    approximate: false,
  };
}

/** Names a map point after the nearest address or place, if one is close. */
async function namePoint(lat: number, lon: number): Promise<Hit> {
  const r = await api.reverse(lat, lon).catch(() => null);
  if (r && (r.distance_km ?? 99) < 0.25) return { ...r, lat, lon, id: `pt:${lat},${lon}` };
  return pointHit(lat, lon);
}

export default function App() {
  const mapEl = useRef<HTMLDivElement>(null);
  const ctrl = useRef<MapController | null>(null);
  const [tab, setTab] = useState<Tab>("search");
  const [flavor, setFlavor] = useState<Flavor>(() => (localStorage.getItem("rm.flavor") as Flavor) || "light");
  const [status, setStatus] = useState<Status | null>(null);
  const [progress, setProgress] = useState<Record<string, Progress>>({});
  const [place, setPlace] = useState<Hit | null>(null);
  const [waypoints, setWaypoints] = useState<(Hit | null)[]>([null, null]);
  const [avoid, setAvoid] = useState<AvoidOptions>({ avoid_tolls: false, avoid_highways: false, avoid_ferries: false });
  const [routes, setRoutes] = useState<RouteOption[]>([]);
  const [selected, setSelected] = useState(0);
  const [routing, setRouting] = useState(false);
  const [routeError, setRouteError] = useState<string | null>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [toasts, setToasts] = useState<{ id: number; text: string }[]>([]);
  const [online, setOnline] = useState(navigator.onLine);
  const [importing, setImporting] = useState(false);
  const [importReport, setImportReport] = useState<ImportReport | null>(null);
  const [catalogStamp, setCatalogStamp] = useState(0);
  const [dragging, setDragging] = useState(false);

  const notify = useCallback((text: string) => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t, { id, text }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 5000);
  }, []);

  const near = useCallback((): [number, number] => ctrl.current?.center() ?? [38.6, -96.5], []);

  const refreshStatus = useCallback(async () => {
    const s = await api.status();
    setStatus(s);
    return s;
  }, []);

  // ---- importing pack files (from a USB drive, for example) ----
  const runImport = useCallback(
    async (paths: string[]) => {
      if (!paths.length) return;
      setTab("downloads");
      setImporting(true);
      setImportReport(null);
      try {
        const r = await api.importPacks(paths);
        setImportReport(r);
        if (r.catalog_updated) setCatalogStamp((n) => n + 1);
        const s = await refreshStatus();
        ctrl.current?.reloadTiles();
        const bbox = r.first_region ? s.installed.regions[r.first_region]?.bbox : null;
        if (bbox) ctrl.current?.fitBbox(bbox);
        const n = r.imported.length;
        notify(n ? `Imported ${n} pack${n === 1 ? "" : "s"}` : r.already.length ? "Those packs are already installed" : "Nothing was imported");
      } catch (e) {
        setImportReport({ imported: [], skipped: [String(e)], already: [], warnings: [], catalog_updated: false });
      } finally {
        setImporting(false);
      }
    },
    [notify, refreshStatus],
  );

  // Files or folders dropped anywhere on the window are imported.
  useEffect(() => {
    const un = getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === "enter" || p.type === "over") setDragging(true);
      else if (p.type === "leave") setDragging(false);
      else if (p.type === "drop") {
        setDragging(false);
        runImport(p.paths);
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, [runImport]);

  // ---- map setup ----
  useEffect(() => {
    if (!mapEl.current) return;
    const c = new MapController(mapEl.current, flavor, loadView());
    ctrl.current = c;
    c.onMove = (v) => localStorage.setItem("rm.view", JSON.stringify(v));
    c.onContextMenu = (lat, lon, x, y) => setMenu({ lat, lon, x, y });
    c.onRouteClick = (id) => setSelected(id);
    c.onPoiClick = async (name, lat, lon) => {
      const hits = await api.search(name, [lat, lon], 5).catch(() => []);
      const h = hits.find((x) => (x.distance_km ?? 99) < 0.3);
      setPlace(h ?? { ...pointHit(lat, lon, name), kind: "place" });
      setTab("search");
    };
    refreshStatus().then((s) => {
      if (Object.keys(s.installed.regions).length === 0) setTab("downloads");
    });
    const un = api.onProgress((p) => setProgress((cur) => ({ ...cur, [`${p.region}:${p.component}`]: p })));
    const on = () => setOnline(true);
    const off = () => setOnline(false);
    window.addEventListener("online", on);
    window.addEventListener("offline", off);
    return () => {
      un.then((f) => f());
      window.removeEventListener("online", on);
      window.removeEventListener("offline", off);
      c.destroy();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    ctrl.current?.setFlavor(flavor);
    localStorage.setItem("rm.flavor", flavor);
    document.documentElement.dataset.theme = flavor;
  }, [flavor]);

  useEffect(() => {
    ctrl.current?.showPlace(tab === "search" ? place : null);
  }, [place, tab]);

  // ---- directions ----
  const onDrag = useCallback(
    async (i: number, lat: number, lon: number) => {
      const h = await namePoint(lat, lon);
      setWaypoints((w) => w.map((x, j) => (j === i ? h : x)));
    },
    [],
  );

  useEffect(() => {
    ctrl.current?.setWaypoints(
      tab === "directions" ? waypoints.map((w) => (w ? { lat: w.lat, lon: w.lon, label: w.name } : null)) : [],
      onDrag,
    );
  }, [waypoints, tab, onDrag]);

  const routeSeq = useRef(0);
  useEffect(() => {
    const pts = waypoints.filter((w): w is Hit => !!w);
    const seq = ++routeSeq.current;
    if (pts.length < 2 || pts.length !== waypoints.length) {
      setRoutes([]);
      setRouteError(null);
      return;
    }
    setRouting(true);
    setRouteError(null);
    api
      .route({ locations: pts.map((p) => ({ lat: p.lat, lon: p.lon })), ...avoid })
      .then((r) => {
        if (seq !== routeSeq.current) return;
        setRoutes(r);
        setSelected(0);
        if (r[0]) ctrl.current?.fitCoords(r[0].coordinates, 70);
      })
      .catch((e) => {
        if (seq !== routeSeq.current) return;
        setRoutes([]);
        setRouteError(String(e));
      })
      .finally(() => seq === routeSeq.current && setRouting(false));
  }, [waypoints, avoid]);

  useEffect(() => {
    ctrl.current?.setRoutes(tab === "directions" ? routes : [], selected);
    if (tab !== "directions") ctrl.current?.highlight(null, null);
  }, [routes, selected, tab]);

  const directionsTo = (h: Hit) => {
    setWaypoints((w) => {
      const next = [...w];
      next[next.length - 1] = h;
      return next;
    });
    setTab("directions");
  };

  // ---- context menu ----
  const menuAction = async (action: "from" | "to" | "here" | "copy") => {
    if (!menu) return;
    const { lat, lon } = menu;
    setMenu(null);
    if (action === "copy") {
      navigator.clipboard?.writeText(`${lat.toFixed(6)}, ${lon.toFixed(6)}`);
      notify("Coordinates copied");
      return;
    }
    const h = await namePoint(lat, lon);
    if (action === "here") {
      const r = await api.reverse(lat, lon).catch(() => null);
      setPlace(r ?? pointHit(lat, lon));
      setTab("search");
    } else if (action === "from") {
      setWaypoints((w) => [h, ...w.slice(1)]);
      setTab("directions");
    } else {
      directionsTo(h);
    }
  };

  const regionsInstalled = useMemo(
    () => Object.values(status?.installed.regions ?? {}).map((r) => r.name),
    [status],
  );

  const onPacksChanged = async (zoomTo?: CatalogRegion) => {
    await refreshStatus();
    ctrl.current?.reloadTiles();
    if (zoomTo?.bbox) ctrl.current?.fitBbox(zoomTo.bbox);
  };

  const selectedRoute = routes.find((r) => r.id === selected);

  return (
    <div className="app">
      <aside className="panel">
        <header className="panel-head">
          <div className="brand">
            <img src="/logo.svg" alt="" width={26} height={26} />
            <span>ReflectMaps</span>
          </div>
          <button
            className="icon-btn"
            title={flavor === "light" ? "Dark map" : "Light map"}
            onClick={() => setFlavor(flavor === "light" ? "dark" : "light")}
          >
            {flavor === "light" ? <MoonIcon /> : <SunIcon />}
          </button>
        </header>
        <nav className="tabs">
          <button className={tab === "search" ? "on" : ""} onClick={() => setTab("search")}>
            <SearchIcon size={16} /> Search
          </button>
          <button className={tab === "directions" ? "on" : ""} onClick={() => setTab("directions")}>
            <RouteIcon size={16} /> Directions
          </button>
          <button className={tab === "downloads" ? "on" : ""} onClick={() => setTab("downloads")}>
            <DownloadIcon size={16} /> Downloads
          </button>
        </nav>

        <section className="panel-body">
          {tab === "search" && (
            <>
              <SearchBox
                autoFocus
                placeholder="Search places and addresses"
                near={near}
                onSelect={(h) => {
                  setPlace(h);
                  ctrl.current?.flyTo(h.lat, h.lon, zoomFor(h.kind));
                }}
                onClear={() => setPlace(null)}
              />
              {place && (
                <PlaceCard
                  hit={place}
                  onClose={() => setPlace(null)}
                  onDirections={(h) => directionsTo(h)}
                />
              )}
              {!place && (
                <div className="tips muted small">
                  <p>Try a business name, a full address like “123 N High St Columbus”, a city, a ZIP code, or a category like “coffee” or “gas”.</p>
                  <p>Right-click the map for directions from or to any point.</p>
                </div>
              )}
            </>
          )}
          {tab === "directions" && (
            <Directions
              waypoints={waypoints}
              setWaypoints={setWaypoints}
              avoid={avoid}
              setAvoid={setAvoid}
              routes={routes}
              selected={selected}
              onSelect={(id) => {
                setSelected(id);
                const r = routes.find((x) => x.id === id);
                if (r) ctrl.current?.fitCoords(r.coordinates, 70);
              }}
              loading={routing}
              error={routeError}
              near={near}
              onStep={(m) => {
                ctrl.current?.highlight(m.lat, m.lon);
                ctrl.current?.flyTo(m.lat, m.lon, 16);
              }}
              onPrint={() => window.print()}
            />
          )}
          {tab === "downloads" && (
            <Downloads
              status={status}
              progress={progress}
              onChanged={onPacksChanged}
              notify={notify}
              onImport={runImport}
              importing={importing}
              importReport={importReport}
              onDismissReport={() => setImportReport(null)}
              catalogStamp={catalogStamp}
            />
          )}
        </section>

        <footer className="panel-foot small">
          <span className={`dot ${online ? "online" : "offline"}`} />
          {regionsInstalled.length ? (
            <span>Offline ready: {regionsInstalled.join(", ")}</span>
          ) : (
            <span>No states downloaded yet</span>
          )}
          {status && !status.routing_ready && status.routing_error && (
            <span className="warn-text" title={status.routing_error}> · directions unavailable</span>
          )}
        </footer>
      </aside>

      <main className="map-wrap" onClick={() => menu && setMenu(null)}>
        <div ref={mapEl} className="map" />
        {menu && (
          <ul className="context-menu" style={{ left: menu.x, top: menu.y }} onClick={(e) => e.stopPropagation()}>
            <li onClick={() => menuAction("from")}>Directions from here</li>
            <li onClick={() => menuAction("to")}>Directions to here</li>
            <li onClick={() => menuAction("here")}>What’s here?</li>
            <li onClick={() => menuAction("copy")}>Copy coordinates</li>
          </ul>
        )}
        <div className="toasts">
          {toasts.map((t) => (
            <div key={t.id} className="toast">{t.text}</div>
          ))}
        </div>
      </main>

      {dragging && (
        <div className="drop-overlay">
          <div>Drop map pack files or a folder to import them</div>
        </div>
      )}

      {selectedRoute && (
        <div className="print-only">
          <h1>
            {waypoints[0]?.name} → {waypoints[waypoints.length - 1]?.name}
          </h1>
          <p>
            {selectedRoute.label}
            {selectedRoute.via ? ` via ${selectedRoute.via}` : ""} · {formatDuration(selectedRoute.time_s)} ·{" "}
            {formatMiles(selectedRoute.distance_mi)}
          </p>
          <ol>
            {selectedRoute.maneuvers.map((m, i) => (
              <li key={i}>
                {m.instruction} {m.distance_mi > 0 ? `(${formatMiles(m.distance_mi)})` : ""}
              </li>
            ))}
          </ol>
          <p className="small">Printed from ReflectMaps. © OpenStreetMap contributors.</p>
        </div>
      )}
    </div>
  );
}
