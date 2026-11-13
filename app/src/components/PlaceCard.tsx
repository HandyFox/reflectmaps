import { openUrl } from "@tauri-apps/plugin-opener";
import type { Hit } from "../api";
import { kindLabel, prettyCategory } from "../format";
import { CloseIcon, GlobeIcon, PhoneIcon, PinIcon, RouteIcon } from "./Icons";

type Props = {
  hit: Hit;
  onClose: () => void;
  onDirections: (hit: Hit) => void;
};

export function PlaceCard({ hit, onClose, onDirections }: Props) {
  const sub = hit.kind === "place" ? prettyCategory(hit.category) : kindLabel(hit.kind);
  const coords = `${hit.lat.toFixed(5)}, ${hit.lon.toFixed(5)}`;
  return (
    <div className="card place-card">
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
        {hit.phone && (
          <li>
            <PhoneIcon /> <span>{formatPhone(hit.phone)}</span>
          </li>
        )}
        {hit.website && (
          <li>
            <GlobeIcon />{" "}
            <a href="#" onClick={(e) => { e.preventDefault(); openUrl(hit.website!).catch(() => {}); }}>
              {hit.website.replace(/^https?:\/\/(www\.)?/, "").replace(/\/$/, "")}
            </a>
          </li>
        )}
        <li className="muted small">{coords}</li>
      </ul>
    </div>
  );
}

function formatPhone(p: string): string {
  const d = p.replace(/\D/g, "");
  const n = d.length === 11 && d.startsWith("1") ? d.slice(1) : d;
  return n.length === 10 ? `(${n.slice(0, 3)}) ${n.slice(3, 6)}-${n.slice(6)}` : p;
}
