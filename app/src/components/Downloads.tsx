import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import { api, type Catalog, type CatalogRegion, type Progress, type Status } from "../api";
import { formatBytes } from "../format";
import { CheckIcon, DownloadIcon, FolderIcon } from "./Icons";

const PARTS: [string, string][] = [
  ["map", "Map"],
  ["search", "Search"],
  ["routing", "Directions"],
];

type Props = {
  status: Status | null;
  progress: Record<string, Progress>;
  onChanged: (zoomTo?: CatalogRegion) => void;
  notify: (msg: string) => void;
};

export function Downloads({ status, progress, onChanged, notify }: Props) {
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [online, setOnline] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const load = async (refresh: boolean) => {
    setLoading(true);
    const res = await api.catalog(refresh).catch((e) => ({ catalog: null, online: false, error: String(e) }));
    if (res.catalog) setCatalog(res.catalog);
    setOnline(res.online);
    setError(res.error ?? null);
    setLoading(false);
  };

  useEffect(() => {
    load(false).then(() => load(true));
  }, []);

  const installed = status?.installed.regions ?? {};

  const install = async (r: CatalogRegion, parts: string[]) => {
    try {
      await api.install(r.id, parts);
      notify(`${r.name} is ready to use offline`);
      onChanged(r);
    } catch (e) {
      const msg = String(e);
      if (!msg.includes("cancelled")) notify(`${r.name}: ${msg}`);
      onChanged();
    }
  };

  const remove = async (id: string, name: string) => {
    await api.remove(id).catch((e) => notify(String(e)));
    notify(`${name} removed`);
    onChanged();
  };

  const importFile = async () => {
    const path = await open({
      multiple: false,
      filters: [{ name: "ReflectMaps packs", extensions: ["pmtiles", "gz"] }],
    });
    if (!path) return;
    try {
      const what = await api.importPack(path as string);
      notify(`Imported ${what}`);
      onChanged();
    } catch (e) {
      notify(String(e));
    }
  };

  const regions: CatalogRegion[] = catalog?.regions ?? [];
  // Show installed regions even when the catalog is unavailable.
  for (const [id, r] of Object.entries(installed)) {
    if (!regions.find((x) => x.id === id)) regions.push({ id, name: r.name, bbox: r.bbox ?? undefined, components: {} });
  }

  return (
    <div className="downloads">
      <p className="muted small pad-x">
        Download a state once to browse, search and get directions there with no internet connection.
      </p>
      {error && <div className="note warn">{online ? error : "Offline. Showing the last downloaded list."}</div>}
      {loading && !catalog && <div className="muted pad">Loading the list of states…</div>}
      {!loading && regions.length === 0 && <div className="note">No download list yet. Connect to the internet once to see available states.</div>}

      <ul className="region-list">
        {regions.map((r) => {
          const inst = installed[r.id];
          const parts = PARTS.filter(([k]) => r.components[k]);
          const missing = parts.filter(([k]) => !inst?.components[k]).map(([k]) => k);
          const outdated = parts
            .filter(([k]) => inst?.components[k] && catalog && inst.components[k].tag !== catalog.tag && inst.components[k].sha256 !== r.components[k].sha256)
            .map(([k]) => k);
          const todo = [...missing, ...outdated];
          const size = todo.reduce((s, k) => s + (r.components[k]?.size ?? 0), 0);
          const active = Object.values(progress).filter((p) => p.region === r.id && !["done", "error", "cancelled"].includes(p.stage));
          const busy = active.length > 0;
          const prog = active[0];
          const pct = prog && prog.total ? Math.round((prog.received / prog.total) * 100) : 0;
          return (
            <li key={r.id} className="region">
              <div className="region-head">
                <div>
                  <div className="region-name">{r.name}</div>
                  <div className="region-parts">
                    {(parts.length ? parts : PARTS.filter(([k]) => inst?.components[k])).map(([k, label]) => (
                      <span key={k} className={`part ${inst?.components[k] ? "have" : ""}`}>
                        {inst?.components[k] ? <CheckIcon /> : null}
                        {label}
                        {r.components[k] ? ` ${formatBytes(r.components[k].size)}` : ""}
                      </span>
                    ))}
                  </div>
                </div>
                <div className="region-actions">
                  {busy ? (
                    <button className="btn" onClick={() => api.cancel(r.id)}>Pause</button>
                  ) : todo.length > 0 && online ? (
                    <button className="btn primary" onClick={() => install(r, todo)}>
                      <DownloadIcon size={16} /> {inst ? (missing.length ? "Download rest" : "Update") : "Download"} · {formatBytes(size)}
                    </button>
                  ) : null}
                  {inst && !busy && (
                    <button className="btn danger-text" onClick={() => remove(r.id, r.name)}>Remove</button>
                  )}
                </div>
              </div>
              {busy && prog && (
                <div className="progress">
                  <div className="bar"><div style={{ width: `${prog.stage === "downloading" ? pct : 100}%` }} /></div>
                  <div className="muted small">
                    {PARTS.find(([k]) => k === prog.component)?.[1]}:{" "}
                    {prog.stage === "downloading"
                      ? `${formatBytes(prog.received)} of ${formatBytes(prog.total)}`
                      : prog.stage === "verifying"
                        ? "checking file"
                        : "installing"}
                  </div>
                </div>
              )}
              {Object.values(progress)
                .filter((p) => p.region === r.id && p.stage === "error")
                .slice(-1)
                .map((p) => (
                  <div key="err" className="note error small">{p.message}</div>
                ))}
            </li>
          );
        })}
      </ul>

      <div className="downloads-foot">
        <button className="btn" onClick={importFile}>Import pack from file…</button>
        {online && (
          <button className="btn" onClick={() => load(true)} disabled={loading}>Check for updates</button>
        )}
        {status && (
          <button className="link-btn small" onClick={() => openPath(status.data_dir).catch(() => {})}>
            <FolderIcon /> Open data folder
          </button>
        )}
        {catalog && (
          <p className="muted small">
            Data build {catalog.tag} · Overture {catalog.sources?.overture_release} · map {catalog.sources?.protomaps_build}
          </p>
        )}
      </div>
    </div>
  );
}
