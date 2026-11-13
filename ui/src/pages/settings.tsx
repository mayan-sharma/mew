import { useEffect, useState } from "preact/hooks";
import { type Settings, call, onEvent } from "../api";
import { Confirm, Row, Segmented, Toggle } from "../components/controls";
import { Icon } from "../components/icons";
import { toast } from "../components/toast";
import { device, mac } from "../platform";

const startAtLogin = mac ? "Open at login" : "Start with Windows";

export function SettingsPage() {
  const [s, setS] = useState<Settings | null>(null);
  const [confirm, setConfirm] = useState<"transcripts" | "stats" | null>(null);
  const load = () => call<Settings>("settings").then(setS);
  useEffect(() => {
    load();
    return onEvent((e) => e.type === "settings" && load());
  }, []);
  if (!s) return <div class="page" aria-busy="true" />;

  const set = (key: string, value: unknown) => {
    setS({ ...s, [key]: value }); // optimistic; the reply carries the real state
    call<Settings>("set", { key, value }).then(setS).catch(() => {
      toast("Couldn't save that setting");
      load();
    });
  };

  const install = () => call("installUpdate").catch(() => toast("Couldn't start the update"));

  const wipe = async () => {
    const which = confirm;
    setConfirm(null);
    await call(which === "stats" ? "resetStats" : "forgetAll");
    toast(which === "stats" ? "Stats reset" : "All transcripts deleted");
  };

  return (
    <div class="page narrow">
      <header class="page-header">
        <h1>Settings</h1>
      </header>

      <section class="settings-group" aria-labelledby="talking">
        <h2 id="talking">Talking</h2>
        <Row title="Talk key" description="Hold it to talk, release to type. Double-tap for hands-free, Esc to cancel. Shift + the key types your last dictation again.">
          <Segmented label="Talk key" value={s.hotkey} options={s.hotkeys} onChange={(v) => set("hotkey", v)} />
        </Row>
        <Row title="Microphone" htmlFor="mic">
          <select id="mic" class="select" value={s.microphone ?? ""}
            onChange={(e) => set("microphone", e.currentTarget.value || null)}>
            <option value="">{mac ? "System default" : "Windows default"}</option>
            {s.microphones.map((m) => <option value={m}>{m}</option>)}
          </select>
        </Row>
        <Row title="Tidy up with AI"
          description="Removes ums, applies “no wait, I mean…” corrections and fixes punctuation. Off types exactly what was heard.">
          <Toggle label="Tidy up with AI" checked={s.cleanup} onChange={(v) => set("cleanup", v)} />
        </Row>
        <Row title="Learn from my edits"
          description="After Mew types, it watches that text box for 30 seconds. If you fix a word, Mew uses your spelling next time. Never reads password fields.">
          <Toggle label="Learn from my edits" checked={s.learn} onChange={(v) => set("learn", v)} />
        </Row>
      </section>

      <section class="settings-group" aria-labelledby="privacy">
        <h2 id="privacy">History and privacy</h2>
        <p class="group-note">Your words stay on this {device}. Audio is never saved. Mew only goes online to download its models and to check GitHub for updates.</p>
        <Row title="Keep transcripts" htmlFor="keep" description="Word counts and stats are kept either way.">
          <select id="keep" class="select" value={s.history} onChange={(e) => set("history", e.currentTarget.value)}>
            {s.historyOptions.map((o) => <option value={o.id}>{o.label}</option>)}
          </select>
        </Row>
        <Row title="Delete all transcripts" description="Removes the text of every dictation.">
          <button type="button" class="button" onClick={() => setConfirm("transcripts")}>Delete transcripts</button>
        </Row>
        <Row title="Reset stats" description="Starts your numbers and streak from zero.">
          <button type="button" class="button" onClick={() => setConfirm("stats")}>Reset stats</button>
        </Row>
      </section>

      <section class="settings-group" aria-labelledby="general">
        <h2 id="general">General</h2>
        <Row title={startAtLogin}>
          <Toggle label={startAtLogin} checked={s.autostart} onChange={(v) => set("autostart", v)} />
        </Row>
        <Row title="Check for updates automatically" description="Asks GitHub for the newest version every 12 hours.">
          <Toggle label="Check for updates automatically" checked={s.autoUpdates} onChange={(v) => set("autoUpdates", v)} />
        </Row>
        <Row title="Updates" description={s.updateReady
          ? `You're on Mew ${s.version}. Mew ${s.updateReady} is ${mac ? "out" : "ready"}.`
          : `You're on Mew ${s.version}.`}>
          {s.updateReady
            ? <button type="button" class="button primary" onClick={install}>{mac ? "Download" : "Restart to update"}</button>
            : <button type="button" class="button" onClick={() => call("checkUpdates")}>Check for updates</button>}
        </Row>
      </section>

      <section class="settings-group" aria-labelledby="about">
        <h2 id="about">About</h2>
        <ul class="models">
          {s.models.map((m) => (
            <li key={m.name}>
              <span class={m.ready ? "status ready" : "status pending"} aria-hidden="true">
                <Icon name={m.ready ? "check" : "mic"} size={14} />
              </span>
              <span class="model-name">{m.name}</span>
              <span class="muted">{m.role} · {m.ready ? "ready" : "downloading"}</span>
            </li>
          ))}
        </ul>
        <div class="about-links">
          <button type="button" class="link" onClick={() => call("open", { target: "notices" })}>Licenses</button>
          <button type="button" class="link" onClick={() => call("open", { target: "logs" })}>Logs folder</button>
          <button type="button" class="link" onClick={() => call("open", { target: "data" })}>Data folder</button>
        </div>
      </section>

      <Confirm open={confirm !== null}
        title={confirm === "stats" ? "Reset all stats?" : "Delete all transcripts?"}
        body={confirm === "stats"
          ? "Your word counts, streak and charts start over. Transcripts are deleted too."
          : "This removes the text of every dictation. Your word counts and stats stay."}
        action={confirm === "stats" ? "Reset stats" : "Delete transcripts"}
        onConfirm={wipe} onCancel={() => setConfirm(null)} />
    </div>
  );
}
