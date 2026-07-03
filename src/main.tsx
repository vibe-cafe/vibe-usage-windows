import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./styles/globals.css";
import { PopoverApp } from "./PopoverApp";
import { SettingsApp } from "./SettingsApp";
import { AppStateProvider } from "./state/AppStateContext";

// Single HTML entry — the window label decides what to render (multi-page
// HTML entries proved unreliable in packaged builds: blank settings window).
const label = getCurrentWindow().label;

const root = ReactDOM.createRoot(document.getElementById("root")!);

if (label === "settings") {
  document.body.style.background = "#1C1C1E";
  root.render(
    <React.StrictMode>
      <SettingsApp />
    </React.StrictMode>,
  );
} else {
  root.render(
    <React.StrictMode>
      <AppStateProvider>
        <PopoverApp />
      </AppStateProvider>
    </React.StrictMode>,
  );
}
