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
    for (const id of ["rm-route-alt", "rm-route-sel"]) {
      this.map.on("mouseenter", id, () => (this.map.getCanvas().style.cursor = "pointer"));
      this.map.on("mouseleave", id, () => (this.map.getCanvas().style.cursor = ""));
    }
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

  center(): [number, number] {
    const c = this.map.getCenter();
    return [c.lat, c.lng];
  }

  destroy() {
    this.map.remove();
  }
}
