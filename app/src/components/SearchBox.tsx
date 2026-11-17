import { useEffect, useRef, useState } from "react";
import { api, isCancelled, type HistoryItem, type Hit, type SavedPlace, type SearchProgress } from "../api";
import { formatKm, kindLabel } from "../format";
import { samePlace, savedAsHit } from "../userData";
import { ClockIcon, CloseIcon, KindIcon, SearchIcon, StarIcon } from "./Icons";

/** Address history and address book offered by a search box. */
export type SearchExtras = {
  history: HistoryItem[];
  places: SavedPlace[];
  onForget: (h: Hit) => void;
  onClearHistory: () => void;
};

type Item = { hit: Hit; from: "saved" | "recent" | "result" };

const fold = (s: string) => s.toLowerCase().normalize("NFD").replace(/[\u0300-\u036f]/g, "");

type Props = {
  placeholder: string;
  near: () => [number, number];
  /** States to search (null = all), read when each search starts. */
  states?: () => string[] | null;
  /** Identifies this box, so its newer searches cancel its older ones. */
  channel: string;
  onSelect: (hit: Hit) => void;
  /** Enter without picking a suggestion: show all results (main search box). */
  onSubmit?: (query: string) => void;
  value?: string;
  onClear?: () => void;
  autoFocus?: boolean;
  compact?: boolean;
  dot?: string;
  extras?: SearchExtras;
};

let seqCounter = 0;
/** A new search number, shared by all search boxes. */
export const nextSearchSeq = () => ++seqCounter;

/** Thin progress line for searches that take a moment. */
export function SearchProgressLine({ p, label = "Searching" }: { p: SearchProgress | null; label?: string }) {
  if (!p) return null;
  const pct = p.total ? Math.round((p.done / p.total) * 100) : 0;
  return (
    <div className="search-progress" role="progressbar" aria-valuenow={pct}>
      <div className="bar thin">
        <div style={{ width: `${Math.max(pct, 4)}%` }} />
      </div>
      <span className="muted small">
        {label}… {p.total > 1 ? `${p.done} of ${p.total} state${p.total === 1 ? "" : "s"}` : ""}
      </span>
    </div>
  );
}

/**
 * Listens to search progress for one channel; shows it only when a search
 * takes longer than a blink.
 */
export function useSearchProgress(channel: string, seqRef: React.MutableRefObject<number>, busy: boolean) {
  const [p, setP] = useState<SearchProgress | null>(null);
  const [show, setShow] = useState(false);
  useEffect(() => {
    const un = api.onSearchProgress((e) => {
      if (e.channel === channel && e.seq === seqRef.current) setP(e);
    });
    return () => {
      un.then((f) => f());
    };
  }, [channel, seqRef]);
  useEffect(() => {
    if (!busy) {
      setShow(false);
      setP(null);
      return;
    }
    const t = setTimeout(() => setShow(true), 200);
    return () => clearTimeout(t);
  }, [busy]);
  return show ? (p ?? { channel, seq: seqRef.current, done: 0, total: 0 }) : null;
}

