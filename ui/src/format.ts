const number = new Intl.NumberFormat();
const time = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });
const weekday = new Intl.DateTimeFormat(undefined, { weekday: "long", day: "numeric", month: "long" });
const shortDay = new Intl.DateTimeFormat(undefined, { weekday: "short", day: "numeric", month: "short" });
const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

export const fmtNumber = (n: number) => number.format(Math.round(n));
export const fmtTime = (ms: number) => time.format(ms);

export function fmtDuration(minutes: number): string {
  if (minutes < 1) return "<1m";
  const h = Math.floor(minutes / 60);
  const m = Math.round(minutes % 60);
  return h ? `${h}h ${m}m` : `${m}m`;
}

export function fmtSeconds(ms: number): string {
  return ms < 1000 ? `${(ms / 1000).toFixed(2)}s` : `${(ms / 1000).toFixed(1)}s`;
}

/** "Today", "Yesterday", or "Tuesday, 21 October". */
export function fmtDay(ms: number): string {
  const day = startOfDay(ms);
  const today = startOfDay(Date.now());
  if (day === today) return "Today";
  if (day === today - 86_400_000) return "Yesterday";
  return weekday.format(ms);
}

/** For chart tooltips: "Tue, 21 Oct". `iso` is a local YYYY-MM-DD date. */
export function fmtIsoDay(iso: string): string {
  const [y, m, d] = iso.split("-").map(Number);
  return shortDay.format(new Date(y, m - 1, d));
}

export function fmtAgo(ms: number): string {
  const seconds = (ms - Date.now()) / 1000;
  const units: [Intl.RelativeTimeFormatUnit, number][] = [["day", 86400], ["hour", 3600], ["minute", 60]];
  for (const [unit, size] of units) {
    if (Math.abs(seconds) >= size) return relative.format(Math.round(seconds / size), unit);
  }
  return "just now";
}

export function startOfDay(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

export function greeting(): string {
  const hour = new Date().getHours();
  if (hour < 5) return "Up late";
  if (hour < 12) return "Good morning";
  if (hour < 17) return "Good afternoon";
  return "Good evening";
}

export const plural = (n: number, one: string, many = `${one}s`) => `${fmtNumber(n)} ${n === 1 ? one : many}`;
