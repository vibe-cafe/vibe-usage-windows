// The Settings window's look: macOS `SettingsTheme.classic` (the finalized
// 2026-11-20 design, `交接笔记/设置页设计-Fable/`), value for value. Dark only:
// the window pins a dark title bar (`theme: "Dark"`) and these are fixed
// colors, not system ones. Windows has no 「新版」 settings theme yet; the
// panel here is the classic one, so the settings window is too.

export const T = {
  // Window
  /** Includes the 1 px divider on its right. */
  sidebarWidth: 168,
  pageTopInset: 20,
  pageHorizontalInset: 22,
  pageBottomInset: 22,
  /** Space between two untitled cards. */
  cardGap: 16,
  /** Cards stop widening here when the (resizable) window is made wide, so
   *  the table never stretches into a long thin strip. */
  pageMaxWidth: 640,

  // Surfaces
  background: "#1E1E1E",
  sidebarBackground: "#181818",
  card: "#292929",
  rowLeading: 14,
  rowTrailing: 12,
  rowHeight: 44,
  tallRowHeight: 52,
  /** A drag preview and the open menu: one step above the card. */
  raised: "#383838",
  /** The rounded block an expanded row opens into. */
  configBlock: "#323232",
  /** Text fields, path lines, the device code. */
  inset: "#1B1B1B",
  /** The 「未开启」 band. */
  band: "#222222",
  line: "rgba(255,255,255,0.075)",
  strongLine: "rgba(255,255,255,0.14)",
  selection: "rgba(255,255,255,0.11)",

  // Text
  text: "#E9E9E9",
  secondaryText: "#9C9C9C",
  tertiaryText: "#6A6A6A",

  // Accents: green is "on", red is error / expiry / reset, amber is "needs X".
  ok: "#3CCB7F",
  amber: "#E5A43B",
  red: "#F0645D",

  // Type (px). Windows renders with Segoe UI / Microsoft YaHei UI.
  font: '-apple-system, "Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI", "PingFang SC", sans-serif',
  mono: '"JetBrains Mono", "SF Mono", Consolas, monospace',
} as const;

/** Text styles of the classic theme (size / weight), as inline style objects. */
export const TYPE = {
  pageTitle: { fontSize: 15, fontWeight: 600 },
  label: { fontSize: 13, fontWeight: 500 },
  body: { fontSize: 13, fontWeight: 400 },
  mono: { fontSize: 12, fontWeight: 400, fontFamily: T.mono },
  caption: { fontSize: 11, fontWeight: 400 },
  tag: { fontSize: 10, fontWeight: 500 },
  sectionTitle: { fontSize: 12, fontWeight: 600 },
} as const;
