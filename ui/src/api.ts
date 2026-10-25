// Bridge to mew.exe: requests go out over window.ipc, replies and live events come back via window.__mew.

export type Entry = {
  id: number;
  at: number;
  app: string;
  text: string | null;
  raw: string | null;
  words: number;
  audioMs: number;
  latencyMs: number;
  cleaned: boolean;
};

export type Stats = {
  totalWords: number;
  totalDictations: number;
  wordsToday: number;
  wordsWeek: number;
  minutesSaved: number;
  speakingWpm: number;
  medianLatencyMs: number;
  streakDays: number;
  daily: [string, number][];
  apps: [string, number, number][];
};

export type Overview = { stats: Stats; recent: Entry[]; talkKey: string };

export type Option = { id: string; label: string };

export type Settings = {
  hotkey: string;
  hotkeys: Option[];
  microphone: string | null;
  microphones: string[];
  cleanup: boolean;
  autostart: boolean;
  history: string;
  historyOptions: Option[];
  version: string;
  models: { name: string; role: string; ready: boolean }[];
};

export type MewEvent =
  | { type: "dictation"; entry: Entry }
  | { type: "settings" }
  | { type: "navigate"; page: string };

type Reply = { id: number; ok: boolean; data?: unknown; error?: string };

declare global {
  interface Window {
    ipc?: { postMessage(message: string): void };
    __mew?: { receive(message: Reply | { event: MewEvent }): void };
  }
}

const pending = new Map<number, { resolve(v: unknown): void; reject(e: Error): void }>();
const listeners = new Set<(event: MewEvent) => void>();
let nextId = 1;

window.__mew = {
  receive(message) {
    if ("event" in message) {
      listeners.forEach((listener) => listener(message.event));
      return;
    }
    const waiter = pending.get(message.id);
    if (!waiter) return;
    pending.delete(message.id);
    if (message.ok) waiter.resolve(message.data);
    else waiter.reject(new Error(message.error ?? "Request failed"));
  },
};

export function call<T>(method: string, params: Record<string, unknown> = {}): Promise<T> {
  const id = nextId++;
  return new Promise<T>((resolve, reject) => {
    if (!window.ipc) {
      reject(new Error("Not running inside Mew"));
      return;
    }
    pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
    window.ipc.postMessage(JSON.stringify({ id, method, params }));
  });
}

export function onEvent(listener: (event: MewEvent) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
