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

export type ImportReport = {
  imported: string[];
  skipped: string[];
  already: string[];
  warnings: string[];
  catalog_updated: boolean;
  first_region?: string | null;
};

export type SearchProgress = { channel: string; seq: number; done: number; total: number };

/** True for the error of a search replaced by a newer one. */
export const isCancelled = (e: unknown) => String(e).includes("cancelled");

export type QueueStatus = {
  items: string[];
  active: string[];
  paused: boolean;
  failed: Record<string, string>;
  batch_total: number;
  batch_done: number;
  batch_states: number;
  batch_states_done: number;
  note?: string | null;
};

export type StorageInfo = {
  data_dir: string;
  portable: boolean;
  free?: number | null;
  total?: number | null;
  file_system?: string | null;
  warning?: string | null;
};

export type CheckProgress = { done: number; total: number; region: string };
export type RegionCheck = { region: string; name: string; status: "ok" | "damaged" | "unchecked"; problems: string[] };

export type RouteRequest = {
  locations: { lat: number; lon: number }[];
  avoid_tolls: boolean;
  avoid_highways: boolean;
  avoid_ferries: boolean;
};

export const api = {
  status: () => invoke<Status>("app_status"),
  tile: (z: number, x: number, y: number) => invoke<ArrayBuffer>("get_tile", { z, x, y }),
  /** `states` null = all; `channel` + `seq` let a newer search cancel an older one. */
  search: (
    query: string,
    near: [number, number] | null,
    limit = 8,
    states: string[] | null = null,
    channel = "default",
    seq = 0,
  ) => invoke<Hit[]>("search", { query, near, limit, states, channel, seq }),
  onSearchProgress: (cb: (p: SearchProgress) => void): Promise<UnlistenFn> =>
    listen<SearchProgress>("search-progress", (e) => cb(e.payload)),
  onRouteProgress: (cb: (p: { done: number; total: number }) => void): Promise<UnlistenFn> =>
    listen<{ done: number; total: number }>("route-progress", (e) => cb(e.payload)),
  reverse: (lat: number, lon: number) => invoke<Hit | null>("reverse", { lat, lon }),
  route: (request: RouteRequest) => invoke<RouteOption[]>("route", { request }),
  catalog: (refresh: boolean) => invoke<CatalogResult>("get_catalog", { refresh }),
  install: (region: string, components: string[]) => invoke<void>("install_region", { region, components }),
  cancel: (region: string) => invoke<void>("cancel_install", { region }),
  remove: (region: string) => invoke<void>("remove_region", { region }),
  importPacks: (paths: string[]) => invoke<ImportReport>("import_packs", { paths }),
  storage: () => invoke<StorageInfo>("storage_info"),
  queue: () => invoke<QueueStatus>("queue_status"),
  queueAdd: (regions: string[]) => invoke<number>("queue_add", { regions }),
  queueRemove: (regions: string[]) => invoke<void>("queue_remove", { regions }),
  queuePause: () => invoke<void>("queue_pause"),
  queueResume: () => invoke<void>("queue_resume"),
  queueClearFailed: () => invoke<void>("queue_clear_failed"),
  checkData: () => invoke<RegionCheck[]>("check_data"),
  cancelCheck: () => invoke<void>("cancel_check"),
  onQueue: (cb: (q: QueueStatus) => void): Promise<UnlistenFn> => listen<QueueStatus>("queue", (e) => cb(e.payload)),
  onPacksChanged: (cb: (region: string) => void): Promise<UnlistenFn> =>
    listen<string>("packs-changed", (e) => cb(e.payload)),
  onCheckProgress: (cb: (p: CheckProgress) => void): Promise<UnlistenFn> =>
    listen<CheckProgress>("check-progress", (e) => cb(e.payload)),
  onProgress: (cb: (p: Progress) => void): Promise<UnlistenFn> =>
    listen<Progress>("pack-progress", (e) => cb(e.payload)),
};
