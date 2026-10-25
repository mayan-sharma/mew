import { useLayoutEffect, useRef, useState } from "preact/hooks";
import { fmtIsoDay, fmtNumber, plural } from "../format";

/** Rounds up to 1, 2 or 5 × 10^k so gridlines land on friendly numbers. */
function niceMax(max: number): number {
  if (max <= 0) return 10;
  const magnitude = 10 ** Math.floor(Math.log10(max));
  return ([1, 2, 5, 10].find((step) => step * magnitude >= max) ?? 10) * magnitude;
}

/** Path for a bar anchored to the baseline with 4px rounded top corners. */
function barPath(x: number, y: number, w: number, h: number): string {
  const r = Math.min(4, w / 2, h);
  return `M${x},${y + h}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${y + h}Z`;
}

const HEIGHT = 196;
const M = { top: 10, right: 4, bottom: 26, left: 44 };

/** Words per day, oldest to newest. One series, so no legend: the section title names it. */
export function DailyBars({ days }: { days: [string, number][] }) {
  const box = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(640);
  const [hover, setHover] = useState<number | null>(null);
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const observer = new ResizeObserver(([entry]) => setWidth(Math.max(280, entry.contentRect.width)));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const top = niceMax(Math.max(...days.map(([, w]) => w)));
  const plotW = width - M.left - M.right;
  const plotH = HEIGHT - M.top - M.bottom;
  const band = plotW / days.length;
  const barW = Math.max(2, band - 2); // 2px surface gap between bars
  const y = (v: number) => M.top + plotH - (v / top) * plotH;
  const ticks = [0, top / 2, top];
  const labelled = new Set([0, Math.floor(days.length / 2), days.length - 1]);

  return (
    <div class="chart" ref={box} onMouseLeave={() => setHover(null)}>
      <svg width={width} height={HEIGHT} role="img" aria-label="Words dictated per day over the last 30 days">
        {ticks.map((t) => (
          <g key={t}>
            <line x1={M.left} x2={width - M.right} y1={y(t)} y2={y(t)} class="grid" />
            <text x={M.left - 8} y={y(t)} class="axis" text-anchor="end" dominant-baseline="middle">{fmtNumber(t)}</text>
          </g>
        ))}
        {days.map(([day, words], i) => {
          const x = M.left + i * band + (band - barW) / 2;
          const h = (words / top) * plotH;
          return (
            <g key={day}>
              {words > 0 && <path d={barPath(x, y(words), barW, h)} class={hover === i ? "bar active" : "bar"} />}
              <rect x={M.left + i * band} y={M.top} width={band} height={plotH} fill="transparent"
                onMouseEnter={() => setHover(i)} />
              {labelled.has(i) && (
                <text x={x + barW / 2} y={HEIGHT - 8} class="axis" text-anchor="middle">
                  {i === days.length - 1 ? "Today" : fmtIsoDay(day)}
                </text>
              )}
            </g>
          );
        })}
      </svg>
      {hover !== null && (
        <div class="tooltip" style={{
          left: `${Math.min(Math.max(M.left + hover * band + band / 2, 70), width - 70)}px`,
          top: `${Math.max(y(days[hover][1]) - 12, 0)}px`,
        }}>
          <strong>{plural(days[hover][1], "word")}</strong>
          <span>{fmtIsoDay(days[hover][0])}</span>
        </div>
      )}
      <table class="sr-only">
        <caption>Words per day</caption>
        <tbody>
          {days.map(([day, words]) => (
            <tr key={day}><th scope="row">{fmtIsoDay(day)}</th><td>{words}</td></tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** Top apps by words: direct labels on every row, so no tooltip or legend needed. */
export function AppBars({ apps }: { apps: [string, number, number][] }) {
  const max = Math.max(1, ...apps.map(([, w]) => w));
  return (
    <ul class="app-bars">
      {apps.map(([app, words, count]) => (
        <li key={app}>
          <div class="app-bar-head">
            <span class="app-name">{app}</span>
            <span class="app-value">{plural(words, "word")}</span>
          </div>
          <div class="app-bar-track" title={plural(count, "dictation")}>
            <div class="app-bar-fill" style={{ transform: `scaleX(${words / max})` }} />
          </div>
        </li>
      ))}
    </ul>
  );
}
