import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./styles/globals.css";
import { PopoverApp } from "./PopoverApp";
import { SettingsApp } from "./SettingsApp";
import { AppStateProvider } from "./state/AppStateContext";

// Single HTML entry. Dynamically-created windows pass an explicit route query;
// the Tauri window label is the fallback for packaged app windows.
function getWindowRoute() {
  const route = new URLSearchParams(window.location.search).get("window");
  if (route) return route;

  try {
    return getCurrentWindow().label;
  } catch {
    return "popover";
  }
}

const route = getWindowRoute();

const root = ReactDOM.createRoot(document.getElementById("root")!);

if (route === "settings") {
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
