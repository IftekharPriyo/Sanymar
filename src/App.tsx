import { useState } from "react";
import { DashboardPage } from "./pages/DashboardPage";
import { SettingsPage } from "./pages/SettingsPage";
import "./styles.css";

type Page = "dashboard" | "settings";

export default function App() {
  const [page, setPage] = useState<Page>("dashboard");
  return (
    <div className="app-shell">
      <header className="topbar">
        <button className="brand" onClick={() => setPage("dashboard")}>
          <span className="brand-mark">S</span>
          <span>Sanymar</span>
        </button>
        <nav aria-label="Primary navigation">
          <button
            className={page === "dashboard" ? "active" : ""}
            onClick={() => setPage("dashboard")}
          >
            Studio
          </button>
          <button
            className={page === "settings" ? "active" : ""}
            onClick={() => setPage("settings")}
          >
            Settings
          </button>
        </nav>
      </header>
      {page === "dashboard" ? <DashboardPage /> : <SettingsPage />}
    </div>
  );
}
