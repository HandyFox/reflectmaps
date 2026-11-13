import { layers, namedFlavor } from "@protomaps/basemaps";
import type { StyleSpecification } from "maplibre-gl";

export type Flavor = "light" | "dark";

/** Bumped after a map pack is installed so MapLibre drops cached empty tiles. */
let tileVersion = 0;
export function bumpTileVersion() {
  tileVersion += 1;
  return tileUrl();
}
export function tileUrl() {
  return `rm://tiles/{z}/{x}/{y}?v=${tileVersion}`;
}

export const ATTRIBUTION =
  '<a href="https://www.openstreetmap.org/copyright">© OpenStreetMap contributors</a> · ' +
  '<a href="https://protomaps.com">Protomaps</a> · <a href="https://overturemaps.org">Overture Maps</a>';

export function buildStyle(flavor: Flavor): StyleSpecification {
  const assets = `${window.location.origin}/basemaps-assets`;
  return {
    version: 8,
    glyphs: `${assets}/fonts/{fontstack}/{range}.pbf`,
    sprite: `${assets}/sprites/v4/${flavor}`,
    sources: {
      protomaps: {
        type: "vector",
        tiles: [tileUrl()],
        maxzoom: 15,
        attribution: ATTRIBUTION,
      },
    },
    layers: layers("protomaps", namedFlavor(flavor), { lang: "en" }),
  };
}
