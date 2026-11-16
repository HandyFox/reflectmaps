import type { Bbox, InstalledRegion } from "./api";

/**
 * Which states a search reads.
 * - near: states in or next to the map view (follows the map)
 * - all: every installed state
 * - pick: a list the user ticked
 */
export type ScopeMode = "near" | "all" | "pick";
export type SearchScope = { mode: ScopeMode; picked: string[] };

const KEY = "rm.searchScope";

export function loadScope(): SearchScope {
  try {
    const s = JSON.parse(localStorage.getItem(KEY) ?? "");
    if (s && ["near", "all", "pick"].includes(s.mode) && Array.isArray(s.picked)) return s;
  } catch {
    /* default below */
  }
  return { mode: "near", picked: [] };
}

export function saveScope(s: SearchScope) {
  try {
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    /* not essential */
  }
}

/** Installed states touching the view, widened so neighbors just off screen count too. */
export function statesNear(regions: Record<string, InstalledRegion>, view: Bbox | null): string[] {
  if (!view) return Object.keys(regions);
  const [w, s, e, n] = view;
  const padX = Math.max((e - w) * 0.25, 0.4);
  const padY = Math.max((n - s) * 0.25, 0.3);
  const v: Bbox = [w - padX, s - padY, e + padX, n + padY];
  return Object.entries(regions)
    .filter(([, r]) => {
      const b = r.bbox;
      if (!b) return true; // unknown extent: include it
      return b[0] <= v[2] && b[2] >= v[0] && b[1] <= v[3] && b[3] >= v[1];
    })
    .map(([id]) => id);
}

/**
 * The state ids to search, or null for all of them. "Near the map" with
 * nothing near (e.g. looking at the ocean) falls back to all states.
 */
export function scopeStates(scope: SearchScope, regions: Record<string, InstalledRegion>, view: Bbox | null): string[] | null {
  const installed = Object.keys(regions);
  if (scope.mode === "all" || installed.length === 0) return null;
  const ids =
    scope.mode === "near" ? statesNear(regions, view) : scope.picked.filter((id) => regions[id]);
  if (!ids.length || ids.length >= installed.length) return null;
  return ids;
}

/** Short description for the picker button. */
export function scopeLabel(scope: SearchScope, regions: Record<string, InstalledRegion>, view: Bbox | null): string {
  const ids = scopeStates(scope, regions, view);
  const total = Object.keys(regions).length;
  const names = (list: string[]) => {
    const n = list.map((id) => regions[id]?.name ?? id.toUpperCase()).sort();
    return n.length <= 3 ? n.join(", ") : `${n.slice(0, 2).join(", ")} +${n.length - 2} more`;
  };
  if (scope.mode === "all") return `All states (${total})`;
  if (scope.mode === "near") return ids ? `Near the map: ${names(ids)}` : `Near the map: all ${total} states`;
  return ids ? names(ids) : `All states (${total})`;
}
