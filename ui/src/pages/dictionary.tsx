import { useEffect, useState } from "preact/hooks";
import { call } from "../api";
import { Icon } from "../components/icons";
import { toast } from "../components/toast";

export function DictionaryPage() {
  const [terms, setTerms] = useState<string[] | null>(null);
  const [draft, setDraft] = useState("");
  useEffect(() => {
    call<{ terms: string[] }>("dictionary").then((d) => setTerms(d.terms));
  }, []);

  const save = (next: string[]) =>
    call<{ terms: string[] }>("setDictionary", { terms: next }).then((d) => setTerms(d.terms));

  const add = (e: Event) => {
    e.preventDefault();
    const word = draft.trim();
    if (!word || !terms) return;
    if (terms.some((t) => t.toLowerCase() === word.toLowerCase())) {
      toast(`“${word}” is already in your dictionary`);
      return;
    }
    save([...terms, word]);
    setDraft("");
  };

  const remove = (word: string) => {
    if (!terms) return;
    const before = terms;
    save(terms.filter((t) => t !== word));
    toast(`Removed “${word}”`, { label: "Undo", run: () => save(before) });
  };

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

      {terms && (terms.length ? (
        <ul class="terms" aria-label="Your words">
          {terms.map((t) => (
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
    </div>
  );
}
