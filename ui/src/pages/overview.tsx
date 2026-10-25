import { useEffect, useState } from "preact/hooks";
import { type Overview, call, onEvent } from "../api";
import { Cat } from "../components/cat";
import { AppBars, DailyBars } from "../components/charts";
import { fmtAgo, fmtDuration, fmtNumber, fmtSeconds, greeting, plural } from "../format";

export function OverviewPage({ go }: { go(page: string): void }) {
  const [data, setData] = useState<Overview | null>(null);
  const load = () => call<Overview>("overview").then(setData).catch(() => {});
  useEffect(() => {
    load();
    return onEvent((e) => (e.type === "dictation" || e.type === "settings") && load());
  }, []);
  if (!data) return <div class="page" aria-busy="true" />;

  const { stats, recent, talkKey } = data;
  if (stats.totalDictations === 0) {
    return (
      <div class="page empty-state">
        <Cat size={96} mood="asleep" />
        <h1>Nothing said yet</h1>
        <p>Hold <kbd>{talkKey}</kbd> in any app, say something, then let go. Your words and stats show up here.</p>
      </div>
    );
  }

  return (
    <div class="page">
      <header class="page-header">
        <h1>{greeting()}</h1>
        <p class="lede">
          {stats.wordsToday > 0
            ? <>You've said {plural(stats.wordsToday, "word")} today, {fmtNumber(stats.wordsWeek)} this week.</>
            : <>You've said {plural(stats.wordsWeek, "word")} this week. Hold <kbd>{talkKey}</kbd> to add more.</>}
        </p>
      </header>

      <section class="metrics" aria-label="Your numbers">
        <Metric value={fmtDuration(stats.minutesSaved)} label="Time saved" note="vs typing at 40 wpm" />
        <Metric value={fmtNumber(stats.totalWords)} label="Words, all time" note={plural(stats.totalDictations, "dictation")} />
        <Metric value={stats.streakDays ? plural(stats.streakDays, "day") : "0 days"} label="Streak"
          note={stats.streakDays ? "Keep it going" : "Talk today to start one"} />
        <Metric value={fmtSeconds(stats.medianLatencyMs)} label="Typical wait" note={`${fmtNumber(stats.speakingWpm)} wpm speaking`} />
      </section>

      <section class="section">
        <div class="section-head">
          <h2>Words per day</h2>
          <span class="section-note">Last 30 days</span>
        </div>
        <DailyBars days={stats.daily} />
      </section>

      <div class="split">
        <section class="section">
          <div class="section-head"><h2>Where you talk</h2></div>
          <AppBars apps={stats.apps} />
        </section>
        <section class="section">
          <div class="section-head">
            <h2>Latest</h2>
            <button type="button" class="link" onClick={() => go("history")}>See all</button>
          </div>
          {recent.length ? (
            <ul class="recent">
              {recent.map((e) => (
                <li key={e.id}>
                  <p class="recent-text">{e.text}</p>
                  <span class="recent-meta">{e.app} · {fmtAgo(e.at)}</span>
                </li>
              ))}
            </ul>
          ) : (
            <p class="muted">Transcripts aren't being kept. You can change that in Settings.</p>
          )}
        </section>
      </div>
    </div>
  );
}

function Metric({ value, label, note }: { value: string; label: string; note: string }) {
  return (
    <div class="metric">
      <div class="metric-value">{value}</div>
      <div class="metric-label">{label}</div>
      <div class="metric-note">{note}</div>
    </div>
  );
}
