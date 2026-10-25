import { signal } from "./signal";

type Toast = { id: number; text: string; action?: { label: string; run(): void } };

export const toasts = signal<Toast[]>([]);
let nextId = 1;

/** Shows a short message; with `action`, a button (e.g. Undo) stays for 5 seconds. */
export function toast(text: string, action?: Toast["action"]) {
  const id = nextId++;
  toasts.set([...toasts.get().filter((t) => !t.action), { id, text, action }]);
  setTimeout(() => toasts.set(toasts.get().filter((t) => t.id !== id)), action ? 5000 : 2400);
}

export function Toasts() {
  const list = toasts.use();
  return (
    <div class="toasts" role="status" aria-live="polite">
      {list.map((t) => (
        <div class="toast" key={t.id}>
          <span>{t.text}</span>
          {t.action && (
            <button type="button" class="toast-action" onClick={() => {
              t.action!.run();
              toasts.set(toasts.get().filter((x) => x.id !== t.id));
            }}>
              {t.action.label}
            </button>
          )}
        </div>
      ))}
    </div>
  );
}
