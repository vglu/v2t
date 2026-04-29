import React from "react";
import ReactDOM from "react-dom/client";

// i18n must initialize *before* any component that calls useTranslation.
// The import has side effects only (registers detectors and inits i18next);
// it must stay above the App import so the very first render is localized.
import "./i18n";
import "./i18n/types";

import App from "./App";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
