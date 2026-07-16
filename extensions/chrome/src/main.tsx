import { createRoot } from "react-dom/client";

import { App } from "./App";
import "./styles.css";

const root = document.getElementById("zode-react-root");

if (!root) {
  throw new Error("zode React root is missing");
}

createRoot(root).render(<App />);
