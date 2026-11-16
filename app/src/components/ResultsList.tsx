import type { Hit } from "../api";
import { formatKm, formatPhone, kindLabel, prettyCategory } from "../format";
import { CloseIcon } from "./Icons";

type Props = {
  query: string;
  hits: Hit[];
  scoped: boolean;
  hovered: string | null;
  onHover: (id: string | null) => void;
  onPick: (h: Hit) => void;
  onClose: () => void;
  onSearchAll: () => void;
};

/** Numbered results matching the pins on the map. */
export function ResultsList({ query, hits, scoped, hovered, onHover, onPick, onClose, onSearchAll }: Props) {
  return (
    <div className="results">
      <div className="results-head">
        <span>
          <strong>{hits.length ? `${hits.length} result${hits.length === 1 ? "" : "s"}` : "No results"}</strong>
          <span className="muted"> for “{query}”</span>
        </span>
        <button className="icon-btn subtle" title="Clear results" onClick={onClose}>
          <CloseIcon />
        </button>
      </div>
      <ol className="results-list">
        {hits.map((h, i) => (
          <li
            key={h.id}
            className={hovered === h.id ? "hot" : ""}
            onMouseEnter={() => onHover(h.id)}
            onMouseLeave={() => onHover(null)}
            onClick={() => onPick(h)}
          >
            <span className="result-num">{i + 1}</span>
            <div className="hit-text">
              <div className="hit-name">{h.name}</div>
              <div className="hit-label">
                {h.kind === "place" ? prettyCategory(h.category) || "Place" : kindLabel(h.kind)}
                {h.label ? ` · ${h.label}` : ""}
                {h.approximate ? " · approximate" : ""}
              </div>
              {h.phone && <div className="hit-label">{formatPhone(h.phone)}</div>}
            </div>
            {h.distance_km !== undefined && <div className="hit-dist">{formatKm(h.distance_km)}</div>}
          </li>
        ))}
      </ol>
      {scoped && (
        <button className="link-btn small" onClick={onSearchAll}>
          Search all states for “{query}”
        </button>
      )}
    </div>
  );
}
