import type { ReactNode } from "react";
import type { Hit, Maneuver, RouteOption } from "../api";
import { formatDuration, formatMiles } from "../format";
import { CloseIcon, PlusIcon, SwapIcon } from "./Icons";
import { SearchBox } from "./SearchBox";

export type AvoidOptions = { avoid_tolls: boolean; avoid_highways: boolean; avoid_ferries: boolean };

type Props = {
  waypoints: (Hit | null)[];
  setWaypoints: (w: (Hit | null)[]) => void;
  avoid: AvoidOptions;
  setAvoid: (a: AvoidOptions) => void;
  routes: RouteOption[];
  selected: number;
  onSelect: (id: number) => void;
  loading: boolean;
  error: string | null;
  near: () => [number, number];
  /** States the From/To boxes search (null = all). */
  states: () => string[] | null;
  /** Route searches finished so far, while routing. */
  progress: { done: number; total: number } | null;
  scopePicker: ReactNode;
  onStep: (m: Maneuver) => void;
  onPrint: () => void;
};

const MAX_POINTS = 6;

export function Directions(p: Props) {
  const { waypoints, setWaypoints } = p;
  const update = (i: number, h: Hit | null) => {
    const next = [...waypoints];
    next[i] = h;
    setWaypoints(next);
  };
  const route = p.routes.find((r) => r.id === p.selected);
  const last = waypoints.length - 1;

  return (
    <div className="directions">
      <div className="waypoints">
        {waypoints.map((w, i) => (
          <div className="wp-row" key={i}>
            <SearchBox
              compact
              dot={i === 0 ? "start" : i === last ? "end" : "stop"}
              placeholder={i === 0 ? "Choose starting point" : i === last ? "Choose destination" : `Stop ${i}`}
              value={w?.name ?? ""}
              near={p.near}
              states={p.states}
              channel={`wp${i}`}
              onSelect={(h) => update(i, h)}
              onClear={() => update(i, null)}
            />
            {waypoints.length > 2 && (
              <button
                className="icon-btn subtle"
                title="Remove stop"
                onClick={() => setWaypoints(waypoints.filter((_, j) => j !== i))}
              >
                <CloseIcon />
              </button>
            )}
          </div>
        ))}
        <div className="wp-tools">
          {waypoints.length < MAX_POINTS && (
            <button className="link-btn" onClick={() => setWaypoints([...waypoints.slice(0, last), null, waypoints[last]])}>
              <PlusIcon /> Add stop
            </button>
          )}
          <button className="link-btn" title="Reverse" onClick={() => setWaypoints([...waypoints].reverse())}>
            <SwapIcon size={16} /> Reverse
          </button>
        </div>
      </div>

      {p.scopePicker}

      <div className="avoid">
        <span className="muted small">Avoid</span>
        {(
          [
            ["avoid_tolls", "Tolls"],
            ["avoid_highways", "Highways"],
            ["avoid_ferries", "Ferries"],
          ] as const
        ).map(([k, label]) => (
          <label key={k} className={`chip ${p.avoid[k] ? "on" : ""}`}>
            <input type="checkbox" checked={p.avoid[k]} onChange={(e) => p.setAvoid({ ...p.avoid, [k]: e.target.checked })} />
            {label}
          </label>
        ))}
      </div>

      {p.loading && (
        <div className="search-progress pad-y" role="progressbar">
          <div className="bar thin">
            <div
              className={p.progress && p.progress.total ? "" : "indeterminate"}
              style={{
                width: p.progress && p.progress.total ? `${Math.max(8, (p.progress.done / p.progress.total) * 100)}%` : undefined,
              }}
            />
          </div>
          <span className="muted small">
            Finding routes…
            {p.progress && p.progress.total > 1
              ? ` ${p.progress.done} of ${p.progress.total} route searches done`
              : ""}
          </span>
        </div>
      )}
      {p.error && !p.loading && <div className="note error">{p.error}</div>}

      {!p.loading && p.routes.length > 0 && (
        <>
          <ul className="route-list">
            {p.routes.map((r) => (
              <li key={r.id} className={r.id === p.selected ? "selected" : ""} onClick={() => p.onSelect(r.id)}>
                <div className="route-main">
                  <span className="route-time">{formatDuration(r.time_s)}</span>
                  <span className="route-dist">{formatMiles(r.distance_mi)}</span>
                </div>
                <div className="route-sub">
                  <strong>{r.label}</strong>
                  {r.via ? ` · via ${r.via}` : ""}
                  {r.has_toll ? " · tolls" : ""}
                  {r.has_ferry ? " · ferry" : ""}
                </div>
                {r.warnings.map((w) => (
                  <div key={w} className="route-warn">{w}</div>
                ))}
              </li>
            ))}
          </ul>
          {route && (
            <div className="steps">
              <div className="steps-head">
                <span>Steps</span>
                <button className="link-btn" onClick={p.onPrint}>Print directions</button>
              </div>
              <ol>
                {route.maneuvers.map((m, i) => (
                  <li key={i} onClick={() => p.onStep(m)}>
                    <span className="step-text">{m.instruction}</span>
                    {m.distance_mi > 0 && <span className="step-dist">{formatMiles(m.distance_mi)}</span>}
                  </li>
                ))}
              </ol>
            </div>
          )}
        </>
      )}
      {!p.loading && !p.error && p.routes.length === 0 && (
        <p className="muted pad small">
          Search for a start and a destination, or right-click the map and choose “Directions from here”.
          Drag the A and B pins to adjust.
        </p>
      )}
    </div>
  );
}
