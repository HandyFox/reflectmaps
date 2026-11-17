import { useCallback, useEffect, useRef, useState } from "react";
import { api, type HistoryItem, type Hit, type SavedPlace } from "./api";

const HISTORY_MAX = 50;

/** Same spot: within about 15 m. */
export const samePlace = (a: Hit, b: Hit) =>
  a.id === b.id || (Math.abs(a.lat - b.lat) < 0.00015 && Math.abs(a.lon - b.lon) < 0.00015 && a.name === b.name);

/** A saved place as a search hit, under the user's name for it. */
export function savedAsHit(p: SavedPlace): Hit {
  const orig = p.hit;
  const detail = orig.name !== p.name ? [orig.name, orig.label].filter(Boolean).join(", ") : orig.label;
  return { ...orig, name: p.name, label: detail };
}

/** Points the user dropped on the map have no name worth remembering. */
const memorable = (h: Hit) => h.kind !== "coordinates";

/**
 * Address history and address book, kept in the data folder through the
 * app core (so a portable drive carries them).
 */
export function useUserData() {
  const [history, setHistory] = useState<HistoryItem[]>([]);
  const [places, setPlaces] = useState<SavedPlace[]>([]);
  const hist = useRef<HistoryItem[]>([]);
  const book = useRef<SavedPlace[]>([]);

  const reload = useCallback(async () => {
    const [h, p] = await Promise.all([
      api.userGet<HistoryItem[]>("history").catch(() => []),
      api.userGet<SavedPlace[]>("places").catch(() => []),
    ]);
    hist.current = Array.isArray(h) ? h : [];
    book.current = Array.isArray(p) ? p : [];
    setHistory(hist.current);
    setPlaces(book.current);
  }, []);

  useEffect(() => {
    reload();
  }, [reload]);

  const saveHistory = (next: HistoryItem[]) => {
    hist.current = next;
    setHistory(next);
    api.userSet("history", next).catch(() => {});
  };
  const saveBook = (next: SavedPlace[]) => {
    book.current = next;
    setPlaces(next);
    api.userSet("places", next).catch(() => {});
  };

  /** Puts places at the top of the history (newest first, no repeats). */
  const remember = useCallback((hits: Hit[]) => {
    const keep = hits.filter(memorable);
    if (!keep.length) return;
    const now = Date.now();
    let next = hist.current.filter((x) => !keep.some((h) => samePlace(h, x.hit)));
    next = [...keep.map((hit) => ({ hit: { ...hit, distance_km: undefined, score: 0 }, at: now })), ...next];
    saveHistory(next.slice(0, HISTORY_MAX));
  }, []);

  const forget = useCallback((h: Hit) => saveHistory(hist.current.filter((x) => !samePlace(x.hit, h))), []);
  const clearHistory = useCallback(() => saveHistory([]), []);

  const savedFor = useCallback((h: Hit) => book.current.find((p) => samePlace(p.hit, h) || p.hit.id === h.id), []);

  const save = useCallback((h: Hit, name: string) => {
    const existing = book.current.find((p) => samePlace(p.hit, h));
    if (existing) {
      saveBook(book.current.map((p) => (p.id === existing.id ? { ...p, name } : p)));
      return;
    }
    const entry: SavedPlace = {
      id: `p${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`,
      name: name.trim() || h.name,
      hit: { ...h, distance_km: undefined, score: 0 },
      added: Date.now(),
    };
    saveBook([...book.current, entry]);
  }, []);

  const rename = useCallback((id: string, name: string) => {
    saveBook(book.current.map((p) => (p.id === id ? { ...p, name: name.trim() || p.name } : p)));
  }, []);

  const unsave = useCallback((id: string) => saveBook(book.current.filter((p) => p.id !== id)), []);

  const clearAll = useCallback(async () => {
    await api.userClear();
    await reload();
  }, [reload]);

  return { history, places, remember, forget, clearHistory, savedFor, save, rename, unsave, reload, clearAll };
}

export type UserData = ReturnType<typeof useUserData>;
