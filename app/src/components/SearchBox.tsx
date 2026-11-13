import { useEffect, useRef, useState } from "react";
import { api, type Hit } from "../api";
import { formatKm, kindLabel } from "../format";
import { CloseIcon, KindIcon, SearchIcon } from "./Icons";

type Props = {
  placeholder: string;
  near: () => [number, number];
  onSelect: (hit: Hit) => void;
  onResults?: (hits: Hit[], query: string) => void;
  value?: string;
  onClear?: () => void;
  autoFocus?: boolean;
  compact?: boolean;
  dot?: string;
};

/** Search field with autocomplete over the installed search packs. */
export function SearchBox({ placeholder, near, onSelect, onResults, value, onClear, autoFocus, compact, dot }: Props) {
  const [query, setQuery] = useState(value ?? "");
  const [hits, setHits] = useState<Hit[]>([]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (value !== undefined) setQuery(value);
  }, [value]);

  useEffect(() => {
    const q = query.trim();
    if (!open || !q) {
      setHits([]);
      return;
    }
    const my = ++seq.current;
    const t = setTimeout(async () => {
      try {
        const res = await api.search(query, near(), compact ? 6 : 10);
        if (my === seq.current) {
          setHits(res);
          setActive(0);
          setError(null);
          onResults?.(res, query);
        }
      } catch (e) {
        if (my === seq.current) setError(String(e));
      }
    }, 90);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, open]);

  const choose = (h: Hit) => {
    setQuery(compact ? h.name : h.name);
    setOpen(false);
    onSelect(h);
    input.current?.blur();
  };

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
              setActive((a) => Math.min(a + 1, hits.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActive((a) => Math.max(a - 1, 0));
            } else if (e.key === "Enter" && hits[active]) {
              choose(hits[active]);
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
      {open && query.trim() && (hits.length > 0 || error) && (
        <ul className="dropdown" role="listbox">
          {error && <li className="dropdown-error">{error}</li>}
          {hits.map((h, i) => (
            <li
              key={h.id}
              role="option"
              aria-selected={i === active}
              className={i === active ? "active" : ""}
              onMouseDown={(e) => e.preventDefault()}
              onMouseEnter={() => setActive(i)}
              onClick={() => choose(h)}
            >
              <KindIcon kind={h.kind} />
              <div className="hit-text">
                <div className="hit-name">{h.name}</div>
                <div className="hit-label">
                  {h.kind === "place" && h.category ? `${h.category.replace(/_/g, " ")} · ` : `${kindLabel(h.kind)} · `}
                  {h.label}
                  {h.approximate ? " · approximate" : ""}
                </div>
              </div>
              {h.distance_km !== undefined && <div className="hit-dist">{formatKm(h.distance_km)}</div>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
