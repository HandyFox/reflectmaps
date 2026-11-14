import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import type { RouteOption } from "../api";
import { formatDuration, formatMiles } from "../format";
import { niceScale, renderSnapshot, type Snapshot, type SnapshotPin } from "../map/snapshot";
import { CloseIcon } from "./Icons";

export type PrintStop = { name: string; label: string };

export type PrintJob =
  | {
      kind: "map";
      title: string;
      center: [number, number];
      zoom: number;
      bearing: number;
      /** Width of the map on screen, so the printout covers about the same area. */
      viewWidth: number;
      pins: SnapshotPin[];
      line?: [number, number][];
    }
  | { kind: "directions"; route: RouteOption; stops: PrintStop[]; pins: SnapshotPin[] };

type Paper = "letter" | "legal" | "tabloid" | "a4";
const PAPERS: Record<Paper, { label: string; w: number; h: number }> = {
  letter: { label: "Letter (8.5 × 11 in)", w: 8.5, h: 11 },
  legal: { label: "Legal (8.5 × 14 in)", w: 8.5, h: 14 },
  tabloid: { label: "Tabloid (11 × 17 in)", w: 11, h: 17 },
  a4: { label: "A4", w: 8.27, h: 11.69 },
};
const MARGIN = 0.5; // inches on every side
const HEADER = 0.75; // title block above a printed map
const FOOTER = 0.35; // credit line below it

type Layout = { pageW: number; pageH: number; boxW: number; boxH: number; mapH: number };

function layout(job: PrintJob, paper: Paper, landscape: boolean): Layout {
  const p = PAPERS[paper];
  const pageW = landscape ? p.h : p.w;
  const pageH = landscape ? p.w : p.h;
  const boxW = pageW - 2 * MARGIN;
  // A hair under the printable height so a one-page map never spills over.
  const boxH = pageH - 2 * MARGIN - 0.06;
  const mapH = job.kind === "map" ? boxH - HEADER - FOOTER : Math.min(boxH * 0.48, boxW * 0.75);
  return { pageW, pageH, boxW, boxH, mapH };
}

function printRoot(): HTMLElement {
  let el = document.getElementById("print-root");
  if (!el) {
    el = document.createElement("div");
    el.id = "print-root";
    document.body.appendChild(el);
  }
  return el;
}

const today = () => new Date().toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" });

type Props = { job: PrintJob; onClose: () => void };

