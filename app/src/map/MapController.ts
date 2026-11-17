import maplibregl, {
  type GeoJSONSource,
  type LngLatBoundsLike,
  type MapMouseEvent,
  type VectorTileSource,
} from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import type { FeatureCollection } from "geojson";
import { api, type Bbox, type Hit, type RouteOption } from "../api";
import { buildStyle, bumpTileVersion, type Flavor } from "./style";

let protocolAdded = false;
function addTileProtocol() {
  if (protocolAdded) return;
  protocolAdded = true;
  // Tiles come from the local map packs through the Rust core.
  maplibregl.addProtocol("rm", async (params) => {
    const m = /^rm:\/\/tiles\/(\d+)\/(\d+)\/(\d+)/.exec(params.url);
    if (!m) throw new Error(`bad tile url ${params.url}`);
    const data = await api.tile(Number(m[1]), Number(m[2]), Number(m[3]));
    return { data };
  });
}

export type Waypoint = { lat: number; lon: number; label: string };
export type View = { center: [number, number]; zoom: number };

const ROUTE_SRC = "rm-routes";
const POINT_SRC = "rm-point";
const RESULT_SRC = "rm-results";
const RESULT_LAYERS = ["rm-result-dot", "rm-result-num"];
const SAVED_SRC = "rm-saved";

export type SavedPin = { id: string; name: string; lat: number; lon: number };

function pinElement(cls: string, text = ""): HTMLElement {
  const el = document.createElement("div");
  el.className = cls;
  if (text) {
    const s = document.createElement("span");
    s.textContent = text;
    el.appendChild(s);
  }
  return el;
}

export class MapController {
  map: maplibregl.Map;
  private flavor: Flavor;
  private routes: RouteOption[] = [];
  private selected = 0;
  private point: [number, number] | null = null;
  private placeMarker: maplibregl.Marker | null = null;
  private wpMarkers: maplibregl.Marker[] = [];
  private results: Hit[] = [];
  private resultSelected: string | null = null;
  private resultHover: string | null = null;
  onResultClick: (id: string) => void = () => {};
  onSavedClick: (id: string) => void = () => {};
  private saved: SavedPin[] = [];
  onResultHover: (id: string | null) => void = () => {};
  onRouteClick: (id: number) => void = () => {};
  onContextMenu: (lat: number, lon: number, x: number, y: number) => void = () => {};
  onPoiClick: (name: string, lat: number, lon: number) => void = () => {};
  onMove: (v: View) => void = () => {};

  constructor(el: HTMLElement, flavor: Flavor, view: View) {
    addTileProtocol();
    this.flavor = flavor;
    this.map = new maplibregl.Map({
      container: el,
      style: buildStyle(flavor),
      center: view.center,
      zoom: view.zoom,
      attributionControl: { compact: true },
      maxPitch: 60,
    });
    this.map.addControl(new maplibregl.NavigationControl({ visualizePitch: true }), "bottom-right");
    this.map.addControl(new maplibregl.ScaleControl({ unit: "imperial", maxWidth: 120 }), "bottom-left");
    this.map.on("style.load", () => this.addOverlays());
    this.map.on("contextmenu", (e: MapMouseEvent) => {
      e.preventDefault();
      this.onContextMenu(e.lngLat.lat, e.lngLat.lng, e.point.x, e.point.y);
    });
    this.map.on("moveend", () => {
      const c = this.map.getCenter();
      this.onMove({ center: [c.lng, c.lat], zoom: this.map.getZoom() });
    });
    this.map.on("click", (e) => {
      const pins = this.map.getLayer("rm-result-dot")
        ? this.map.queryRenderedFeatures(e.point, { layers: RESULT_LAYERS })
        : [];
      if (pins.length) {
        this.onResultClick(String(pins[0].properties?.id));
        return;
      }
      const stars = this.map.getLayer("rm-saved") ? this.map.queryRenderedFeatures(e.point, { layers: ["rm-saved"] }) : [];
      if (stars.length) {
        this.onSavedClick(String(stars[0].properties?.id));
        return;
      }
      const routeHits = this.map.queryRenderedFeatures(e.point, { layers: ["rm-route-alt", "rm-route-sel"] });
      if (routeHits.length) {
        this.onRouteClick(Number(routeHits[0].properties?.id ?? 0));
        return;
      }
      const poiLayers = this.map
        .getStyle()
        .layers.filter((l) => l.id === "pois" || l.id.startsWith("pois"))
        .map((l) => l.id);
      if (!poiLayers.length) return;
      const pois = this.map.queryRenderedFeatures(e.point, { layers: poiLayers });
      const f = pois.find((p) => p.properties?.name);
      if (f && f.geometry.type === "Point") {
        const [lon, lat] = f.geometry.coordinates as [number, number];
        this.onPoiClick(String(f.properties.name), lat, lon);
      }
    });
    for (const id of ["rm-route-alt", "rm-route-sel", "rm-saved", ...RESULT_LAYERS]) {
      this.map.on("mouseenter", id, () => (this.map.getCanvas().style.cursor = "pointer"));
      this.map.on("mouseleave", id, () => (this.map.getCanvas().style.cursor = ""));
    }
    this.map.on("mousemove", "rm-result-dot", (e) => {
      const id = String(e.features?.[0]?.properties?.id ?? "");
      if (id && id !== this.resultHover) {
        this.resultHover = id;
        this.updateResults();
        this.onResultHover(id);
      }
    });
    this.map.on("mouseleave", "rm-result-dot", () => {
      this.resultHover = null;
      this.updateResults();
      this.onResultHover(null);
    });
  }

