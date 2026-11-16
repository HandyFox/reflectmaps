import { useEffect, useRef, useState } from "react";
import type { Bbox, InstalledRegion } from "../api";
import { scopeLabel, statesNear, type ScopeMode, type SearchScope } from "../searchScope";

type Props = {
  scope: SearchScope;
  setScope: (s: SearchScope) => void;
  regions: Record<string, InstalledRegion>;
  view: Bbox | null;
};

/** "Search in: ..." button and its menu of states. */
export function ScopePicker({ scope, setScope, regions, view }: Props) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState("");
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (box.current && !box.current.contains(e.target as Node)) setOpen(false);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", esc);
    };
  }, [open]);

  const all = Object.entries(regions).sort((a, b) => a[1].name.localeCompare(b[1].name));
  if (all.length < 2) return null;
  const near = new Set(statesNear(regions, view));
  const picked = new Set(scope.picked);
  const needle = filter.trim().toLowerCase();
  const shown = all.filter(([id, r]) => !needle || r.name.toLowerCase().includes(needle) || id === needle);

  const setMode = (mode: ScopeMode) => {
    // Picking for the first time starts from the states near the map.
    const start = mode === "pick" && scope.picked.length === 0 ? [...near] : scope.picked;
    setScope({ mode, picked: start });
  };
  const toggle = (id: string) => {
    const next = new Set(picked);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setScope({ mode: "pick", picked: [...next] });
  };

  return (
    <div className="scope" ref={box}>
      <button className="scope-btn" onClick={() => setOpen(!open)} title="Choose which states to search">
        <span className="muted">Search in:</span> <span className="scope-label">{scopeLabel(scope, regions, view)}</span>
        <span className="scope-caret">▾</span>
      </button>
      {open && (
        <div className="scope-menu">
          <label className="scope-opt">
            <input type="radio" checked={scope.mode === "near"} onChange={() => setMode("near")} />
            <span>
              <strong>Near the map</strong>
              <span className="muted small"> States in view and next to it; follows the map as you move it. Fastest.</span>
            </span>
          </label>
          <label className="scope-opt">
            <input type="radio" checked={scope.mode === "pick"} onChange={() => setMode("pick")} />
            <span>
              <strong>These states</strong>
              <span className="muted small"> Only the states you tick below.</span>
            </span>
          </label>
          <label className="scope-opt">
            <input type="radio" checked={scope.mode === "all"} onChange={() => setMode("all")} />
            <span>
              <strong>All states ({all.length})</strong>
              <span className="muted small"> Everything you downloaded. Slowest.</span>
            </span>
          </label>
          {scope.mode === "pick" && (
            <div className="scope-pick">
              <div className="scope-pick-tools">
                <input
                  type="search"
                  placeholder="Find a state"
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                  autoFocus
                />
                <button className="link-btn small" onClick={() => setScope({ mode: "pick", picked: [...near] })}>
                  Near map
                </button>
                <button className="link-btn small" onClick={() => setScope({ mode: "pick", picked: [] })}>
                  None
                </button>
              </div>
              <ul className="scope-list">
                {shown.map(([id, r]) => (
                  <li key={id}>
                    <label>
                      <input type="checkbox" checked={picked.has(id)} onChange={() => toggle(id)} />
                      {r.name}
                    </label>
                  </li>
                ))}
              </ul>
              <div className="muted small">
                {picked.size === 0 ? "Tick at least one state (none ticked searches all)." : `${picked.size} ticked`}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
