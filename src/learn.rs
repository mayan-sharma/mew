//! Turns "what Mew typed" vs "what the text box says now" into spelling fixes worth remembering.
//! Pure logic; `edits.rs` does the watching.

/// A phrase Mew heard and the spelling the user fixed it to ("cooper netties" -> "Kubernetes").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub heard: String,
    pub fixed: String,
}

const MAX_FIXES: usize = 3;
const MAX_PHRASE_WORDS: usize = 3;
/// More changed words than this share of the dictation means a rewrite, not a spelling fix.
const REWRITE_SHARE: f32 = 0.4;

/// Finds spelling fixes the user made to `typed`. `before` is the text box right after Mew typed (it contains
/// `typed`); `after` is the same box later. Returns nothing unless the edit is clearly a few word-level fixes.
pub fn fixes(typed: &str, before: &str, after: &str) -> Vec<Fix> {
    let typed = typed.trim();
    let Some(at) = before.rfind(typed).filter(|_| !typed.is_empty()) else { return Vec::new() };
    let (prefix, suffix) = (&before[..at], &before[at + typed.len()..]);
    if !(after.starts_with(prefix) && after.ends_with(suffix) && after.len() >= prefix.len() + suffix.len()) {
        return Vec::new(); // text around the dictation changed too; can't line it up safely
    }
    let edited = &after[prefix.len()..after.len() - suffix.len()];
    let (old, new) = (words(typed), words(edited));
    if old == new {
        return Vec::new();
    }

    let blocks = changed_blocks(&old, &new);
    let changed: usize = blocks.iter().map(|(o, _)| o.len()).sum();
    if changed as f32 > (old.len() as f32 * REWRITE_SHARE).max(1.0) {
        return Vec::new();
    }
    blocks
        .into_iter()
        .filter(|(o, n)| (1..=MAX_PHRASE_WORDS).contains(&o.len()) && (1..=MAX_PHRASE_WORDS).contains(&n.len()))
        .map(|(o, n)| Fix { heard: o.join(" "), fixed: n.join(" ") })
        .filter(worth_learning)
        .take(MAX_FIXES)
        .collect()
}