  private addOverlays() {
    const m = this.map;
    if (!m.getSource(ROUTE_SRC)) {
      m.addSource(ROUTE_SRC, { type: "geojson", data: this.routeData() });
      m.addSource(POINT_SRC, { type: "geojson", data: this.pointData() });
      const labelLayer = m.getStyle().layers.find((l) => l.type === "symbol")?.id;
      const alt = this.flavor === "dark" ? "#7b8794" : "#9aa5b1";
      m.addLayer(
        {
          id: "rm-route-alt-casing",
          type: "line",
          source: ROUTE_SRC,
          filter: ["==", ["get", "selected"], false],
          layout: { "line-cap": "round", "line-join": "round" },
          paint: { "line-color": this.flavor === "dark" ? "#1f2933" : "#ffffff", "line-width": 9 },
        },
        labelLayer,
      );
      m.addLayer(
        {
          id: "rm-route-alt",
          type: "line",
          source: ROUTE_SRC,
          filter: ["==", ["get", "selected"], false],
          layout: { "line-cap": "round", "line-join": "round" },
          paint: { "line-color": alt, "line-width": 6 },
        },
        labelLayer,
      );
      m.addLayer(
        {
          id: "rm-route-sel-casing",
          type: "line",
          source: ROUTE_SRC,
          filter: ["==", ["get", "selected"], true],
          layout: { "line-cap": "round", "line-join": "round" },
          paint: { "line-color": "#0b4fbd", "line-width": 10 },
        },
        labelLayer,
      );
      m.addLayer(
        {
          id: "rm-route-sel",
          type: "line",
          source: ROUTE_SRC,
          filter: ["==", ["get", "selected"], true],
          layout: { "line-cap": "round", "line-join": "round" },
          paint: { "line-color": "#2f7bf5", "line-width": 6.5 },
        },
        labelLayer,
      );
      // Saved places: gold stars, named when zoomed in.
      if (!m.hasImage("rm-star")) m.addImage("rm-star", starImage(), { pixelRatio: 2 });
      m.addSource(SAVED_SRC, { type: "geojson", data: this.savedData() });
      m.addLayer({
        id: "rm-saved",
        type: "symbol",
        source: SAVED_SRC,
        layout: {
          "icon-image": "rm-star",
          "icon-allow-overlap": true,
          "text-field": ["step", ["zoom"], "", 12, ["get", "name"]],
          "text-font": ["Noto Sans Medium"],
          "text-size": 12,
          "text-offset": [0, 1.3],
          "text-anchor": "top",
          "text-optional": true,
        },
        paint: { "text-color": "#6b4e00", "text-halo-color": "#ffffff", "text-halo-width": 1.5 },
      });

      // Search results: numbered pins, the hovered or chosen one larger.
      m.addSource(RESULT_SRC, { type: "geojson", data: this.resultData() });
      m.addLayer({
        id: "rm-result-dot",
        type: "circle",
        source: RESULT_SRC,
        paint: {
          "circle-radius": ["case", ["get", "hot"], 13, 10],
          "circle-color": ["case", ["get", "hot"], "#b3261e", "#d1453b"],
          "circle-stroke-color": "#ffffff",
          "circle-stroke-width": 2,
        },
      });
      m.addLayer({
        id: "rm-result-num",
        type: "symbol",
        source: RESULT_SRC,
        layout: {
          "text-field": ["get", "n"],
          "text-font": ["Noto Sans Medium"],
          "text-size": 12,
          "text-allow-overlap": true,
          "text-ignore-placement": true,
          "symbol-sort-key": ["get", "order"],
        },
        paint: { "text-color": "#ffffff" },
      });
      m.addLayer({
        id: "rm-point",
        type: "circle",
        source: POINT_SRC,
        paint: {
          "circle-radius": 7,
          "circle-color": "#ffffff",
          "circle-stroke-color": "#2f7bf5",
          "circle-stroke-width": 3,
        },
      });
    }
  }