export function PrintDialog({ job, onClose }: Props) {
  const [paper, setPaper] = useState<Paper>(() => (localStorage.getItem("rm.paper") as Paper) || "letter");
  const [landscape, setLandscape] = useState(job.kind === "map");
  const [title, setTitle] = useState(job.kind === "map" ? job.title : "");
  const [withMap, setWithMap] = useState(true);
  const [withSteps, setWithSteps] = useState(true);
  const [bigText, setBigText] = useState(false);
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [rendering, setRendering] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const lay = useMemo(() => layout(job, paper, landscape), [job, paper, landscape]);

  useEffect(() => {
    try {
      localStorage.setItem("rm.paper", paper);
    } catch {
      /* not essential */
    }
  }, [paper]);

  // Render the map image for the chosen paper shape.
  useEffect(() => {
    if (job.kind === "directions" && !withMap) return;
    let cancelled = false;
    setRendering(true);
    setError(null);
    const aspect = lay.boxW / lay.mapH;
    const opts =
      job.kind === "map"
        ? (() => {
            const width = Math.round(Math.min(1600, Math.max(700, job.viewWidth)));
            return {
              width,
              height: Math.round(width / aspect),
              center: job.center,
              zoom: job.zoom,
              bearing: job.bearing,
              line: job.line,
              pins: job.pins,
            };
          })()
        : (() => {
            const xs = job.route.coordinates.map((c) => c[0]);
            const ys = job.route.coordinates.map((c) => c[1]);
            const width = 1000;
            return {
              width,
              height: Math.round(width / aspect),
              bounds: [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)] as [number, number, number, number],
              padding: 50,
              line: job.route.coordinates,
              pins: job.pins,
            };
          })();
    // About 200 dots per inch on paper, within what graphics cards allow.
    const want = (lay.boxW * 200) / opts.width;
    const cap = 4096 / Math.max(opts.width, opts.height);
    const pixelRatio = Math.max(1, Math.min(3, want, cap));
    renderSnapshot({ ...opts, pixelRatio })
      .then((s) => !cancelled && setSnap(s))
      .catch((e) => !cancelled && setError(`Could not draw the map: ${e}`))
      .finally(() => !cancelled && setRendering(false));
    return () => {
      cancelled = true;
    };
  }, [job, lay, withMap]);

  // Paper size and orientation for the print dialog.
  useEffect(() => {
    const style = document.createElement("style");
    style.id = "print-page-style";
    style.textContent = `@page { size: ${lay.pageW}in ${lay.pageH}in; margin: ${MARGIN}in; }`;
    document.head.appendChild(style);
    return () => style.remove();
  }, [lay]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const print = async () => {
    try {
      await invoke("print_page");
    } catch {
      window.print();
    }
  };

  const sheet = <Sheet job={job} lay={lay} snap={snap} title={title} withMap={withMap} withSteps={withSteps} bigText={bigText} />;
  const previewScale = Math.min(560 / (lay.pageW * 96), 1);
  const busy = rendering && (job.kind === "map" || withMap);

  return (
    <>
      <div className="modal-backdrop" onClick={onClose}>
        <div className="modal print-dialog" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Print">
          <div className="print-options">
            <div className="print-options-head">
              <h2>{job.kind === "map" ? "Print map" : "Print directions"}</h2>
              <button className="icon-btn" onClick={onClose} title="Close">
                <CloseIcon />
              </button>
            </div>
            {job.kind === "map" && (
              <label className="field">
                <span>Title</span>
                <input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Map title" />
              </label>
            )}
            <label className="field">
              <span>Paper</span>
              <select value={paper} onChange={(e) => setPaper(e.target.value as Paper)}>
                {Object.entries(PAPERS).map(([k, v]) => (
                  <option key={k} value={k}>{v.label}</option>
                ))}
              </select>
            </label>
            <div className="field">
              <span>Orientation</span>
              <div className="seg">
                <button className={!landscape ? "on" : ""} onClick={() => setLandscape(false)}>Portrait</button>
                <button className={landscape ? "on" : ""} onClick={() => setLandscape(true)}>Landscape</button>
              </div>
            </div>
            {job.kind === "directions" && (
              <div className="field checks">
                <label><input type="checkbox" checked={withMap} onChange={(e) => setWithMap(e.target.checked)} /> Overview map</label>
                <label><input type="checkbox" checked={withSteps} onChange={(e) => setWithSteps(e.target.checked)} /> Turn-by-turn steps</label>
                <label><input type="checkbox" checked={bigText} onChange={(e) => setBigText(e.target.checked)} /> Large text</label>
              </div>
            )}
            {job.kind === "map" && (
              <p className="muted small">
                Prints the area you see on screen, shaped to the paper. Zoom and move the map first to change it.
              </p>
            )}
            {error && <div className="note error small">{error}</div>}
            <div className="print-actions">
              <button className="btn primary" onClick={print} disabled={busy}>
                {busy ? "Preparing map…" : "Print…"}
              </button>
              <button className="btn" onClick={onClose}>Cancel</button>
            </div>
            <p className="muted small">
              To make a PDF, choose “Save as PDF” or “Microsoft Print to PDF” as the printer. If the page comes out
              cut off, pick the same paper size and orientation in the print dialog.
            </p>
          </div>
          <div className="print-preview">
            <div
              className="paper"
              style={{
                // zoom (unlike transform) also shrinks the layout box
                zoom: previewScale,
                width: `${lay.pageW}in`,
                minHeight: `${lay.pageH}in`,
                padding: `${MARGIN}in`,
              }}
            >
              {sheet}
            </div>
          </div>
        </div>
      </div>
      {createPortal(sheet, printRoot())}
    </>
  );
}

type SheetProps = {
  job: PrintJob;
  lay: Layout;
  snap: Snapshot | null;
  title: string;
  withMap: boolean;
  withSteps: boolean;
  bigText: boolean;
};

