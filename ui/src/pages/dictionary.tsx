import { useEffect, useState } from "preact/hooks";
import { type Dictionary, call, onEvent } from "../api";
import { Icon } from "../components/icons";
import { toast } from "../components/toast";
import { fmtAgo } from "../format";

export function DictionaryPage({ go }: { go(page: string): void }) {
  const [dict, setDict] = useState<Dictionary | null>(null);
  const [draft, setDraft] = useState("");
  const load = () => call<Dictionary>("dictionary").then(setDict);
  useEffect(() => {
    load();
    return onEvent((e) => e.type === "dictionary" && load());
  }, []);

  const save = (terms: string[]) => call<Dictionary>("setDictionary", { terms }).then(setDict);

  const add = (e: Event) => {
    e.preventDefault();
    const word = draft.trim();
    if (!word || !dict) return;
    if (dict.terms.some((t) => t.toLowerCase() === word.toLowerCase())) {
      toast(`“${word}” is already in your dictionary`);
      return;
    }
    save([...dict.terms, word]);
    setDraft("");
  };

  const remove = (word: string) => {
    if (!dict) return;
    const before = dict.terms;
    save(before.filter((t) => t !== word));
    toast(`Removed “${word}”`, { label: "Undo", run: () => save(before) });
  };

  const unlearn = (heard: string, fixed: string) =>
    call<Dictionary>("forgetLearned", { heard }).then((d) => {
      setDict(d);
      toast(`Mew will type “${heard}” as heard again`);
    }, () => toast(`Couldn't forget “${fixed}”`));

  return (
    <div class="page narrow">
      <header class="page-header">
        <h1>Dictionary</h1>
        <p class="lede">Names, products and jargon Mew should always spell your way. Used from your next dictation.</p>
      </header>

      <form class="add-word" onSubmit={add}>
        <label for="new-word" class="field-label">Add a word or phrase</label>
        <div class="add-word-row">
          <input id="new-word" class="input" value={draft} onInput={(e) => setDraft(e.currentTarget.value)}
            placeholder="For example: Kubernetes" autocomplete="off" />
          <button type="submit" class="button primary" disabled={!draft.trim()}>
            <Icon name="plus" size={16} /> Add
          </button>
        </div>
      </form>

      {dict && (dict.terms.length ? (
        <ul class="terms" aria-label="Your words">
          {dict.terms.map((t) => (
            <li class="term" key={t}>
              <span>{t}</span>
              <button type="button" class="term-remove" onClick={() => remove(t)} aria-label={`Remove ${t}`}>
                <Icon name="close" size={14} />
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <p class="muted">No words yet. Add the ones Mew gets wrong, like a teammate's name or your product.</p>
      ))}

      <section class="section learned-section" aria-labelledby="learned-title">
        <div class="section-head">
          <h2 id="learned-title">Learned from your edits</h2>
        </div>
        {dict?.learned.length ? (
          <ul class="learned">
            {dict.learned.map((l) => (
              <li key={l.heard}>
                <span class="learned-pair">
                  <span class="learned-heard">{l.heard}</span>
                  <span class="learned-arrow" aria-label="becomes">→</span>
                  <strong>{l.fixed}</strong>
                </span>
                <span class="muted learned-when">{fmtAgo(l.at)}</span>
                <button type="button" class="icon-button" onClick={() => unlearn(l.heard, l.fixed)}
                  aria-label={`Stop replacing ${l.heard}`} title="Forget">
                  <Icon name="close" size={16} />
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p class="muted">
            When you fix a word right after Mew types it, Mew remembers the fix here.{" "}
            <button type="button" class="link" onClick={() => go("settings")}>Learning settings</button>
          </p>
        )}
      </section>
    </div>
  );
}
