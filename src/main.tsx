import React from "react";
import ReactDOM from "react-dom/client";
import App from './App';
import './i18n'; // Import i18n config
import "./App.css";

import { invoke } from "@tauri-apps/api/core";
import { isMiniWindow, isTauri } from "./utils/env";
import { ensureFullViewState } from "./utils/windowManager";

// 配合 visible:false，先修复偏小的主窗口，再请求 Rust 显示；悬浮窗不走这里。
if (isTauri() && !isMiniWindow()) {
  ensureFullViewState()
    .then(() => invoke("show_main_window"))
    .catch(console.error);
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />

  </React.StrictMode>,
);