  private savedData(): FeatureCollection {
    return {
      type: "FeatureCollection",
      features: this.saved.map((p) => ({
        type: "Feature" as const,
        properties: { id: p.id, name: p.name },
        geometry: { type: "Point" as const, coordinates: [p.lon, p.lat] },
      })),
    };
  }

  /** Stars for the address book (an empty list hides them). */
  setSaved(pins: SavedPin[]) {
    this.saved = pins;
    (this.map.getSource(SAVED_SRC) as GeoJSONSource | undefined)?.setData(this.savedData());
  }

  private resultData(): FeatureCollection {
    return {
      type: "FeatureCollection",
      features: this.results
        .map((h, i) => ({ h, i }))
        // the chosen one is shown by the place marker instead
        .filter(({ h }) => h.id !== this.resultSelected)
        // first results drawn on top
        .reverse()
        .map(({ h, i }) => ({
          type: "Feature" as const,
          properties: { id: h.id, n: String(i + 1), order: i, hot: h.id === this.resultHover },
          geometry: { type: "Point" as const, coordinates: [h.lon, h.lat] },
        })),
    };
  }

  private updateResults() {
    (this.map.getSource(RESULT_SRC) as GeoJSONSource | undefined)?.setData(this.resultData());
  }

  /** Shows search results as numbered pins. */
  setResults(hits: Hit[], selectedId: string | null = null) {
    this.results = hits;
    this.resultSelected = selectedId;
    this.updateResults();
  }

  /** Highlights the pin of a result (when its row is hovered in the list). */
  hoverResult(id: string | null) {
    this.resultHover = id;
    this.updateResults();
  }

  /** Frames a set of results: one is zoomed to, several are fitted. */
  fitHits(hits: Hit[], zoomForOne: number) {
    if (!hits.length) return;
    if (hits.length === 1) {
      this.flyTo(hits[0].lat, hits[0].lon, zoomForOne);
      return;
    }
    const b = new maplibregl.LngLatBounds([hits[0].lon, hits[0].lat], [hits[0].lon, hits[0].lat]);
    hits.forEach((h) => b.extend([h.lon, h.lat]));
    this.map.fitBounds(b, { padding: { top: 70, bottom: 50, left: 50, right: 50 }, maxZoom: 15 });
  }

  /** [west, south, east, north] of the visible map. */
  bounds(): Bbox {
    const b = this.map.getBounds();
    return [b.getWest(), b.getSouth(), b.getEast(), b.getNorth()];
  }

  private routeData(): FeatureCollection {
    const order = [...this.routes].sort((a, b) => Number(a.id === this.selected) - Number(b.id === this.selected));
    return {
      type: "FeatureCollection",
      features: order.map((r) => ({
        type: "Feature",
        properties: { id: r.id, selected: r.id === this.selected },
        geometry: { type: "LineString", coordinates: r.coordinates },
      })),
    };
  }

  private pointData(): FeatureCollection {
    return {
      type: "FeatureCollection",
      features: this.point
        ? [{ type: "Feature", properties: {}, geometry: { type: "Point", coordinates: this.point } }]
        : [],
    };
  }

