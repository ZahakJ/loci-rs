/**
 * Loci icon set: inline SVG, drawn on a 24-unit grid, rendered with a constant
 * 1.5px stroke (non-scaling) in currentColor. Shared by every view:
 * append new entries to `paths` rather than creating another icon file.
 */
import type { ReactNode, SVGProps } from 'react';

const dot = (cx: number, cy: number, r = 1.1) => <circle cx={cx} cy={cy} r={r} fill="currentColor" stroke="none" />;

export const paths = {
  home: <><path d="M3.5 10.5 12 3.75l8.5 6.75" /><path d="M5.75 9v11.25h12.5V9" /><path d="M10 20.25v-5.5h4v5.5" /></>,
  cards: <><rect x="3.5" y="7" width="13.5" height="13.25" rx="2" /><path d="M7.5 3.75h10.75a2 2 0 0 1 2 2V16.5" /></>,
  browse: <><rect x="3.5" y="4" width="17" height="16" rx="2" /><path d="M3.5 9h17M9 9v11" /></>,
  stats: <><path d="M3.5 20.25h17" /><path d="M6.5 16.5v-4.5M11 16.5V6M15.5 16.5v-7M20 16.5v-2.5" /></>,
  settings: <><path d="M4 7h9.5M17.5 7H20M4 17h3.5M11.5 17H20" /><circle cx="15.5" cy="7" r="2" /><circle cx="9.5" cy="17" r="2" /></>,
  plus: <path d="M12 5v14M5 12h14" />,
  minus: <path d="M5 12h14" />,
  search: <><circle cx="10.75" cy="10.75" r="6.25" /><path d="m15.5 15.5 5 5" /></>,
  import: <><path d="M12 3.5v11" /><path d="m7.5 10 4.5 4.5 4.5-4.5" /><path d="M4 14.5v4.25c0 .83.67 1.5 1.5 1.5h13c.83 0 1.5-.67 1.5-1.5V14.5" /></>,
  export: <><path d="M12 14.5v-11" /><path d="M7.5 8 12 3.5 16.5 8" /><path d="M4 14.5v4.25c0 .83.67 1.5 1.5 1.5h13c.83 0 1.5-.67 1.5-1.5V14.5" /></>,
  download: <><path d="M12 4v11" /><path d="m7 10.5 5 5 5-5" /><path d="M5 20h14" /></>,
  upload: <><path d="M12 20V9" /><path d="m7 13.5 5-5 5 5" /><path d="M5 4h14" /></>,
  back: <path className="icon-flip" d="M15 5 8 12l7 7" />,
  'chevron-left': <path className="icon-flip" d="m14.5 6-6 6 6 6" />,
  'chevron-right': <path className="icon-flip" d="m9.5 6 6 6-6 6" />,
  'chevron-down': <path d="m6 9.5 6 6 6-6" />,
  'chevron-up': <path d="m6 14.5 6-6 6 6" />,
  more: <>{dot(5.5, 12, 1.4)}{dot(12, 12, 1.4)}{dot(18.5, 12, 1.4)}</>,
  flag: <><path d="M5.5 21V4" /><path d="M5.5 4.5h11.75l-2.5 4.25 2.5 4.25H5.5" /></>,
  suspend: <><circle cx="12" cy="12" r="8.5" /><path d="M10 9v6M14 9v6" /></>,
  pause: <path d="M9 5.5v13M15 5.5v13" />,
  bury: <path d="M19.5 14.25A8 8 0 1 1 9.75 4.5a6.25 6.25 0 0 0 9.75 9.75Z" />,
  moon: <path d="M19.5 14.25A8 8 0 1 1 9.75 4.5a6.25 6.25 0 0 0 9.75 9.75Z" />,
  sun: <><circle cx="12" cy="12" r="3.75" /><path d="M12 2.75v2M12 19.25v2M2.75 12h2M19.25 12h2M5.45 5.45l1.4 1.4M17.15 17.15l1.4 1.4M5.45 18.55l1.4-1.4M17.15 6.85l1.4-1.4" /></>,
  system: <><rect x="3.5" y="4.5" width="17" height="12" rx="2" /><path d="M9 20h6M12 16.5V20" /></>,
  undo: <><path className="icon-flip" d="M9 14 4.5 9.5 9 5" /><path className="icon-flip" d="M4.5 9.5h10a5.25 5.25 0 0 1 0 10.5H11" /></>,
  info: <><circle cx="12" cy="12" r="8.75" /><path d="M12 11v5.25" />{dot(12, 7.9)}</>,
  help: <><circle cx="12" cy="12" r="8.75" /><path d="M9.6 9.4a2.5 2.5 0 0 1 4.85.85c0 1.65-2.45 2.2-2.45 3.75" />{dot(12, 16.9)}</>,
  edit: <><path d="M4.5 19.5h3.75L19 8.75 15.25 5 4.5 15.75v3.75Z" /><path d="m13.25 7 3.75 3.75" /></>,
  trash: <><path d="M4.5 7h15" /><path d="M9.5 7V4.5h5V7" /><path d="m6.5 7 .9 12.4c.06.9.8 1.6 1.7 1.6h5.8c.9 0 1.64-.7 1.7-1.6L17.5 7" /><path d="M10.25 11v6M13.75 11v6" /></>,
  check: <path d="m5 12.5 4.5 4.5L19 7.5" />,
  close: <path d="M6.5 6.5l11 11M17.5 6.5l-11 11" />,
  keyboard: <><rect x="2.75" y="6" width="18.5" height="12" rx="2" />{dot(6.75, 10)}{dot(10.25, 10)}{dot(13.75, 10)}{dot(17.25, 10)}<path d="M8 14.25h8" /></>,
  play: <path d="M7.5 5v14l11.25-7L7.5 5Z" />,
  image: <><rect x="3.5" y="4.5" width="17" height="15" rx="2" /><circle cx="9" cy="9.75" r="1.6" /><path d="m20.5 15.5-4.75-4.75L6.5 19.5" /></>,
  audio: <><path d="M4.5 9.25v5.5h3.75l4.75 4V5.25l-4.75 4H4.5Z" /><path d="M16.25 8.75a4.75 4.75 0 0 1 0 6.5M18.75 6.25a8.25 8.25 0 0 1 0 11.5" /></>,
  cloze: <><path d="M8.5 4.5H5.5v15h3M15.5 4.5h3v15h-3" /><path d="M9.75 12h4.5" /></>,
  occlusion: <><path d="m3.5 3.5 17 17" /><path d="M10.4 5.35A9 9 0 0 1 12 5.25c4.9 0 8.5 4.6 8.5 6.75 0 .95-.7 2.3-1.95 3.6M6.7 6.7C4.65 8.05 3.5 10.3 3.5 12c0 2.15 3.6 6.75 8.5 6.75 1.55 0 2.95-.45 4.2-1.15" /><path d="M9.9 9.9a3 3 0 0 0 4.2 4.2" /></>,
  sparkle: <><path d="M11 3.5 12.7 8.3 17.5 10l-4.8 1.7L11 16.5l-1.7-4.8L4.5 10l4.8-1.7L11 3.5Z" /><path d="M18 15v5M15.5 17.5h5" /></>,
  clock: <><circle cx="12" cy="12" r="8.75" /><path d="M12 7.25V12l3.25 2" /></>,
  calendar: <><rect x="3.75" y="5.25" width="16.5" height="15" rx="2" /><path d="M3.75 10h16.5M8.25 3.25v3.5M15.75 3.25v3.5" /></>,
  warning: <><path d="M10.4 4.45 2.95 17.5A1.85 1.85 0 0 0 4.55 20.25h14.9a1.85 1.85 0 0 0 1.6-2.75L13.6 4.45a1.85 1.85 0 0 0-3.2 0Z" /><path d="M12 9.5v4.25" />{dot(12, 16.9)}</>,
  archive: <><rect x="3.25" y="4" width="17.5" height="4.5" rx="1" /><path d="M5 8.5v10a1.5 1.5 0 0 0 1.5 1.5h11a1.5 1.5 0 0 0 1.5-1.5v-10M10 12.25h4" /></>,
  history: <><path d="M4.25 12a7.75 7.75 0 1 0 2.3-5.5L4.25 8.75" /><path d="M4.25 4.5v4.25H8.5" /><path d="M12 8v4.25l2.75 1.75" /></>,
  cloud: <path d="M7.25 18.5a4.25 4.25 0 0 1-.6-8.46 5.75 5.75 0 0 1 11.1 1.06 3.75 3.75 0 0 1-.5 7.4H7.25Z" />,
  copy: <><rect x="8.5" y="8.5" width="11.75" height="11.75" rx="2" /><path d="M15.5 8.5V5.75a2 2 0 0 0-2-2h-7.75a2 2 0 0 0-2 2v7.75a2 2 0 0 0 2 2H8.5" /></>,
  external: <><path d="M13.75 4.25h6v6M19.75 4.25 11 13" /><path d="M17.75 14v4.25a1.5 1.5 0 0 1-1.5 1.5H5.75a1.5 1.5 0 0 1-1.5-1.5V7.75a1.5 1.5 0 0 1 1.5-1.5H10" /></>,
  file: <><path d="M13.75 3.5H7a1.75 1.75 0 0 0-1.75 1.75v13.5A1.75 1.75 0 0 0 7 20.5h10a1.75 1.75 0 0 0 1.75-1.75V8.5l-5-5Z" /><path d="M13.5 3.75V8.75h5" /></>,
  shield: <><path d="M12 3.5 5 6.25v5.5c0 4.25 3 7.5 7 8.75 4-1.25 7-4.5 7-8.75v-5.5L12 3.5Z" /><path d="m9 12 2.25 2.25L15.25 10" /></>,
  text: <><path d="M5 7V5h14v2M12 5v14M9 19h6" /></>,
  star: <path d="m12 4 2.4 4.95 5.35.75-3.9 3.75.95 5.35L12 16.25 7.2 18.8l.95-5.35-3.9-3.75 5.35-.75L12 4Z" />,
  target: <><circle cx="12" cy="12" r="8.75" /><circle cx="12" cy="12" r="4.75" />{dot(12, 12)}</>,
  layers: <><path d="m12 4 8.5 4.5L12 13 3.5 8.5 12 4Z" /><path d="m3.5 12.5 8.5 4.5 8.5-4.5" /><path d="m3.5 16.25 8.5 4.25 8.5-4.25" /></>,
  palace: <><path d="M3.5 9.25C6.5 5.9 9.15 4.5 12 4.5s5.5 1.4 8.5 4.75" /><path d="M4.5 10.25h15" /><path d="M7 10.25v8.5M12 10.25v8.5M17 10.25v8.5" /><path d="M3.5 19.25h17" /></>,
  // ── editor, browse, stats, note types ──
  bold: <path d="M7 4.75h5.75a3.5 3.5 0 0 1 0 7H7V4.75ZM7 11.75h6.5a3.75 3.75 0 0 1 0 7.5H7v-7.5Z" />,
  italic: <path d="M10.5 4.75h7M6.5 19.25h7M14 4.75l-4 14.5" />,
  underline: <path d="M7 4.5v6.75a5 5 0 0 0 10 0V4.5M5.5 20h13" />,
  subscript: <><path d="m4.5 6.5 8 9M12.5 6.5l-8 9" /><path d="M16.5 15.25a1.75 1.75 0 1 1 3 1.25L16.5 20h3.25" /></>,
  superscript: <><path d="m4.5 8.5 8 9M12.5 8.5l-8 9" /><path d="M16.5 5.25a1.75 1.75 0 1 1 3 1.25L16.5 10h3.25" /></>,
  list: <><path d="M9.5 6.5h10.5M9.5 12h10.5M9.5 17.5h10.5" />{dot(5, 6.5)}{dot(5, 12)}{dot(5, 17.5)}</>,
  link: <><path d="M10 14a3.75 3.75 0 0 0 5.3 0l3.2-3.2a3.75 3.75 0 0 0-5.3-5.3l-1 1" /><path d="M14 10a3.75 3.75 0 0 0-5.3 0l-3.2 3.2a3.75 3.75 0 0 0 5.3 5.3l1-1" /></>,
  maths: <path d="M18 5.5H7.5l5.25 6.5-5.25 6.5H18" />,
  code: <path className="icon-flip" d="m8.5 7-5 5 5 5M15.5 7l5 5-5 5" />,
  eraser: <><path d="M9.25 19.5h10.25" /><path d="m4.9 14.6 9.2-9.2a1.75 1.75 0 0 1 2.5 0l2.6 2.6a1.75 1.75 0 0 1 0 2.5L11 18.75a2.5 2.5 0 0 1-1.75.75H8.6a2.5 2.5 0 0 1-1.77-.73L4.9 16.83a1.6 1.6 0 0 1 0-2.23ZM9.5 10l5 5" /></>,
  pin: <><path d="M9 4.25h6M10 4.25v5.25L6.75 13.5h10.5L14 9.5V4.25" /><path d="M12 13.5v6.25" /></>,
  rtl: <><path d="M10 4.5h7.5M14.5 4.5v11M11.5 4.5v11M11.5 10H10a2.75 2.75 0 0 1 0-5.5" /><path d="M20 19.5H6M8.5 17 6 19.5 8.5 22" /></>,
  tag: <><path d="M4 4.75v6.4c0 .46.18.9.5 1.23l7.6 7.6a1.75 1.75 0 0 0 2.48 0l5.15-5.16a1.75 1.75 0 0 0 0-2.47l-7.6-7.6A1.75 1.75 0 0 0 10.9 4.25H4.5" /><circle cx="8.25" cy="8.5" r="1.25" /></>,
  deck: <><rect x="4" y="8" width="16" height="11.5" rx="2" /><path d="M6.5 5.25h11M8.75 2.75h6.5" /></>,
  columns: <><rect x="3.5" y="4.5" width="17" height="15" rx="2" /><path d="M9.25 4.5v15M14.75 4.5v15" /></>,
  'sort-asc': <><path d="M7 19V5M3.75 8.25 7 5l3.25 3.25" /><path d="M13.5 7h6.5M13.5 12h4.5M13.5 17h2.5" /></>,
  'sort-desc': <><path d="M7 5v14M3.75 15.75 7 19l3.25-3.25" /><path d="M13.5 7h2.5M13.5 12h4.5M13.5 17h6.5" /></>,
  cursor: <path d="M5.5 4.5 18 10.25l-5.5 1.75-2.25 5.5L5.5 4.5Z" />,
  rect: <rect x="4" y="6" width="16" height="12" rx="1.5" />,
  ellipse: <ellipse cx="12" cy="12" rx="8.5" ry="6.25" />,
  polygon: <><path d="M12 4 20 9.75 17 19.5H7L4 9.75 12 4Z" /></>,
  'zoom-fit': <><path d="M4 9V5.5A1.5 1.5 0 0 1 5.5 4H9M15 4h3.5A1.5 1.5 0 0 1 20 5.5V9M20 15v3.5a1.5 1.5 0 0 1-1.5 1.5H15M9 20H5.5A1.5 1.5 0 0 1 4 18.5V15" /><rect x="8.5" y="8.5" width="7" height="7" rx="1" /></>,
  'zoom-in': <><circle cx="10.75" cy="10.75" r="6.25" /><path d="m15.5 15.5 5 5M8.25 10.75h5M10.75 8.25v5" /></>,
  'zoom-out': <><circle cx="10.75" cy="10.75" r="6.25" /><path d="m15.5 15.5 5 5M8.25 10.75h5" /></>,
  eye: <><path d="M3 12c2-4 5.25-6.25 9-6.25S19 8 21 12c-2 4-5.25 6.25-9 6.25S5 16 3 12Z" /><circle cx="12" cy="12" r="2.75" /></>,
  group: <><rect x="3.75" y="3.75" width="10" height="10" rx="1.5" /><rect x="10.25" y="10.25" width="10" height="10" rx="1.5" /></>,
  grip: <>{dot(9, 6.5)}{dot(15, 6.5)}{dot(9, 12)}{dot(15, 12)}{dot(9, 17.5)}{dot(15, 17.5)}</>,
  'arrow-up': <path d="M12 19.5V5M6.5 10.5 12 5l5.5 5.5" />,
  'arrow-down': <path d="M12 4.5V19M6.5 13.5 12 19l5.5-5.5" />,
  sidebar: <><rect x="3.5" y="4.5" width="17" height="15" rx="2" /><path d="M9 4.5v15" /></>,
  replace: <><path d="M4.5 8.5h11.25M12.5 5.25l3.25 3.25-3.25 3.25" /><path d="M19.5 15.5H8.25M11.5 12.25 8.25 15.5l3.25 3.25" /></>,
  reposition: <><path d="M5 6.5h9M5 12h9M5 17.5h5" /><path d="M18 5v13.5M15.25 15.75 18 18.5l2.75-2.75" /></>,
  forget: <><path d="M4.25 12a7.75 7.75 0 1 0 2.3-5.5L4.25 8.75" /><path d="M4.25 4.5v4.25H8.5" /><path d="m9.5 9.5 5 5M14.5 9.5l-5 5" /></>,
} satisfies Record<string, ReactNode>;

export type IconName = keyof typeof paths;

export interface IconProps extends Omit<SVGProps<SVGSVGElement>, 'name'> {
  name: IconName;
  /** pixel size; default 20 */
  size?: number;
  /** accessible label; when absent the icon is decorative (aria-hidden) */
  label?: string;
}

export function Icon({ name, size = 20, label, className, ...rest }: IconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className ? `loci-icon ${className}` : 'loci-icon'}
      aria-hidden={label ? undefined : true}
      role={label ? 'img' : undefined}
      aria-label={label}
      focusable="false"
      {...rest}
    >
      <g vectorEffect="non-scaling-stroke">{paths[name]}</g>
    </svg>
  );
}