function MapFigure({ snap, w, h }: { snap: Snapshot | null; w: number; h: number }) {
  const scale = snap ? niceScale(snap.metersPerPixel * snap.width * 0.25) : null;
  return (
    <div className="ps-map" style={{ width: `${w}in`, height: `${h}in` }}>
      {snap ? <img src={snap.url} alt="" /> : <div className="ps-map-wait">Drawing map…</div>}
      {snap && scale && (
        <div className="ps-scale">
          <div className="ps-scale-bar" style={{ width: `${(scale.meters / snap.metersPerPixel / snap.width) * 100}%` }} />
          <span>{scale.label}</span>
        </div>
      )}
      {snap && (
        <div className="ps-north" style={{ transform: `rotate(${-snap.bearing}deg)` }} title="North">
          <svg viewBox="0 0 24 32" width="18" height="24" aria-hidden="true">
            <path d="M12 2 20 26 12 21 4 26Z" fill="#111" />
            <text x="12" y="32" fontSize="8" textAnchor="middle" fill="#111" fontFamily="Arial">N</text>
          </svg>
        </div>
      )}
    </div>
  );
}

const CREDIT = "Map data © OpenStreetMap contributors, Overture Maps Foundation · Protomaps · Printed from ReflectMaps";

function Sheet({ job, lay, snap, title, withMap, withSteps, bigText }: SheetProps) {
  if (job.kind === "map") {
    return (
      <div className="print-sheet" style={{ width: `${lay.boxW}in`, height: `${lay.boxH}in` }}>
        <header className="ps-head" style={{ height: `${HEADER}in` }}>
          <h1>{title || "Map"}</h1>
          <div className="ps-sub">
            {today()} · center {job.center[1].toFixed(4)}, {job.center[0].toFixed(4)}
          </div>
        </header>
        <MapFigure snap={snap} w={lay.boxW} h={lay.mapH} />
        <footer className="ps-foot" style={{ height: `${FOOTER}in` }}>{CREDIT}</footer>
      </div>
    );
  }
  const r = job.route;
  const to = job.stops[job.stops.length - 1];
  let total = 0;
  return (
    <div className={`print-sheet directions${bigText ? " big" : ""}`} style={{ width: `${lay.boxW}in` }}>
      <header className="ps-head">
        <h1>Directions to {to?.name || "destination"}</h1>
        <div className="ps-summary">
          <strong>{formatDuration(r.time_s)}</strong> · {formatMiles(r.distance_mi)}
          {r.via ? ` · via ${r.via}` : ""}
          {r.has_toll ? " · tolls" : ""}
          {r.has_ferry ? " · ferry" : ""}
        </div>
        <ol className="ps-stops">
          {job.stops.map((s, i) => (
            <li key={i}>
              <span className="ps-stop-mark" style={{ background: job.pins[i]?.color }}>{job.pins[i]?.label}</span>
              <span>
                <strong>{s.name}</strong>
                {s.label && s.label !== s.name ? <span className="ps-muted"> · {s.label}</span> : null}
              </span>
            </li>
          ))}
        </ol>
        {r.warnings.map((w) => (
          <div key={w} className="ps-warn">{w}</div>
        ))}
      </header>
      {withMap && <MapFigure snap={snap} w={lay.boxW} h={lay.mapH} />}
      {withSteps && (
        <table className="ps-steps">
          <thead>
            <tr>
              <th className="n">#</th>
              <th>Step</th>
              <th className="d">Go</th>
              <th className="d">At mile</th>
            </tr>
          </thead>
          <tbody>
            {r.maneuvers.map((m, i) => {
              const at = total;
              total += m.distance_mi;
              return (
                <tr key={i}>
                  <td className="n">{i + 1}</td>
                  <td>{m.instruction}</td>
                  <td className="d">{m.distance_mi > 0 ? formatMiles(m.distance_mi) : ""}</td>
                  <td className="d ps-muted">{i > 0 ? at.toFixed(1) : ""}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      <footer className="ps-foot">
        Printed {today()} from ReflectMaps (offline map data). Road conditions and signs take priority over these
        directions. {CREDIT.replace(" · Printed from ReflectMaps", "")}
      </footer>
    </div>
  );
}
