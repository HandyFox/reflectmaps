import maplibregl, { type LngLatBoundsLike } from "maplibre-gl";
import type { FeatureCollection } from "geojson";
import { buildStyle } from "./style";

export type SnapshotPin = { lon: number; lat: number; label: string; color: string };

export type SnapshotOptions = {
  /** Size of the map in CSS pixels; labels and lines are drawn at this scale. */
  width: number;
  height: number;
  /** Device pixels per CSS pixel: higher gives a sharper print. */
  pixelRatio: number;
  center?: [number, number];
  zoom?: number;
  bearing?: number;
  /** Fit these bounds instead of center/zoom. */
  bounds?: [number, number, number, number];
  padding?: number;
  /** A route line to draw. */
  line?: [number, number][];
  pins?: SnapshotPin[];
};

export type Snapshot = {
  url: string;
  /** Map scale at the center, meters per CSS pixel of the snapshot. */
  metersPerPixel: number;
  bearing: number;
  width: number;
  height: number;
};

/**
 * Renders the map off screen, always in the light style, and returns it as
 * a PNG. Used for printing, where the screen's canvas is the wrong size and
 * too coarse.
 */
export function renderSnapshot(o: SnapshotOptions): Promise<Snapshot> {
  const box = document.createElement("div");
  // Off screen but laid out, so MapLibre gets a real size.
  box.style.cssText = `position:fixed;left:-20000px;top:0;width:${o.width}px;height:${o.height}px;pointer-events:none;`;
  document.body.appendChild(box);
  const map = new maplibregl.Map({
    container: box,
    style: buildStyle("light"),
    interactive: false,
    attributionControl: false,
    pixelRatio: o.pixelRatio,
    fadeDuration: 0,
    canvasContextAttributes: { preserveDrawingBuffer: true, antialias: true },
    center: o.center ?? [-96.5, 38.6],
    zoom: o.zoom ?? 4,
    bearing: o.bearing ?? 0,
  });
  if (o.bounds) {
    const p = o.padding ?? 40;
    map.fitBounds(o.bounds as LngLatBoundsLike, { padding: p, maxZoom: 16, animate: false });
  }

  const finish = (resolve: (s: Snapshot) => void, reject: (e: Error) => void) => {
    try {
      const url = map.getCanvas().toDataURL("image/png");
      const c = map.getCenter();
      const metersPerPixel = (40075016.686 * Math.cos((c.lat * Math.PI) / 180)) / (512 * 2 ** map.getZoom());
      resolve({ url, metersPerPixel, bearing: map.getBearing(), width: o.width, height: o.height });
    } catch (e) {
      reject(e instanceof Error ? e : new Error(String(e)));
    } finally {
      map.remove();
      box.remove();
    }
  };

  return new Promise((resolve, reject) => {
    let done = false;
    // Never wait forever, e.g. for a tile that fails to load.
    const timer = setTimeout(() => {
      if (!done) {
        done = true;
        finish(resolve, reject);
      }
    }, 30000);
    map.on("load", () => {
      addOverlays(map, o);
      map.once("idle", () => {
        if (done) return;
        done = true;
        clearTimeout(timer);
        finish(resolve, reject);
      });
    });
    map.on("error", (e) => console.warn("print map:", e.error?.message ?? e));
  });
}

function addOverlays(map: maplibregl.Map, o: SnapshotOptions) {
  const labelLayer = map.getStyle().layers.find((l) => l.type === "symbol")?.id;
  if (o.line && o.line.length > 1) {
    const data: FeatureCollection = {
      type: "FeatureCollection",
      features: [{ type: "Feature", properties: {}, geometry: { type: "LineString", coordinates: o.line } }],
    };
    map.addSource("print-route", { type: "geojson", data });
    map.addLayer(
      {
        id: "print-route-casing",
        type: "line",
        source: "print-route",
        layout: { "line-cap": "round", "line-join": "round" },
        paint: { "line-color": "#0b3f99", "line-width": 8 },
      },
      labelLayer,
    );
    map.addLayer(
      {
        id: "print-route",
        type: "line",
        source: "print-route",
        layout: { "line-cap": "round", "line-join": "round" },
        paint: { "line-color": "#2f7bf5", "line-width": 5 },
      },
      labelLayer,
    );
  }
  if (o.pins?.length) {
    const data: FeatureCollection = {
      type: "FeatureCollection",
      features: o.pins.map((p) => ({
        type: "Feature",
        properties: { label: p.label, color: p.color },
        geometry: { type: "Point", coordinates: [p.lon, p.lat] },
      })),
    };
    map.addSource("print-pins", { type: "geojson", data });
    map.addLayer({
      id: "print-pins",
      type: "circle",
      source: "print-pins",
      paint: {
        "circle-radius": 11,
        "circle-color": ["get", "color"],
        "circle-stroke-color": "#ffffff",
        "circle-stroke-width": 2.5,
      },
    });
    map.addLayer({
      id: "print-pin-labels",
      type: "symbol",
      source: "print-pins",
      layout: {
        "text-field": ["get", "label"],
        "text-font": ["Noto Sans Medium"],
        "text-size": 13,
        "text-allow-overlap": true,
        "text-ignore-placement": true,
      },
      paint: { "text-color": "#ffffff" },
    });
  }
}

/** A round scale-bar length (in feet or miles) no longer than maxMeters. */
export function niceScale(maxMeters: number): { meters: number; label: string } {
  const feet = maxMeters / 0.3048;
  if (feet < 2640) {
    const steps = [50, 100, 200, 250, 500, 1000, 2000];
    const f = [...steps].reverse().find((s) => s <= feet) ?? 50;
    return { meters: f * 0.3048, label: `${f.toLocaleString()} ft` };
  }
  const miles = maxMeters / 1609.344;
  const steps = [0.5, 1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000];
  const m = [...steps].reverse().find((s) => s <= miles) ?? 0.5;
  return { meters: m * 1609.344, label: `${m} mi` };
}
