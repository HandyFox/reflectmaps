import { open } from "@tauri-apps/plugin-dialog";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import { api, type Catalog, type CatalogRegion, type ImportReport, type Progress, type Status } from "../api";
import { formatBytes } from "../format";
import { CheckIcon, DownloadIcon, FolderIcon } from "./Icons";

const RELEASES = "https://github.com/HandyFox/reflectmaps/releases";

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
  onImport: (paths: string[]) => void;
  importing: boolean;
  importReport: ImportReport | null;
  onDismissReport: () => void;
  /** Changes when an import brought a new catalog.json. */
  catalogStamp: number;
};

export function Downloads({ status, progress, onChanged, notify, onImport, importing, importReport, onDismissReport, catalogStamp }: Props) {
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [online, setOnline] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [filter, setFilter] = useState("");

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

  useEffect(() => {
    if (catalogStamp > 0) load(false);
  }, [catalogStamp]);

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

  const pickFiles = async () => {
    const picked = await open({
      multiple: true,
      title: "Choose map pack files",
      filters: [
        {
          name: "ReflectMaps packs",
          extensions: ["pmtiles", "gz", "json", ...Array.from({ length: 10 }, (_, i) => `part0${i}`)],
        },
        { name: "All files", extensions: ["*"] },
      ],
    });
    if (picked) onImport(Array.isArray(picked) ? picked : [picked]);
  };

  const pickFolder = async () => {
    const picked = await open({ directory: true, multiple: false, title: "Choose the folder with your map packs" });
    if (picked) onImport([picked as string]);
  };

  const releaseUrl = catalog ? `${RELEASES}/tag/${catalog.tag}` : RELEASES;

  const all: CatalogRegion[] = [...(catalog?.regions ?? [])];
  // Show installed regions even when the catalog is unavailable.
  for (const [id, r] of Object.entries(installed)) {
    if (!all.find((x) => x.id === id)) all.push({ id, name: r.name, bbox: r.bbox ?? undefined, components: {} });
  }
  // Installed and downloading states first, then A to Z; filter by name.
  const busy = new Set(Object.values(progress).filter((p) => !["done", "error", "cancelled"].includes(p.stage)).map((p) => p.region));
  const rank = (r: CatalogRegion) => (installed[r.id] || busy.has(r.id) ? 0 : 1);
  const needle = filter.trim().toLowerCase();
  const regions = all
    .filter((r) => !needle || r.name.toLowerCase().includes(needle) || r.id === needle)
    .sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));

  return (
    <div className="downloads">
      <p className="muted small pad-x">
        Download a state once to browse, search and get directions there with no internet connection.
      </p>
      {error && (
        <div className="note warn">
          {catalog ? "Offline. Showing the last downloaded list." : error}
        </div>
      )}
      {loading && !catalog && <div className="muted pad">Loading the list of states…</div>}
      {!loading && regions.length === 0 && (
        <div className="note">
          No download list yet. Connect to the internet once to see available states, or import packs copied from
          another computer.
        </div>
      )}

      <section className="import-box">
        <div className="import-head">
          <strong>Import from a USB drive or folder</strong>
          <span className="muted small">or drag pack files onto this window</span>
        </div>
        <div className="import-actions">
          <button className="btn" onClick={pickFolder} disabled={importing}>Import folder…</button>
          <button className="btn" onClick={pickFiles} disabled={importing}>Import files…</button>
        </div>
        {importing && <div className="muted small">Importing… large states can take a few minutes.</div>}
        {importReport && (
          <div className="import-report">
            {importReport.imported.length > 0 && (
              <div className="note small">
                <strong>Imported:</strong> {importReport.imported.join(", ")}
              </div>
            )}
            {importReport.already.length > 0 && (
              <div className="muted small">Already installed: {importReport.already.join(", ")}</div>
            )}
            {importReport.catalog_updated && importReport.imported.length === 0 && (
              <div className="note small">Updated the list of states from catalog.json.</div>
            )}
            {importReport.warnings.map((w, i) => (
              <div key={`w${i}`} className="note warn small">{w}</div>
            ))}
            {importReport.skipped.length > 0 && (
              <div className="note error small">
                <strong>Not imported:</strong>
                <ul>
                  {importReport.skipped.map((m, i) => (
                    <li key={i}>{m}</li>
                  ))}
                </ul>
              </div>
            )}
            <button className="link-btn small" onClick={onDismissReport}>Dismiss</button>
          </div>
        )}
        <details className="offline-help">
          <summary>How to get packs for a computer without internet</summary>
          <ol>
            <li>
              On a computer with internet, open the map data release on GitHub:{" "}
              {online ? (
                <button className="link-btn url" onClick={() => openUrl(releaseUrl).catch(() => {})}>
                  {releaseUrl.replace("https://", "")}
                </button>
              ) : (
                <code>{releaseUrl.replace("https://", "")}</code>
              )}{" "}
              (the newest “Map data” release).
            </li>
            <li>
              Download <code>catalog.json</code>, then for each state its three files, named by the state’s two-letter
              code: <code>oh-map.pmtiles</code>, <code>oh-search.tar.gz</code> and <code>oh-routing.tar.gz</code>.
              Files too big for GitHub come in parts (<code>.part00</code>, <code>.part01</code>…): get every part.
            </li>
            <li>Copy them all into one folder on a USB drive.</li>
            <li>On this computer, choose <em>Import folder…</em> and pick that folder.</li>
          </ol>
          <p className="muted small">
            catalog.json lets ReflectMaps check every file for damage. Directions work across state lines when all the
            directions packs come from the same release.
          </p>
        </details>
      </section>

      {all.length > 6 && (
        <input
          className="region-filter"
          type="search"
          placeholder={`Find a state (${all.length} available)`}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
      )}
      {needle && regions.length === 0 && <div className="muted pad">No state matches “{filter}”.</div>}
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
