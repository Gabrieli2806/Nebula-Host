import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

// Bundled faces (the CSP blocks remote fonts). Both are the variable wght
// files: Quicksand 300-700 for the interface and headings, JetBrains Mono
// 100-800 for addresses, numbers and logs. Subsets load by unicode-range, so
// an English / Spanish UI only fetches the latin woff2 files.
import "@fontsource-variable/quicksand/wght.css";
import "@fontsource-variable/jetbrains-mono/wght.css";

// global.css must load before App: component CSS files imported from TSX
// open `@layer components`, and the first stylesheet to name a layer fixes
// the cascade order. Loading Tailwind first keeps theme < base < components.
import "./styles/global.css";

import { App } from "./App";
import { applyStoredDensity } from "./app/density";
import { applyStoredTheme } from "./app/theme";

// Before the first paint, so a light-theme user never sees a dark flash and
// a compact user never sees the rows jump.
applyStoredTheme();
applyStoredDensity();

const container = document.getElementById("root");
if (!container) throw new Error("index.html is missing #root");

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
