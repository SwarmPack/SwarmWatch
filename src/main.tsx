import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./App.css";

// Dev-only diagnostics to surface silent errors (useful when running in a
// regular browser vs inside Tauri).
window.addEventListener('error', (e) => {
  console.error('[window.error]', e.error ?? e.message);
});
window.addEventListener('unhandledrejection', (e) => {
  console.error('[unhandledrejection]', e.reason);
});
console.log('[main] boot');

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
