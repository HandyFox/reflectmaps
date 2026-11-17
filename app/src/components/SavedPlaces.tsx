import { open, save } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { api, type Hit, type PlacesImportReport } from "../api";
import { savedAsHit, type UserData } from "../userData";
import { StarIcon } from "./Icons";

type Props = {
  user: UserData;
  onOpen: (h: Hit) => void;
  showStars: boolean;
  setShowStars: (v: boolean) => void;
  notify: (msg: string) => void;
};

/** The address book: saved places, with CSV import and export. */
export function SavedPlaces({ user, onOpen, showStars, setShowStars, notify }: Props) {
  const [filter, setFilter] = useState("");
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<PlacesImportReport | null>(null);
  const needle = filter.trim().toLowerCase();
  const list = [...user.places]
    .sort((a, b) => a.name.localeCompare(b.name))
    .filter((p) => !needle || `${p.name} ${p.hit.name} ${p.hit.label}`.toLowerCase().includes(needle));

  const exportCsv = async () => {
    const path = await save({
      title: "Export address book",
      defaultPath: "ReflectMaps address book.csv",
      filters: [{ name: "CSV (Excel)", extensions: ["csv"] }],
    });
    if (!path) return;
    try {
      const n = await api.placesExport(path);
      notify(`Exported ${n} place${n === 1 ? "" : "s"}`);
    } catch (e) {
      notify(String(e));
    }
  };

  const importCsv = async () => {
    const path = await open({
      title: "Import an address book",
      multiple: false,
      filters: [{ name: "CSV (Excel)", extensions: ["csv", "txt"] }],
    });
    if (!path) return;
    setBusy(true);
    setReport(null);
    try {
      const r = await api.placesImport(path as string);
      await user.reload();
      setReport(r);
    } catch (e) {
      notify(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="saved-section">
      <div className="saved-head">
        <h3>
          <StarIcon filled /> Address book
        </h3>
        <label className="small show-stars">
          <input type="checkbox" checked={showStars} onChange={(e) => setShowStars(e.target.checked)} /> Stars on map
        </label>
      </div>
      {user.places.length === 0 ? (
        <p className="muted small">
          Save places with the <StarIcon /> Save button on a place, or right-click the map and choose “Save this place”.
        </p>
      ) : (
        <>
          {user.places.length > 6 && (
            <input
              className="region-filter"
              type="search"
              placeholder={`Find in ${user.places.length} saved places`}
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
          )}
          <ul className="saved-list">
            {list.map((p) => (
              <li key={p.id} onClick={() => onOpen(savedAsHit(p))}>
                <span className="kind kind-saved">
                  <StarIcon filled />
                </span>
                <div className="hit-text">
                  <div className="hit-name">{p.name}</div>
                  <div className="hit-label">{savedAsHit(p).label}</div>
                </div>
              </li>
            ))}
          </ul>
        </>
      )}
      <div className="saved-tools">
        <button className="link-btn small" onClick={importCsv} disabled={busy}>
          {busy ? "Importing…" : "Import CSV…"}
        </button>
        {user.places.length > 0 && (
          <button className="link-btn small" onClick={exportCsv}>
            Export CSV…
          </button>
        )}
      </div>
      {report && (
        <div className={`note small ${report.skipped.length ? "warn" : ""}`}>
          Added {report.added} place{report.added === 1 ? "" : "s"}
          {report.looked_up ? ` (${report.looked_up} found by looking up their address)` : ""}.
          {report.skipped.length > 0 && (
            <>
              {" "}
              Skipped {report.skipped.length}:
              <ul>
                {report.skipped.slice(0, 10).map((m) => (
                  <li key={m}>{m}</li>
                ))}
                {report.skipped.length > 10 && <li>…and {report.skipped.length - 10} more</li>}
              </ul>
            </>
          )}{" "}
          <button className="link-btn small" onClick={() => setReport(null)}>
            Dismiss
          </button>
        </div>
      )}
    </section>
  );
}
