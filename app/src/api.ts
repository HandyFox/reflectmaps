import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Hit = {
  id: string;
  kind: string;
  name: string;
  label: string;
  category: string;
  lat: number;
  lon: number;
  distance_km?: number;
  phone?: string;
  website?: string;
  source: string;
  score: number;
  approximate: boolean;
};

export type Maneuver = {
  instruction: string;
  distance_mi: number;
  time_s: number;
  type: number;
  street_names: string[];
  lat: number;
  lon: number;
};

export type RouteOption = {
  id: number;
  label: string;
  via: string;
  distance_mi: number;
  time_s: number;
  has_toll: boolean;
  has_highway: boolean;
  has_ferry: boolean;
  warnings: string[];
  coordinates: [number, number][];
  maneuvers: Maneuver[];
};

export type Bbox = [number, number, number, number];

export type CatalogComponent = { file: string; size: number; sha256: string; url: string; kind: string };
export type CatalogRegion = { id: string; name: string; bbox?: Bbox; components: Record<string, CatalogComponent> };
export type Catalog = {
  schema: number;
  tag: string;
  built: string;
  routing_graph: string;
  base: Record<string, CatalogComponent>;
  regions: CatalogRegion[];
  sources: Record<string, string>;
  attribution: { text: string; url: string; license: string }[];
};
export type CatalogResult = { catalog: Catalog | null; online: boolean; error?: string | null };

export type InstalledComponent = {
  tag: string;
  file: string;
  size: number;
  sha256: string;
  path: string;
  graph_build: string;
  tiles: string[];
};
export type InstalledRegion = { name: string; bbox?: Bbox | null; components: Record<string, InstalledComponent> };

export type Status = {
  version: string;
  data_dir: string;
  installed: { regions: Record<string, InstalledRegion> };
  has_world: boolean;
  has_areas: boolean;
  routing_ready: boolean;
  routing_error?: string | null;
  attribution: string[];
};

export type Progress = {
  region: string;
  component: string;
  stage: "downloading" | "verifying" | "installing" | "done" | "error" | "cancelled";
  received: number;
  total: number;
  message?: string | null;
};

export type RouteRequest = {
  locations: { lat: number; lon: number }[];
  avoid_tolls: boolean;
  avoid_highways: boolean;
  avoid_ferries: boolean;
};

export const api = {
  status: () => invoke<Status>("app_status"),
  tile: (z: number, x: number, y: number) => invoke<ArrayBuffer>("get_tile", { z, x, y }),
  search: (query: string, near: [number, number] | null, limit = 8) =>
    invoke<Hit[]>("search", { query, near, limit }),
  reverse: (lat: number, lon: number) => invoke<Hit | null>("reverse", { lat, lon }),
  route: (request: RouteRequest) => invoke<RouteOption[]>("route", { request }),
  catalog: (refresh: boolean) => invoke<CatalogResult>("get_catalog", { refresh }),
  install: (region: string, components: string[]) => invoke<void>("install_region", { region, components }),
  cancel: (region: string) => invoke<void>("cancel_install", { region }),
  remove: (region: string) => invoke<void>("remove_region", { region }),
  importPack: (path: string) => invoke<string>("import_pack", { path }),
  onProgress: (cb: (p: Progress) => void): Promise<UnlistenFn> =>
    listen<Progress>("pack-progress", (e) => cb(e.payload)),
};
