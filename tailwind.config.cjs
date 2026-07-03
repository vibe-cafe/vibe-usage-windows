/**
 * Design tokens converted 1:1 from the macOS app's SwiftUI constants.
 * Every color below traces to a `Color(white:)` / `Color(red:green:blue:)`
 * literal in vibe-usage-app (see docs/PARITY.md). Do not eyeball-adjust.
 */
/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        app: "#0A0A0A", //  Color(white: 0.04)  popover background
        card: "#171717", //  Color(white: 0.09)  card background
        "card-border": "#292929", //  Color(white: 0.16)
        "filter-row": "#1A1A1A", //  Color(white: 0.10)
        "t-secondary": "#A1A1A1", //  Color(white: 0.63)
        "t-tertiary": "#616161", //  Color(white: 0.38)
        "t-muted": "#808080", //  Color(white: 0.5)
        cost: "#33CC80", //  Color(red:0.2, green:0.8, blue:0.5)
        active: "#6199FF", //  Color(red:0.38, green:0.6, blue:1.0)
        link: "#66B3FF", //  Color(red:0.4, green:0.7, blue:1.0)
        danger: "#FF6B6B", //  Color(red:1.0, green:0.42, blue:0.42)
        "quota-warn": "#F59E0B", //  rgb(0.96,0.62,0.04)
        "quota-critical": "#F04545", //  rgb(0.94,0.27,0.27)
      },
      fontFamily: {
        sans: ["-apple-system", "Segoe UI", "Microsoft YaHei UI", "PingFang SC", "sans-serif"],
        mono: ["JetBrains Mono", "SF Mono", "Consolas", "monospace"],
      },
      borderRadius: {
        card: "4px",
        panel: "12px",
      },
    },
  },
  plugins: [],
};
