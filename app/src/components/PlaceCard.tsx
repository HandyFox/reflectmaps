import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import type { Hit, SavedPlace } from "../api";
import { formatPhone, kindLabel, prettyCategory } from "../format";
import { CloseIcon, GlobeIcon, PhoneIcon, PinIcon, RouteIcon, StarIcon } from "./Icons";

type Props = {
  hit: Hit;
  onClose: () => void;
  onDirections: (hit: Hit) => void;
  /** Shown when the place was opened from a list of results. */
  onBack?: () => void;
  /** The address book entry for this place, if saved. */
  saved?: SavedPlace;
  onSave: (name: string) => void;
  onUnsave: (id: string) => void;
  /** Open with the name box showing (from "Save this place" on the map). */
  startSaving?: boolean;
};

export function PlaceCard({ hit, onClose, onDirections, onBack, saved, onSave, onUnsave, startSaving }: Props) {
  const sub = hit.kind === "place" ? prettyCategory(hit.category) : kindLabel(hit.kind);
  const [naming, setNaming] = useState(!!startSaving);
  const [name, setName] = useState(saved?.name ?? hit.name);
  useEffect(() => {
    setNaming(!!startSaving);
    setName(saved?.name ?? hit.name);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hit.id, startSaving]);
  const commit = () => {
    onSave(name.trim() || hit.name);
    setNaming(false);
  };
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
        <button
          className={`btn ${saved ? "saved-btn" : ""}`}
          onClick={() => setNaming(true)}
          title={saved ? "In your address book" : "Save to your address book"}
        >
          <StarIcon filled={!!saved} /> {saved ? "Saved" : "Save"}
        </button>
        <button className="btn" onClick={() => navigator.clipboard?.writeText(coords)}>
          Copy coordinates
        </button>
      </div>
      {naming && (
        <div className="save-box">
          <label className="field">
            <span>{saved ? "Name in your address book" : "Save as"}</span>
            <input
              autoFocus
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") commit();
                if (e.key === "Escape") setNaming(false);
              }}
              onFocus={(e) => e.target.select()}
            />
          </label>
          <div className="save-actions">
            <button className="btn primary" onClick={commit}>{saved ? "Rename" : "Save"}</button>
            <button className="btn" onClick={() => setNaming(false)}>Cancel</button>
            {saved && (
              <button
                className="btn danger-text"
                onClick={() => {
                  onUnsave(saved.id);
                  setNaming(false);
                }}
              >
                Remove
              </button>
            )}
          </div>
        </div>
      )}
      {saved && !naming && saved.name !== hit.name && <div className="muted small">Saved as “{saved.name}”</div>}
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
