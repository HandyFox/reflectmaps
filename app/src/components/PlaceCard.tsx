import { openUrl } from "@tauri-apps/plugin-opener";
import type { Hit } from "../api";
import { formatPhone, kindLabel, prettyCategory } from "../format";
import { CloseIcon, GlobeIcon, PhoneIcon, PinIcon, RouteIcon } from "./Icons";

type Props = {
  hit: Hit;
  onClose: () => void;
  onDirections: (hit: Hit) => void;
  /** Shown when the place was opened from a list of results. */
  onBack?: () => void;
};

export function PlaceCard({ hit, onClose, onDirections, onBack }: Props) {
  const sub = hit.kind === "place" ? prettyCategory(hit.category) : kindLabel(hit.kind);
  const coords = `${hit.lat.toFixed(5)}, ${hit.lon.toFixed(5)}`;
  return (
    <div className="card place-card">
      {onBack && (
        <button className="link-btn small back-btn" onClick={onBack}>
          ← Back to results
        </button>
      )}
      <div className="card-head">
        <div>
          <h2>{hit.name}</h2>
          {sub && <div className="muted">{sub}</div>}
        </div>
        <button className="icon-btn subtle" title="Close" onClick={onClose}>
          <CloseIcon />
        </button>
      </div>
      {hit.approximate && (
        <div className="note warn">
          That exact address was not found. The pin shows the street instead.
        </div>
      )}
      <div className="actions">
        <button className="btn primary" onClick={() => onDirections(hit)}>
          <RouteIcon /> Directions
        </button>
        <button className="btn" onClick={() => navigator.clipboard?.writeText(coords)}>
          Copy coordinates
        </button>
      </div>
      <ul className="facts">
        {hit.label && (
          <li>
            <PinIcon /> <span>{hit.label}</span>
          </li>
        )}
        {hit.kind === "place" && !hit.phone && !hit.website && (
          <li className="muted small">No phone number or website in the map data.</li>
        )}
        {hit.phone && (
          <li>
            <PhoneIcon />{" "}
            <span className="selectable">{formatPhone(hit.phone)}</span>
            <button className="link-btn small" onClick={() => navigator.clipboard?.writeText(formatPhone(hit.phone!))}>
              Copy
            </button>
          </li>
        )}
        {hit.website && (
          <li>
            <GlobeIcon />{" "}
            <a href="#" onClick={(e) => { e.preventDefault(); openUrl(hit.website!).catch(() => {}); }}>
              {hit.website.replace(/^https?:\/\/(www\.)?/, "").replace(/[?#].*$/, "").replace(/\/$/, "")}
            </a>
          </li>
        )}
        <li className="muted small">{coords}</li>
      </ul>
    </div>
  );
}