  setFlavor(f: Flavor) {
    if (f === this.flavor) return;
    this.flavor = f;
    this.map.setStyle(buildStyle(f), { diff: false });
  }

  setRoutes(routes: RouteOption[], selected: number) {
    this.routes = routes;
    this.selected = selected;
    (this.map.getSource(ROUTE_SRC) as GeoJSONSource | undefined)?.setData(this.routeData());
  }

  highlight(lat: number | null, lon: number | null) {
    this.point = lat === null || lon === null ? null : [lon, lat];
    (this.map.getSource(POINT_SRC) as GeoJSONSource | undefined)?.setData(this.pointData());
  }

  showPlace(hit: Hit | null) {
    this.placeMarker?.remove();
    this.placeMarker = null;
    if (hit) {
      this.placeMarker = new maplibregl.Marker({ element: pinElement("rm-pin"), anchor: "bottom" })
        .setLngLat([hit.lon, hit.lat])
        .addTo(this.map);
    }
  }

  setWaypoints(points: (Waypoint | null)[], onDrag: (i: number, lat: number, lon: number) => void) {
    this.wpMarkers.forEach((m) => m.remove());
    this.wpMarkers = [];
    const last = points.length - 1;
    points.forEach((p, i) => {
      if (!p) return;
      const cls = i === 0 ? "rm-wp rm-wp-start" : i === last ? "rm-wp rm-wp-end" : "rm-wp rm-wp-stop";
      const text = i === 0 ? "A" : i === last ? "B" : String(i);
      const mk = new maplibregl.Marker({ element: pinElement(cls, text), draggable: true })
        .setLngLat([p.lon, p.lat])
        .addTo(this.map);
      mk.on("dragend", () => {
        const ll = mk.getLngLat();
        onDrag(i, ll.lat, ll.lng);
      });
      this.wpMarkers.push(mk);
    });
  }

  flyTo(lat: number, lon: number, zoom?: number) {
    this.map.flyTo({ center: [lon, lat], zoom: zoom ?? Math.max(this.map.getZoom(), 15), speed: 1.6 });
  }

  fitCoords(coords: [number, number][], padding = 60) {
    if (!coords.length) return;
    const b = new maplibregl.LngLatBounds(coords[0], coords[0]);
    coords.forEach((c) => b.extend(c));
    this.map.fitBounds(b, { padding: { top: padding, bottom: padding, left: padding, right: padding }, maxZoom: 16 });
  }

  fitBbox(b: Bbox) {
    this.map.fitBounds([b[0], b[1], b[2], b[3]] as LngLatBoundsLike, { padding: 30 });
  }

  /** Call after a map pack is installed or removed. */
  reloadTiles() {
    const url = bumpTileVersion();
    (this.map.getSource("protomaps") as VectorTileSource | undefined)?.setTiles([url]);
  }

  /** What the map shows now, for printing. */
  view(): { center: [number, number]; zoom: number; bearing: number; width: number; height: number } {
    const c = this.map.getCenter();
    const el = this.map.getContainer();
    return { center: [c.lng, c.lat], zoom: this.map.getZoom(), bearing: this.map.getBearing(), width: el.clientWidth, height: el.clientHeight };
  }

  center(): [number, number] {
    const c = this.map.getCenter();
    return [c.lat, c.lng];
  }

  destroy() {
    this.map.remove();
  }
}

/** A gold star with a white edge, drawn at 2x for sharp screens. */
function starImage(): ImageData {
  const size = 44;
  const c = document.createElement("canvas");
  c.width = c.height = size;
  const g = c.getContext("2d")!;
  const cx = size / 2;
  const cy = size / 2 + 1;
  g.beginPath();
  for (let i = 0; i < 10; i++) {
    const r = i % 2 === 0 ? 19 : 8.5;
    const a = -Math.PI / 2 + (i * Math.PI) / 5;
    g.lineTo(cx + r * Math.cos(a), cy + r * Math.sin(a));
  }
  g.closePath();
  g.lineJoin = "round";
  g.lineWidth = 4;
  g.strokeStyle = "#ffffff";
  g.stroke();
  g.fillStyle = "#f2b200";
  g.fill();
  g.lineWidth = 1.2;
  g.strokeStyle = "#a27400";
  g.stroke();
  return g.getImageData(0, 0, size, size);
}
