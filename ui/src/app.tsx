import { useEffect, useState } from "preact/hooks";
import { onEvent } from "./api";
import { Cat } from "./components/cat";
import { Icon, type IconName } from "./components/icons";
import { Toasts } from "./components/toast";
import { DictionaryPage } from "./pages/dictionary";
import { HistoryPage } from "./pages/history";
import { OverviewPage } from "./pages/overview";
import { SettingsPage } from "./pages/settings";

const PAGES: { id: string; label: string; icon: IconName }[] = [
  { id: "overview", label: "Overview", icon: "overview" },
  { id: "history", label: "History", icon: "history" },
  { id: "dictionary", label: "Dictionary", icon: "dictionary" },
  { id: "settings", label: "Settings", icon: "settings" },
];

declare global {
  interface Window {
    __MEW_PAGE?: string; // starting page, set by mew.exe
  }
}

const known = (id: string | undefined) => (PAGES.some((p) => p.id === id) ? id! : "overview");

export function App() {
  const [page, setPage] = useState(() => known(window.__MEW_PAGE));
  const go = (id: string) => setPage(known(id));
  useEffect(() => onEvent((e) => e.type === "navigate" && go(e.page)), []);

  return (
    <div class="shell">
      <nav class="sidebar" aria-label="Mew">
        <div class="brand">
          <Cat size={34} />
          <span>Mew</span>
        </div>
        <ul class="nav">
          {PAGES.map((p) => (
            <li key={p.id}>
              <button type="button" class="nav-item" aria-current={p.id === page ? "page" : undefined} onClick={() => go(p.id)}>
                <Icon name={p.icon} />
                {p.label}
              </button>
            </li>
          ))}
        </ul>
        <p class="sidebar-foot">Runs entirely on this PC.</p>
      </nav>
      <main class="content" key={page}>
        {page === "overview" && <OverviewPage go={go} />}
        {page === "history" && <HistoryPage go={go} />}
        {page === "dictionary" && <DictionaryPage />}
        {page === "settings" && <SettingsPage />}
      </main>
      <Toasts />
    </div>
  );
}
