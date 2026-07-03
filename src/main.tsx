import React from "react";
import ReactDOM from "react-dom/client";
import "./styles/globals.css";
import { PopoverApp } from "./PopoverApp";
import { AppStateProvider } from "./state/AppStateContext";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <AppStateProvider>
      <PopoverApp />
    </AppStateProvider>
  </React.StrictMode>,
);
