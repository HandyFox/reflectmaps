type P = { size?: number };

const svg = (size: number, children: React.ReactNode) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}
    strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    {children}
  </svg>
);

export const SearchIcon = ({ size = 18 }: P) => svg(size, <><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></>);
export const RouteIcon = ({ size = 18 }: P) =>
  svg(size, <><circle cx="6" cy="19" r="2.5" /><circle cx="18" cy="5" r="2.5" /><path d="M8.5 19H15a3.5 3.5 0 0 0 0-7H9a3.5 3.5 0 0 1 0-7h6.5" /></>);
export const DownloadIcon = ({ size = 18 }: P) =>
  svg(size, <><path d="M12 4v11" /><path d="m7 10 5 5 5-5" /><path d="M5 20h14" /></>);
export const CloseIcon = ({ size = 16 }: P) => svg(size, <><path d="M6 6l12 12" /><path d="M18 6 6 18" /></>);
export const SwapIcon = ({ size = 18 }: P) =>
  svg(size, <><path d="M7 4v16" /><path d="m3 8 4-4 4 4" /><path d="M17 20V4" /><path d="m21 16-4 4-4-4" /></>);
export const PlusIcon = ({ size = 16 }: P) => svg(size, <><path d="M12 5v14" /><path d="M5 12h14" /></>);
export const SunIcon = ({ size = 18 }: P) =>
  svg(size, <><circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" /></>);
export const MoonIcon = ({ size = 18 }: P) => svg(size, <path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5Z" />);
export const PinIcon = ({ size = 16 }: P) =>
  svg(size, <><path d="M12 21s7-6.2 7-12a7 7 0 0 0-14 0c0 5.8 7 12 7 12Z" /><circle cx="12" cy="9" r="2.5" /></>);
export const PhoneIcon = ({ size = 16 }: P) =>
  svg(size, <path d="M5 4h4l2 5-2.5 1.5a11 11 0 0 0 5 5L15 13l5 2v4a2 2 0 0 1-2 2A16 16 0 0 1 3 6a2 2 0 0 1 2-2" />);
export const GlobeIcon = ({ size = 16 }: P) =>
  svg(size, <><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" /></>);
export const CheckIcon = ({ size = 14 }: P) => svg(size, <path d="m5 12 5 5 9-10" />);
export const FolderIcon = ({ size = 16 }: P) =>
  svg(size, <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />);

export function KindIcon({ kind }: { kind: string }) {
  const letter: Record<string, string> = {
    place: "●", address: "#", street: "═", city: "◆", town: "◆", village: "◆",
    county: "▢", state: "▣", zip: "✉", neighborhood: "◇", coordinates: "⌖",
  };
  return <span className={`kind kind-${kind}`}>{letter[kind] ?? "●"}</span>;
}