/** Search field with autocomplete over the installed search packs. */
export function SearchBox(props: Props) {
  const { placeholder, near, states, channel, onSelect, onSubmit, value, onClear, autoFocus, compact, dot, extras } = props;
  const [query, setQuery] = useState(value ?? "");
  const [hits, setHits] = useState<Hit[]>([]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [navigated, setNavigated] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [everywhere, setEverywhere] = useState(false);
  const [scoped, setScoped] = useState(false);
  const [doneQuery, setDoneQuery] = useState("");
  const seq = useRef(0);
  const input = useRef<HTMLInputElement>(null);
  const progress = useSearchProgress(channel, seq, busy && open);

  useEffect(() => {
    if (value !== undefined) setQuery(value);
  }, [value]);

  // A new query starts from the chosen states again.
  useEffect(() => setEverywhere(false), [query]);

  useEffect(() => {
    const q = query.trim();
    if (!open || !q) {
      setHits([]);
      setBusy(false);
      return;
    }
    const my = ++seqCounter;
    seq.current = my;
    const t = setTimeout(async () => {
      const list = everywhere ? null : (states?.() ?? null);
      setBusy(true);
      try {
        const res = await api.search(query, near(), compact ? 6 : 10, list, channel, my);
        if (my === seq.current) {
          setHits(res);
          setActive(0);
          setNavigated(false);
          setError(null);
          setScoped(list !== null);
          setDoneQuery(query);
        }
      } catch (e) {
        if (my === seq.current && !isCancelled(e)) setError(String(e));
      } finally {
        if (my === seq.current) setBusy(false);
      }
    }, 120);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, open, everywhere]);

  const choose = (h: Hit) => {
    setQuery(h.name);
    setOpen(false);
    onSelect(h);
    input.current?.blur();
  };

  const submit = () => {
    if (!onSubmit || !query.trim()) return;
    seq.current = ++seqCounter; // drop any suggestion search still running
    setBusy(false);
    setOpen(false);
    onSubmit(query);
    input.current?.blur();
  };

  const noMatches = !busy && !error && hits.length === 0 && doneQuery === query && query.trim() !== "";

  // Empty box: saved places and recent ones. Typing: matching saved places first.
  const q = fold(query.trim());
  const items: Item[] = [];
  if (extras) {
    if (!q) {
      extras.places.slice(0, 8).forEach((p) => items.push({ hit: savedAsHit(p), from: "saved" }));
      extras.history
        .filter((x) => !extras.places.some((p) => samePlace(p.hit, x.hit)))
        .slice(0, 8)
        .forEach((x) => items.push({ hit: x.hit, from: "recent" }));
    } else {
      extras.places
        .filter((p) => fold(`${p.name} ${p.hit.name} ${p.hit.label}`).includes(q))
        .slice(0, 3)
        .forEach((p) => items.push({ hit: savedAsHit(p), from: "saved" }));
    }
  }
  if (q) hits.filter((h) => !items.some((it) => samePlace(it.hit, h))).forEach((h) => items.push({ hit: h, from: "result" }));
  const showList = !!q ? items.length > 0 || !!error || noMatches : items.length > 0;

  return (
    <div className={`searchbox ${compact ? "compact" : ""}`}>
      <div className="searchbox-field">
        {dot ? <span className={`wp-dot ${dot}`} /> : <SearchIcon />}
        <input
          ref={input}
          value={query}
          autoFocus={autoFocus}
          placeholder={placeholder}
          spellCheck={false}
          onFocus={() => setOpen(true)}
          onBlur={() => setTimeout(() => setOpen(false), 150)}
          onChange={(e) => {
            setQuery(e.target.value);
            setOpen(true);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setNavigated(true);
              setActive((a) => Math.min(a + 1, items.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setNavigated(true);
              setActive((a) => Math.max(a - 1, 0));
            } else if (e.key === "Enter") {
              if (onSubmit && !navigated && q) submit();
              else if (items[active]) choose(items[active].hit);
            } else if (e.key === "Escape") {
              setOpen(false);
              input.current?.blur();
            }
          }}
        />
        {query && (
          <button
            className="icon-btn subtle"
            title="Clear"
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => {
              setQuery("");
              setHits([]);
              onClear?.();
              input.current?.focus();
            }}
          >
            <CloseIcon />
          </button>
        )}
      </div>
      {open && q && <SearchProgressLine p={progress} />}
      {open && showList && (
        <ul className="dropdown" role="listbox">
          {error && <li className="dropdown-error">{error}</li>}
          {items.map(({ hit: h, from }, i) => (
            <li
              key={`${from}:${h.id}:${i}`}
              role="option"
              aria-selected={i === active}
              className={`${i === active && (navigated || !onSubmit || !q) ? "active" : ""} from-${from}`}
              onMouseDown={(e) => e.preventDefault()}
              onMouseEnter={() => {
                setActive(i);
                setNavigated(true);
              }}
              onClick={() => choose(h)}
            >
              {from === "saved" ? (
                <span className="kind kind-saved"><StarIcon filled /></span>
              ) : from === "recent" ? (
                <span className="kind kind-recent"><ClockIcon /></span>
              ) : (
                <KindIcon kind={h.kind} />
              )}
              <div className="hit-text">
                <div className="hit-name">{h.name}</div>
                <div className="hit-label">
                  {from === "result"
                    ? `${h.kind === "place" && h.category ? `${h.category.replace(/_/g, " ")} · ` : `${kindLabel(h.kind)} · `}${h.label}${h.approximate ? " · approximate" : ""}`
                    : h.label}
                </div>
              </div>
              {from === "recent" && extras ? (
                <button
                  className="icon-btn subtle forget"
                  title="Remove from history"
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={(e) => {
                    e.stopPropagation();
                    extras.onForget(h);
                  }}
                >
                  <CloseIcon size={14} />
                </button>
              ) : (
                from === "result" && h.distance_km !== undefined && <div className="hit-dist">{formatKm(h.distance_km)}</div>
              )}
            </li>
          ))}
          {!q && extras && extras.history.length > 0 && (
            <li className="dropdown-action subtle" onMouseDown={(e) => e.preventDefault()} onClick={extras.onClearHistory}>
              Clear history
            </li>
          )}
          {q && noMatches && <li className="dropdown-note muted small">No matches{scoped ? " in the chosen states" : ""}.</li>}
          {q && scoped && !everywhere && (noMatches || hits.length > 0) && (
            <li
              className="dropdown-action"
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => setEverywhere(true)}
            >
              Search all states for “{query.trim()}”
            </li>
          )}
          {q && onSubmit && hits.length > 0 && (
            <li className="dropdown-action" onMouseDown={(e) => e.preventDefault()} onClick={submit}>
              Show all results on the map <span className="muted small">(Enter)</span>
            </li>
          )}
        </ul>
      )}
    </div>
  );
}
