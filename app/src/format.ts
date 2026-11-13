export function formatMiles(mi: number): string {
  if (mi < 0.1) return `${Math.max(50, Math.round((mi * 5280) / 50) * 50)} ft`;
  if (mi < 10) return `${mi.toFixed(1)} mi`;
  return `${Math.round(mi).toLocaleString()} mi`;
}

export function formatKm(km: number): string {
  return formatMiles(km * 0.621371);
}

export function formatDuration(s: number): string {
  const min = Math.round(s / 60);
  if (min < 1) return "< 1 min";
  if (min < 60) return `${min} min`;
  const h = Math.floor(min / 60);
  const m = min % 60;
  return m ? `${h} hr ${m} min` : `${h} hr`;
}

export function formatBytes(n: number): string {
  if (n < 1e6) return `${Math.max(1, Math.round(n / 1e3))} KB`;
  if (n < 1e9) return `${Math.round(n / 1e6)} MB`;
  return `${(n / 1e9).toFixed(1)} GB`;
}

const KIND_LABELS: Record<string, string> = {
  place: "Place",
  address: "Address",
  street: "Street",
  city: "City",
  town: "Town",
  village: "Village",
  county: "County",
  state: "State",
  zip: "ZIP code",
  neighborhood: "Neighborhood",
  coordinates: "Coordinates",
};

export function kindLabel(kind: string): string {
  return KIND_LABELS[kind] ?? kind;
}

export function prettyCategory(c: string): string {
  if (!c) return "";
  const s = c.replace(/_/g, " ");
  return s.charAt(0).toUpperCase() + s.slice(1);
}

/** Zoom level that frames a search result of this kind. */
export function zoomFor(kind: string): number {
  switch (kind) {
    case "state":
      return 6.5;
    case "county":
      return 9.5;
    case "city":
      return 11.5;
    case "town":
    case "village":
    case "zip":
      return 12.5;
    case "neighborhood":
    case "street":
      return 14.5;
    default:
      return 16.5;
  }
}
