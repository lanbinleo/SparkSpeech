import React from "react";
import ReactDOM from "react-dom/client";
import { Overlay } from "./Overlay";
import { App } from "./App";
import "./styles.css";

const isOverlay = new URLSearchParams(window.location.search).get("overlay") === "1";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {isOverlay ? <Overlay /> : <App />}
  </React.StrictMode>,
);