/// Words with surrounding punctuation trimmed ("Kubernetes," -> "Kubernetes").
fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-').to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Replacement blocks (old words -> new words) from a longest-common-subsequence alignment.
/// Pure insertions or deletions (adding or dropping words) are not spelling fixes and are skipped.
fn changed_blocks(old: &[String], new: &[String]) -> Vec<(Vec<String>, Vec<String>)> {
    let (n, m) = (old.len(), new.len());
    let mut lcs = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if old[i] == new[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut blocks = Vec::new();
    let (mut o, mut w) = (Vec::new(), Vec::new());
    let mut flush = |o: &mut Vec<String>, w: &mut Vec<String>| {
        if !o.is_empty() && !w.is_empty() {
            blocks.push((std::mem::take(o), std::mem::take(w)));
        }
        o.clear();
        w.clear();
    };
    while i < n || j < m {
        if i < n && j < m && old[i] == new[j] {
            flush(&mut o, &mut w);
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            w.push(new[j].clone());
            j += 1;
        } else {
            o.push(old[i].clone());
            i += 1;
        }
    }
    flush(&mut o, &mut w);
    blocks
}

/// A spelling fix, not a change of mind: the new spelling is a name or jargon (not an everyday word)
/// and either only the capitalization changed or it sounds/looks like what was heard.
fn worth_learning(fix: &Fix) -> bool {
    let special = fix.fixed.split(' ').any(|w| w.chars().count() >= 2 && !is_common(w));
    if !special || fix.heard == fix.fixed {
        return false;
    }
    let (a, b) = (squash(&fix.heard), squash(&fix.fixed));
    a == b || similarity(&a, &b) >= 0.4
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// 1 - normalized Levenshtein distance.
fn similarity(a: &str, b: &str) -> f32 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    1.0 - row[b.len()] as f32 / a.len().max(b.len()) as f32
}

fn is_common(word: &str) -> bool {
    let w = word.to_lowercase();
    COMMON.split_ascii_whitespace().any(|c| c == w)
}

/// Replaces learned phrases in new dictations: whole words only, ignoring case ("Cooper Netties" too).
pub fn apply(text: &str, fixes: &[Fix]) -> String {
    let mut out = text.to_string();
    for fix in fixes {
        let heard: Vec<String> = fix.heard.split(' ').map(str::to_lowercase).collect();
        let tokens: Vec<&str> = out.split(' ').collect();
        let mut rebuilt: Vec<String> = Vec::with_capacity(tokens.len());
        let mut i = 0;
        while i < tokens.len() {
            let window = tokens.get(i..i + heard.len());
            let matches = window.is_some_and(|w| {
                w.iter().zip(&heard).enumerate().all(|(k, (tok, h))| {
                    let core = tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-');
                    // only the last word may carry trailing punctuation; the first may carry leading
                    let leading_ok = k == 0 || !tok.starts_with(|c: char| !c.is_alphanumeric());
                    let trailing_ok = k + 1 == heard.len() || !tok.ends_with(|c: char| !c.is_alphanumeric());
                    core.to_lowercase() == *h && leading_ok && trailing_ok
                })
            });
            if matches {
                let first = tokens[i];
                let last = tokens[i + heard.len() - 1];
                let lead: String = first.chars().take_while(|c| !c.is_alphanumeric()).collect();
                let core_end =
                    last.char_indices().rev().find(|(_, c)| c.is_alphanumeric()).map_or(0, |(p, c)| p + c.len_utf8());
                let trail = &last[core_end..];
                rebuilt.push(format!("{lead}{}{trail}", fix.fixed));
                i += heard.len();
            } else {
                rebuilt.push(tokens[i].to_string());
                i += 1;
            }
        }
        out = rebuilt.join(" ");
    }
    out
}

/// Everyday English words: replacing one of these is a change of mind, not a spelling fix.
const COMMON: &str = "a about above after again against all almost also always am an and another any anyone anything \
are around as ask at away back bad be because been before being below best better between big bit both bring \
but buy by call came can cannot can't change check close come could couldn't day days did didn't different do \
does doesn't doing done don't down during each early easy either else end enough even ever every everyone \
everything example fast feel few find fine first fix for found free friday from full get gets getting give go \
goes going gone good got great had hadn't half happen hard has hasn't have haven't having he hear help her here \
hers herself hey hi high him himself his hold home hope hour hours how however i i'd i'll i'm i've idea if in \
inside into is isn't it it's its itself just keep kind know last late later least leave left less let let's \
life like line little long look looks lot love made make makes making many may maybe me mean meet meeting might \
mind minute minutes monday month more morning most move much must my myself need needs never new next nice night \
no none nor not note nothing now of off often oh ok okay old on once one only open or other others our ours \
ourselves out over own part people place plan please point pretty probably problem put quick quite rather \
read ready real really right run said same saturday saw say says see seem seems send sent set she she's should \
shouldn't show since small so some someone something soon sorry start still stop such sunday sure take talk \
team tell than thank thanks that that's the their theirs them themselves then there there's these they \
they're thing things think this those though thought through thursday time times to today together tomorrow \
too took top try trying tuesday turn two under until up update us use used very via wait want wanted was wasn't \
way we we'll we're we've wednesday week weeks well went were weren't what what's when where which while who \
whole why will with within without won't work works would wouldn't write wrong yeah year years yes yesterday \
yet you you'd you'll you're you've your yours yourself";

#[cfg(test)]
mod tests {
    use super::*;

    fn fix(h: &str, f: &str) -> Fix {
        Fix { heard: h.into(), fixed: f.into() }
    }

    #[test]
    fn learns_a_name_fix_in_the_middle_of_a_box() {
        let typed = "Can you deploy it to cooper netties today?";
        let before = format!("Hi team. {typed} Thanks");
        let after = "Hi team. Can you deploy it to Kubernetes today? Thanks";
        assert_eq!(fixes(typed, &before, after), vec![fix("cooper netties", "Kubernetes")]);
    }

    #[test]
    fn learns_capitalization_of_a_product_name() {
        let typed = "the catalyst build is green";
        assert_eq!(fixes(typed, typed, "the Catalyst build is green"), vec![fix("catalyst", "Catalyst")]);
    }

    #[test]
    fn ignores_changes_of_mind_and_rewrites() {
        let typed = "let's deploy it on Tuesday";
        assert!(fixes(typed, typed, "let's ship it on Tuesday").is_empty(), "common word swap");
        assert!(fixes(typed, typed, "we can talk about this later").is_empty(), "rewrite");
        assert!(fixes(typed, typed, "let's deploy it on Tuesday morning").is_empty(), "pure insertion");
        assert!(fixes(typed, "something else entirely", typed).is_empty(), "can't anchor");
    }

    #[test]
    fn applies_learned_fixes_with_punctuation_and_case() {
        let fixes = [fix("cooper netties", "Kubernetes"), fix("catalyst", "Catalyst")];
        assert_eq!(apply("Cooper Netties, then catalyst.", &fixes), "Kubernetes, then Catalyst.");
        assert_eq!(apply("cooper nettiesish stays", &fixes), "cooper nettiesish stays");
    }
}
