import { ask, open } from "@tauri-apps/plugin-dialog";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type Catalog,
  type CatalogRegion,
  type CheckProgress,
  type ImportReport,
  type Progress,
  type QueueStatus,
  type RegionCheck,
  type Status,
  type StorageInfo,
} from "../api";
import { formatBytes, formatDuration } from "../format";
import { CheckIcon, DownloadIcon, FolderIcon } from "./Icons";

const RELEASES = "https://github.com/HandyFox/reflectmaps/releases";

const PARTS: [string, string][] = [
  ["map", "Map"],
  ["search", "Search"],
  ["routing", "Directions"],
];

/** Installed files take about this much more room than the downloads. */
const INSTALLED_FACTOR = 1.9;

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
  /** Address history / address book sizes, and wiping them. */
  personal: { history: number; places: number };
  onClearPersonal: () => Promise<void>;
};

type Row = {
  r: CatalogRegion;
  parts: [string, string][];
  todo: string[];
  missing: string[];
  need: number;
  installed: boolean;
};

const LIVE = (p: Progress) => !["done", "error", "cancelled"].includes(p.stage);

export function Downloads({
  status,
  progress,
  onChanged,
  notify,
  onImport,
  importing,
  importReport,
  onDismissReport,
  catalogStamp,
  personal,
  onClearPersonal,
}: Props) {
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [online, setOnline] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [queue, setQueue] = useState<QueueStatus | null>(null);
  const [storage, setStorage] = useState<StorageInfo | null>(null);
  const [checking, setChecking] = useState<CheckProgress | null>(null);
  const [checkResult, setCheckResult] = useState<RegionCheck[] | null>(null);
  const [doneSeen, setDoneSeen] = useState(0);
  const samples = useRef<{ t: number; b: number }[]>([]);

  const load = async (refresh: boolean) => {
    setLoading(true);
    const res = await api.catalog(refresh).catch((e) => ({ catalog: null, online: false, error: String(e) }));
    if (res.catalog) setCatalog(res.catalog);
    setOnline(res.online);
    setError(res.error ?? null);
    setLoading(false);
  };

  const loadStorage = () => api.storage().then(setStorage).catch(() => {});

  useEffect(() => {
    load(false).then(() => load(true));
    api.queue().then(setQueue).catch(() => {});
    loadStorage();
    const uns = [
      api.onQueue(setQueue),
      api.onPacksChanged(() => {
        onChanged();
        loadStorage();
      }),
      api.onCheckProgress(setChecking),
    ];
    return () => {
      uns.forEach((u) => u.then((f) => f()));
    };
  }, []);

  useEffect(() => {
    if (catalogStamp > 0) load(false);
  }, [catalogStamp]);

  const installed = status?.installed.regions ?? {};
  const queued = new Set(queue?.items ?? []);
  const active = new Set(queue?.active ?? []);
  const failed = queue?.failed ?? {};

  // Every state with what it still needs.
  const rows: Row[] = useMemo(() => {
    const all: CatalogRegion[] = [...(catalog?.regions ?? [])];
    // Installed states stay listed even without a catalog.
    for (const [id, r] of Object.entries(installed)) {
      if (!all.find((x) => x.id === id)) all.push({ id, name: r.name, bbox: r.bbox ?? undefined, components: {} });
    }
    return all.map((r) => {
      const inst = installed[r.id];
      const parts = PARTS.filter(([k]) => r.components[k] || inst?.components[k]);
      const missing = PARTS.filter(([k]) => r.components[k] && !inst?.components[k]).map(([k]) => k);
      const outdated = PARTS.filter(
        ([k]) => r.components[k] && inst?.components[k] && inst.components[k].sha256 !== r.components[k].sha256,
      ).map(([k]) => k);
      const todo = [...missing, ...outdated];
      const need = todo.reduce((s, k) => s + (r.components[k]?.size ?? 0), 0);
      return { r, parts, todo, missing, need, installed: !!inst };
    });
  }, [catalog, installed]);

  const needle = filter.trim().toLowerCase();
  const visible = rows
    .filter((x) => !needle || x.r.name.toLowerCase().includes(needle) || x.r.id === needle)
    .sort(
      (a, b) =>
        Number(!(a.installed || queued.has(a.r.id))) - Number(!(b.installed || queued.has(b.r.id))) ||
        a.r.name.localeCompare(b.r.name),
    );

  const notInstalled = rows.filter((x) => x.r.components.map && !x.installed);
  // States missing something (not installed, or only partly).
  const allNeed = rows.filter((x) => x.missing.length > 0);
  // Fully installed states with a newer version of some part.
  const updates = rows.filter((x) => x.installed && x.missing.length === 0 && x.todo.length > 0);
  const sel = rows.filter((x) => selected.has(x.r.id));
  const selNeed = sel.filter((x) => x.todo.length > 0);
  const selBytes = selNeed.reduce((s, x) => s + x.need, 0);
  const selInstalled = sel.filter((x) => x.installed);
  const allVisibleSelected = visible.length > 0 && visible.every((x) => selected.has(x.r.id));

  const spaceNote = (bytes: number) => {
    if (!storage?.free) return null;
    const want = bytes * INSTALLED_FACTOR;
    return want > storage.free
      ? `Needs about ${formatBytes(want)} of space once installed, but the drive has ${formatBytes(storage.free)} free.`
      : null;
  };

  const enqueue = async (ids: string[], what: string) => {
    try {
      const n = await api.queueAdd(ids);
      notify(n ? `${what}: ${n} state${n === 1 ? "" : "s"} queued` : "Nothing to download: already up to date");
      setSelected(new Set());
    } catch (e) {
      notify(String(e));
    }
  };

  const downloadAll = async () => {
    const bytes = allNeed.reduce((s, x) => s + x.need, 0);
    const warn = spaceNote(bytes);
    const ok = await ask(
      `Download ${allNeed.length} state${allNeed.length === 1 ? "" : "s"} (${formatBytes(bytes)})?` +
        ` Installed, they take about ${formatBytes(bytes * INSTALLED_FACTOR)}.` +
        (warn ? `\n\n${warn}` : "") +
        "\n\nYou can keep using the app, pause at any time, and close it: downloads continue where they stopped.",
      { title: "Download all states", kind: warn ? "warning" : "info", okLabel: "Download", cancelLabel: "Cancel" },
    );
    if (ok) enqueue(allNeed.map((x) => x.r.id), "Download all");
  };

  const removeSelected = async () => {
    const names = selInstalled.map((x) => x.r.name);
    const ok = await ask(`Remove ${names.length} state${names.length === 1 ? "" : "s"} from this computer?\n\n${names.join(", ")}`, {
      title: "Remove states",
      kind: "warning",
      okLabel: "Remove",
      cancelLabel: "Cancel",
    });
    if (!ok) return;
    await api.queueRemove(selInstalled.map((x) => x.r.id)).catch(() => {});
    for (const x of selInstalled) await api.remove(x.r.id).catch((e) => notify(String(e)));
    setSelected(new Set());
    notify(`Removed ${names.length} state${names.length === 1 ? "" : "s"}`);
    onChanged();
    loadStorage();
  };

  const remove = async (id: string, name: string) => {
    await api.queueRemove([id]).catch(() => {});
    await api.remove(id).catch((e) => notify(String(e)));
    notify(`${name} removed`);
    onChanged();
    loadStorage();
  };

  const clearPersonal = async () => {
    const ok = await ask(
      `Delete your address history (${personal.history}) and address book (${personal.places} saved place${personal.places === 1 ? "" : "s"}) from ${storage?.portable ? "this drive" : "this computer"}?\n\nDo this before copying the drive for someone else. Export the address book first if you want to keep it.`,
      { title: "Clear personal data", kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" },
    );
    if (!ok) return;
    try {
      await onClearPersonal();
      notify("Address history and address book deleted");
    } catch (e) {
      notify(String(e));
    }
  };

  const runCheck = async () => {
    setCheckResult(null);
    setChecking({ done: 0, total: 0, region: "" });
    try {
      setCheckResult(await api.checkData());
    } catch (e) {
      if (!String(e).includes("cancelled")) notify(String(e));
    } finally {
      setChecking(null);
    }
  };

  const toggle = (id: string) =>
    setSelected((cur) => {
      const n = new Set(cur);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });
  const toggleAll = () =>
    setSelected((cur) => {
      const n = new Set(cur);
      if (allVisibleSelected) visible.forEach((x) => n.delete(x.r.id));
      else visible.forEach((x) => n.add(x.r.id));
      return n;
    });

  // ---- overall progress of the queue ----
  const liveBytes = useMemo(() => {
    let b = 0;
    for (const id of queue?.active ?? []) {
      const row = rows.find((x) => x.r.id === id);
      for (const k of row?.todo ?? []) {
        const p = progress[`${id}:${k}`];
        if (!p) continue;
        b += p.stage === "downloading" ? p.received : LIVE(p) || p.stage === "done" ? p.total : 0;
      }
    }
    return b;
  }, [queue, progress, rows]);
  const qTotal = queue?.batch_total ?? 0;
  const qDone = Math.min(qTotal, (queue?.batch_done ?? 0) + liveBytes);
  const running = !!queue && queue.items.length > 0 && !queue.paused;
  useEffect(() => {
    if (!running) {
      samples.current = [];
      return;
    }
    const now = Date.now();
    samples.current = [...samples.current.filter((s) => now - s.t < 20000), { t: now, b: qDone }];
  }, [qDone, running]);
  const rate = (() => {
    const s = samples.current;
    if (s.length < 2) return 0;
    const dt = (s[s.length - 1].t - s[0].t) / 1000;
    return dt > 3 ? Math.max(0, (s[s.length - 1].b - s[0].b) / dt) : 0;
  })();
  const eta = rate > 0 ? (qTotal - qDone) / rate : null;

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
  const installedBytes = Object.values(installed).reduce(
    (s, r) => s + Object.values(r.components).reduce((t, c) => t + c.size, 0),
    0,
  );
  const failedIds = Object.keys(failed);
  const nameOf = (id: string) => rows.find((x) => x.r.id === id)?.r.name ?? id.toUpperCase();

  return (
    <div className="downloads">
      <p className="muted small pad-x">
        Download a state once to browse, search and get directions there with no internet connection.
      </p>
      {storage?.portable && (
        <div className="note small">
          <strong>Portable copy.</strong> Everything is stored on this drive, next to the program. Copy the whole
          ReflectMaps folder to another drive to make another copy.
        </div>
      )}
      {storage?.warning && <div className="note error small">{storage.warning}</div>}
      {error && (
        <div className="note warn">
          {catalog ? "Offline. Showing the last downloaded list." : error}
        </div>
      )}
      {loading && !catalog && <div className="muted pad">Loading the list of states…</div>}

      {/* ---- bulk actions ---- */}
      {catalog && online && (allNeed.length > 0 || updates.length > 0) && (
        <div className="bulk-actions">
          {notInstalled.length > 0 && (
            <button className="btn primary" onClick={downloadAll}>
              <DownloadIcon size={16} />{" "}
              {notInstalled.length === rows.length ? `Download all ${rows.length} states` : `Download all remaining (${allNeed.length})`}
              {" · "}
              {formatBytes(allNeed.reduce((s, x) => s + x.need, 0))}
            </button>
          )}
          {updates.length > 0 && (
            <button className="btn" onClick={() => enqueue(updates.map((x) => x.r.id), "Update all")}>
              Update all ({updates.length}) · {formatBytes(updates.reduce((s, x) => s + x.need, 0))}
            </button>
          )}
        </div>
      )}

      {queue && queue.items.length === 0 && queue.batch_states_done > 0 && queue.batch_states_done !== doneSeen && (
        <div className="note small">
          <strong>
            Finished: {queue.batch_states_done} state{queue.batch_states_done === 1 ? "" : "s"} downloaded (
            {formatBytes(queue.batch_done)}).
          </strong>{" "}
          <button className="link-btn small" onClick={() => setDoneSeen(queue.batch_states_done)}>Dismiss</button>
        </div>
      )}

      {/* ---- the queue ---- */}
      {queue && (queue.items.length > 0 || failedIds.length > 0 || queue.note) && (
        <section className="queue-box">
          {queue.items.length > 0 && (
            <>
              <div className="queue-head">
                <strong>
                  {queue.paused ? "Paused" : "Downloading"} · {queue.batch_states_done} of {queue.batch_states} states
                </strong>
                {queue.paused ? (
                  <button className="btn primary" onClick={() => api.queueResume()}>Resume</button>
                ) : (
                  <button className="btn" onClick={() => api.queuePause()}>Pause all</button>
                )}
              </div>
              <div className="bar big">
                <div style={{ width: `${qTotal ? Math.round((qDone / qTotal) * 100) : 0}%` }} />
              </div>
              <div className="muted small">
                {formatBytes(qDone)} of {formatBytes(qTotal)}
                {running && rate > 0 ? ` · ${formatBytes(rate)}/s` : ""}
                {running && eta !== null ? (eta < 60 ? " · less than a minute left" : ` · about ${formatDuration(eta)} left`) : ""}
              </div>
              {queue.active.length > 0 && !queue.paused && (
                <div className="muted small">Now: {queue.active.map(nameOf).join(", ")}</div>
              )}
              {queue.items.length > queue.active.length && (
                <div className="muted small">
                  Waiting: {queue.items.filter((i) => !active.has(i)).length} state
                  {queue.items.length - queue.active.length === 1 ? "" : "s"}
                  {" · "}
                  <button className="link-btn small" onClick={() => api.queueRemove(queue.items.filter((i) => !active.has(i)))}>
                    cancel waiting
                  </button>
                </div>
              )}
            </>
          )}
          {queue.note && <div className="note warn small">{queue.note}</div>}
          {failedIds.length > 0 && (
            <div className="note error small">
              <strong>Couldn’t download:</strong>
              <ul>
                {failedIds.map((id) => (
                  <li key={id}>
                    {nameOf(id)}: {failed[id]}
                  </li>
                ))}
              </ul>
              <button className="link-btn small" onClick={() => enqueue(failedIds, "Retry")}>Try again</button>{" "}
              <button className="link-btn small" onClick={() => api.queueClearFailed()}>Dismiss</button>
            </div>
          )}
        </section>
      )}

      <details className="import-box" open={!catalog || importing || !!importReport}>
        <summary className="import-head">
          <strong>Import from a USB drive or folder</strong>
          <span className="muted small">or drag pack files onto this window</span>
        </summary>
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
      </details>

      {/* ---- list of states ---- */}
      {rows.length > 0 && (
        <div className="select-bar">
          <label className="select-all">
            <input type="checkbox" checked={allVisibleSelected} onChange={toggleAll} />
            {needle ? "Select shown" : "Select all"}
          </label>
          {selected.size > 0 ? (
            <div className="select-actions">
              <span className="muted small">{selected.size} selected</span>
              {online && selNeed.length > 0 && (
                <button className="btn primary small-btn" onClick={() => enqueue(selNeed.map((x) => x.r.id), "Download")}>
                  Download · {formatBytes(selBytes)}
                </button>
              )}
              {selInstalled.length > 0 && (
                <button className="btn danger-text small-btn" onClick={removeSelected}>Remove</button>
              )}
              <button className="link-btn small" onClick={() => setSelected(new Set())}>Clear</button>
            </div>
          ) : (
            <span className="muted small">
              {Object.keys(installed).length} installed
              {installedBytes > 0 ? ` · ${formatBytes(installedBytes)} downloaded` : ""}
            </span>
          )}
        </div>
      )}
      {selected.size > 0 && selBytes > 0 && spaceNote(selBytes) && <div className="note warn small">{spaceNote(selBytes)}</div>}
      {rows.length > 6 && (
        <input
          className="region-filter"
          type="search"
          placeholder={`Find a state (${rows.length} available)`}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
      )}
      {!loading && rows.length === 0 && (
        <div className="note">
          No download list yet. Connect to the internet once to see available states, or import packs copied from
          another computer.
        </div>
      )}
      {needle && visible.length === 0 && <div className="muted pad">No state matches “{filter}”.</div>}
      <ul className="region-list">
        {visible.map(({ r, parts, todo, missing, need }) => {
          const inst = installed[r.id];
          const isActive = active.has(r.id);
          const isQueued = queued.has(r.id);
          const live = Object.values(progress).filter((p) => p.region === r.id && LIVE(p));
          const prog = live[0];
          const pct = prog && prog.total ? Math.round((prog.received / prog.total) * 100) : 0;
          const lastError = Object.values(progress).filter((p) => p.region === r.id && p.stage === "error").slice(-1)[0];
          return (
            <li key={r.id} className={`region${selected.has(r.id) ? " selected" : ""}`}>
              <div className="region-head">
                <input
                  type="checkbox"
                  className="region-check"
                  checked={selected.has(r.id)}
                  onChange={() => toggle(r.id)}
                  aria-label={`Select ${r.name}`}
                />
                <div className="region-main">
                  <div className="region-name">{r.name}</div>
                  <div className="region-parts">
                    {parts.map(([k, label]) => (
                      <span key={k} className={`part ${inst?.components[k] ? "have" : ""}`}>
                        {inst?.components[k] ? <CheckIcon /> : null}
                        {label}
                        {r.components[k] ? ` ${formatBytes(r.components[k].size)}` : ""}
                      </span>
                    ))}
                  </div>
                </div>
                <div className="region-actions">
                  {isActive || isQueued ? (
                    <>
                      {!isActive && <span className="muted small">Queued</span>}
                      <button className="btn" onClick={() => api.queueRemove([r.id])}>{isActive ? "Stop" : "Cancel"}</button>
                    </>
                  ) : todo.length > 0 && online ? (
                    <button className="btn primary" onClick={() => enqueue([r.id], r.name)}>
                      <DownloadIcon size={16} /> {inst ? (missing.length ? "Download rest" : "Update") : "Download"} ·{" "}
                      {formatBytes(need)}
                    </button>
                  ) : null}
                  {inst && !isActive && !isQueued && (
                    <button className="btn danger-text" onClick={() => remove(r.id, r.name)}>Remove</button>
                  )}
                </div>
              </div>
              {isActive && prog && (
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
              {!isActive && !isQueued && lastError && !failed[r.id] && (
                <div className="note error small">{lastError.message}</div>
              )}
            </li>
          );
        })}
      </ul>

      {/* ---- drive and checking ---- */}
      <section className="drive-box">
        {storage && (
          <div className="muted small">
            Data folder: <code>{storage.data_dir}</code>
            {storage.free != null && (
              <>
                {" · "}
                {formatBytes(storage.free)} free{storage.total ? ` of ${formatBytes(storage.total)}` : ""}
              </>
            )}
            {storage.file_system ? ` · ${storage.file_system}` : ""}
          </div>
        )}
        <div className="drive-actions">
          {Object.keys(installed).length > 0 &&
            (checking ? (
              <button className="btn" onClick={() => api.cancelCheck()}>Stop checking</button>
            ) : (
              <button className="btn" onClick={runCheck} title="Read every installed file and compare it with its checksum">
                Check data
              </button>
            ))}
          {online && (
            <button className="btn" onClick={() => load(true)} disabled={loading}>Check for updates</button>
          )}
          {(personal.history > 0 || personal.places > 0) && (
            <button className="btn" onClick={clearPersonal} title="Delete address history and the address book">
              Clear personal data
            </button>
          )}
          {status && (
            <button className="link-btn small" onClick={() => openPath(status.data_dir).catch(() => {})}>
              <FolderIcon /> Open data folder
            </button>
          )}
        </div>
        {checking && (
          <div className="progress">
            <div className="bar">
              <div style={{ width: `${checking.total ? Math.round((checking.done / checking.total) * 100) : 0}%` }} />
            </div>
            <div className="muted small">
              Checking{checking.region ? ` ${nameOf(checking.region)}` : ""}… {formatBytes(checking.done)} of{" "}
              {formatBytes(checking.total)}
            </div>
          </div>
        )}
        {checkResult && <CheckReport result={checkResult} online={online} onRepair={(ids) => enqueueRepair(ids)} onDismiss={() => setCheckResult(null)} />}
        {catalog && (
          <p className="muted small">
            Data build {catalog.tag} · Overture {catalog.sources?.overture_release} · map {catalog.sources?.protomaps_build}
          </p>
        )}
      </section>
    </div>
  );

  async function enqueueRepair(ids: string[]) {
    // Damaged states are removed and downloaded again.
    for (const id of ids) await api.remove(id).catch(() => {});
    onChanged();
    await enqueue(ids, "Repair");
    setCheckResult(null);
  }
}

function CheckReport({
  result,
  online,
  onRepair,
  onDismiss,
}: {
  result: RegionCheck[];
  online: boolean;
  onRepair: (ids: string[]) => void;
  onDismiss: () => void;
}) {
  const bad = result.filter((r) => r.status === "damaged");
  const unchecked = result.filter((r) => r.status === "unchecked");
  const ok = result.filter((r) => r.status === "ok");
  return (
    <div className={`note small ${bad.length ? "error" : ""}`}>
      {bad.length === 0 ? (
        <strong>
          All {ok.length} checked state{ok.length === 1 ? "" : "s"} are intact.
        </strong>
      ) : (
        <>
          <strong>
            {bad.length} state{bad.length === 1 ? " has" : "s have"} damaged or missing files:
          </strong>
          <ul>
            {bad.map((r) => (
              <li key={r.region}>
                {r.name}: {r.problems.join("; ")}
              </li>
            ))}
          </ul>
          {online ? (
            <button className="link-btn small" onClick={() => onRepair(bad.map((r) => r.region))}>
              Download them again
            </button>
          ) : (
            <span>Copy those states again from the original drive, or connect to the internet and download them again.</span>
          )}{" "}
        </>
      )}
      {unchecked.length > 0 && (
        <div className="muted">
          {unchecked.length} state{unchecked.length === 1 ? "" : "s"} installed by an older version could only be
          checked for missing files ({unchecked.map((r) => r.name).join(", ")}).
        </div>
      )}
      <button className="link-btn small" onClick={onDismiss}>Dismiss</button>
    </div>
  );
}
