import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { type Entry, type Settings, call, onEvent } from "../api";
import { Cat } from "../components/cat";
import { Confirm } from "../components/controls";
import { Icon } from "../components/icons";
import { toast } from "../components/toast";
import { fmtDay, fmtTime, plural, startOfDay } from "../format";
import { mac } from "../platform";

type Page = { items: Entry[]; more: boolean };

export function HistoryPage({ go }: { go(page: string): void }) {
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<Entry[] | null>(null);
  const [more, setMore] = useState(false);
  const [hidden, setHidden] = useState<Set<number>>(new Set());
  const [keep, setKeep] = useState("");
  const [confirmClear, setConfirmClear] = useState(false);
  const search = useRef<HTMLInputElement>(null);
  const sentinel = useRef<HTMLDivElement>(null);

  const load = (q: string, before?: number) =>
    call<Page>("history", { query: q, before }).then((page) => {
      setItems((prev) => (before && prev ? [...prev, ...page.items] : page.items));
      setMore(page.more);
    });

  useEffect(() => {
    const t = setTimeout(() => load(query), query ? 150 : 0);
    return () => clearTimeout(t);
  }, [query]);

  useEffect(() => {
    call<Settings>("settings").then((s) => setKeep(s.historyOptions.find((o) => o.id === s.history)?.label ?? ""));
    const off = onEvent((e) => {
      if (e.type === "dictation" && e.entry.text && !query) setItems((prev) => [e.entry, ...(prev ?? [])]);
    });
    const keys = (ev: KeyboardEvent) => {
      if ((mac ? ev.metaKey : ev.ctrlKey) && ev.key.toLowerCase() === "f") {
        ev.preventDefault();
        search.current?.focus();
      }
    };
    window.addEventListener("keydown", keys);
    return () => {
      off();
      window.removeEventListener("keydown", keys);
    };
  }, [query]);

  // Load the next page when the end of the list scrolls into view.
  useEffect(() => {
    const el = sentinel.current;
    if (!el || !more || !items?.length) return;
    const observer = new IntersectionObserver(([entry]) => entry.isIntersecting && load(query, items[items.length - 1].at));
    observer.observe(el);
    return () => observer.disconnect();
  }, [more, items, query]);

  const visible = (items ?? []).filter((e) => !hidden.has(e.id));
  const groups = useMemo(() => {
    const byDay = new Map<number, Entry[]>();
    for (const e of visible) {
      const day = startOfDay(e.at);
      byDay.set(day, [...(byDay.get(day) ?? []), e]);
    }
    return [...byDay.entries()];
  }, [visible]);

  const remove = (entry: Entry) => {
    setHidden((h) => new Set(h).add(entry.id));
    const timer = setTimeout(() => call("forget", { id: entry.id }), 5000);
    toast("Transcript deleted", {
      label: "Undo",
      run: () => {
        clearTimeout(timer);
        setHidden((h) => {
          const next = new Set(h);
          next.delete(entry.id);
          return next;
        });
      },
    });
  };

  const clearAll = async () => {
    setConfirmClear(false);
    await call("forgetAll");
    setItems([]);
    setMore(false);
    toast("All transcripts deleted. Your stats are kept.");
  };

  return (
    <div class="page">
      <header class="page-header with-action">
        <div>
          <h1>History</h1>
          <p class="lede">
            {keep === "Don't keep"
              ? "Transcripts aren't being kept right now."
              : keep ? `Transcripts are kept for ${keep === "Forever" ? "good" : keep}.` : ""}{" "}
            <button type="button" class="link" onClick={() => go("settings")}>Change</button>
          </p>
        </div>
        <button type="button" class="button" onClick={() => setConfirmClear(true)} disabled={!visible.length && !query}>
          <Icon name="trash" size={16} /> Clear all
        </button>
      </header>

      <div class="search">
        <Icon name="search" size={18} />
        <input ref={search} type="search" placeholder="Search transcripts and apps" aria-label="Search transcripts"
          value={query} onInput={(e) => setQuery(e.currentTarget.value)}
          onKeyDown={(e) => e.key === "Escape" && setQuery("")} />
        <kbd class="search-hint">{mac ? "⌘ F" : "Ctrl F"}</kbd>
      </div>

      {items === null ? null : !visible.length ? (
        <div class="empty-state small">
          <Cat size={64} mood="asleep" />
          <p>{query ? `Nothing matches “${query}”.` : "No transcripts yet. Hold your talk key and say something."}</p>
        </div>
      ) : (
        groups.map(([day, entries]) => (
          <section class="day" key={day} aria-label={fmtDay(day)}>
            <h2 class="day-title">{fmtDay(day)}</h2>
            <ol class="entries">
              {entries.map((e) => <EntryRow key={e.id} entry={e} onDelete={() => remove(e)} />)}
            </ol>
          </section>
        ))
      )}
      <div ref={sentinel} />

      <Confirm open={confirmClear} title="Delete all transcripts?"
        body="This removes the text of every dictation. Your word counts and stats stay."
        action="Delete transcripts" onConfirm={clearAll} onCancel={() => setConfirmClear(false)} />
    </div>
  );
}

function EntryRow({ entry, onDelete }: { entry: Entry; onDelete(): void }) {
  const [original, setOriginal] = useState(false);
  const text = original && entry.raw ? entry.raw : entry.text ?? "";
  const copy = () => call("copy", { text }).then(() => toast("Copied"), () => toast("Couldn't copy that"));
  return (
    <li class="entry">
      <time class="entry-time" dateTime={new Date(entry.at).toISOString()}>{fmtTime(entry.at)}</time>
      <div class="entry-body">
        <p class={original ? "entry-text original" : "entry-text"}>{text}</p>
        <div class="entry-meta">
          <span class="chip">{entry.app}</span>
          <span>{plural(entry.words, "word")}</span>
          {entry.raw && (
            <button type="button" class="link small" onClick={() => setOriginal(!original)} aria-pressed={original}>
              <Icon name="sparkle" size={14} /> {original ? "Show tidied" : "Show what I heard"}
            </button>
          )}
        </div>
      </div>
      <div class="entry-actions">
        <button type="button" class="icon-button" onClick={copy} aria-label="Copy transcript" title="Copy">
          <Icon name="copy" size={18} />
        </button>
        <button type="button" class="icon-button" onClick={onDelete} aria-label="Delete transcript" title="Delete">
          <Icon name="trash" size={18} />
        </button>
      </div>
    </li>
  );
}
