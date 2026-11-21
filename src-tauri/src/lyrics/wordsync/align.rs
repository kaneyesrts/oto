//! Word-level alignment of lyrics text to recognised speech.
//!
//! The recogniser hears a noisy version of the lyrics: words get misheard, skipped (backing
//! vocals, ad-libs), invented (hallucinations over instrumentals) or merged/split. We align the
//! two word sequences globally (Needleman–Wunsch with 1:2 / 2:1 merges) on a fuzzy similarity,
//! take times from confident matches, and interpolate the rest between them.

use super::super::{LyricLine, Word};

/// A word heard by the recogniser, times in ms.
#[derive(Debug, Clone)]
pub struct RecWord {
    pub t0: u32,
    pub t1: u32,
    pub text: String,
}

/// Prior timing for a line (from synced or estimated lyrics), used only to break ties
/// between equally good alignments, e.g. which chorus repeat a heard chorus belongs to.
#[derive(Debug, Clone, Copy)]
pub struct Prior {
    /// Expected start of the line, ms.
    pub t: u32,
    /// Expected duration, ms.
    pub d: u32,
    /// How far off the prior may plausibly be before matches start costing, ms.
    pub slack: u32,
}

#[derive(Debug, Clone)]
pub struct AlignResult {
    pub lines: Vec<LyricLine>,
    /// Lyric words that took their time from a confident match.
    pub matched: usize,
    pub total: usize,
    /// Line of the last lyric word matched with confidence.
    pub last_anchor_line: Option<usize>,
}

impl AlignResult {
    pub fn match_rate(&self) -> f32 {
        if self.total == 0 { 0.0 } else { self.matched as f32 / self.total as f32 }
    }
}

/// Lowercase, letters and digits only, ё → е.
pub fn normalize(s: &str) -> String {
    s.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| if c == 'ё' { 'е' } else { c })
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// 1 = identical, 0 = nothing in common.
pub fn similarity(a: &[char], b: &[char]) -> f32 {
    let n = a.len().max(b.len());
    if n == 0 {
        return 0.0;
    }
    1.0 - levenshtein(a, b) as f32 / n as f32
}

/// Splits a line into display words. Each word keeps its trailing whitespace, so joining the
/// texts gives the line back; bits with no letters ("—", "...") stick to a neighbour.
pub fn split_words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut pending = String::new(); // leading punctuation waiting for a word
    for chunk in text.split_inclusive(char::is_whitespace) {
        if normalize(chunk).is_empty() {
            match out.last_mut() {
                Some(last) => last.push_str(chunk),
                None => pending.push_str(chunk),
            }
        } else {
            out.push(std::mem::take(&mut pending) + chunk);
        }
    }
    if !pending.is_empty() {
        match out.last_mut() {
            Some(last) => last.push_str(&pending),
            None if !pending.trim().is_empty() => out.push(pending),
            None => {}
        }
    }
    if let Some(last) = out.last_mut() {
        let t = last.trim_end().len();
        last.truncate(t);
    }
    out
}

struct LWord {
    line: usize,
    text: String,
    key: Vec<char>,
    /// Expected time from the prior, if any.
    expect: Option<(u32, u32)>, // (time, slack)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Move {
    None,
    Diag,
    SkipL,
    SkipR,
    /// One lyric word ↔ two recognised words.
    OneTwo,
    /// Two lyric words ↔ one recognised word.
    TwoOne,
}

const GAP_L: f32 = -0.35; // lyric word not heard
const GAP_R: f32 = -0.25; // heard something that is not in the lyrics
const MERGE: f32 = -0.15; // extra cost of a 1:2 / 2:1 pairing
/// A match below this similarity is not trusted for timing.
pub const ANCHOR_SIM: f32 = 0.6;
/// Longest a single sung word is allowed to last on screen.
const MAX_WORD_MS: u32 = 3000;

fn pair_score(sim: f32) -> f32 {
    2.0 * sim - 1.0
}

/// Penalty for matching a word far from where the prior expects it.
fn prior_penalty(expect: Option<(u32, u32)>, t: u32) -> f32 {
    match expect {
        Some((e, slack)) => {
            let dt = (t as f32 - e as f32).abs();
            let over = (dt - slack as f32).max(0.0);
            (over / (2.0 * slack as f32).max(1.0)).min(1.0) * 0.8
        }
        None => 0.0,
    }
}

fn concat(a: &[char], b: &[char]) -> Vec<char> {
    a.iter().chain(b.iter()).cloned().collect()
}

/// Natural on-screen length of a word that we have no time for.
fn natural_ms(w: &str) -> u32 {
    (90 + 65 * normalize(w).chars().count() as u32).clamp(180, 900)
}

/// Aligns lyrics `lines` to recognised `rec` words. `priors[i]` is the prior for line `i`
/// (or None). Returns re-timed lines with per-word timings.
pub fn align(lines: &[String], priors: &[Option<Prior>], rec: &[RecWord]) -> AlignResult {
    // Lyric words.
    let mut lw: Vec<LWord> = vec![];
    for (li, text) in lines.iter().enumerate() {
        let words = split_words(text);
        let total_chars: usize = words.iter().map(|w| w.chars().count().max(1)).sum::<usize>().max(1);
        let mut acc = 0usize;
        for w in words {
            let expect = priors.get(li).copied().flatten().map(|p| {
                let frac = acc as f32 / total_chars as f32;
                (p.t + (p.d as f32 * frac) as u32, p.slack)
            });
            acc += w.chars().count().max(1);
            lw.push(LWord { line: li, key: normalize(&w).chars().collect(), text: w, expect });
        }
    }
    let rec: Vec<(&RecWord, Vec<char>)> =
        rec.iter().map(|r| (r, normalize(&r.text).chars().collect::<Vec<char>>())).filter(|(_, k)| !k.is_empty()).collect();
    let (n, m) = (lw.len(), rec.len());

    // DP over (i lyric words consumed, j recognised words consumed).
    let w = m + 1;
    let mut score = vec![f32::NEG_INFINITY; (n + 1) * w];
    let mut back = vec![Move::None; (n + 1) * w];
    score[0] = 0.0;
    for i in 0..=n {
        for j in 0..=m {
            let here = i * w + j;
            if i == 0 && j == 0 {
                continue;
            }
            let mut best = (f32::NEG_INFINITY, Move::None);
            let mut consider = |s: f32, mv: Move| {
                if s > best.0 {
                    best = (s, mv);
                }
            };
            if i > 0 && j > 0 {
                let l = &lw[i - 1];
                let r = &rec[j - 1];
                let s = pair_score(similarity(&l.key, &r.1)) - prior_penalty(l.expect, r.0.t0);
                consider(score[(i - 1) * w + j - 1] + s, Move::Diag);
            }
            if i > 0 {
                consider(score[(i - 1) * w + j] + GAP_L, Move::SkipL);
            }
            if j > 0 {
                consider(score[i * w + j - 1] + GAP_R, Move::SkipR);
            }
            if i > 0 && j > 1 {
                let l = &lw[i - 1];
                let k = concat(&rec[j - 2].1, &rec[j - 1].1);
                let s = pair_score(similarity(&l.key, &k)) + MERGE - prior_penalty(l.expect, rec[j - 2].0.t0);
                consider(score[(i - 1) * w + j - 2] + s, Move::OneTwo);
            }
            if i > 1 && j > 0 && lw[i - 2].line == lw[i - 1].line {
                let k = concat(&lw[i - 2].key, &lw[i - 1].key);
                let s = pair_score(similarity(&k, &rec[j - 1].1)) + MERGE - prior_penalty(lw[i - 2].expect, rec[j - 1].0.t0);
                consider(score[(i - 2) * w + j - 1] + s, Move::TwoOne);
            }
            score[here] = best.0;
            back[here] = best.1;
        }
    }

    // Trace back into per-lyric-word times (start, end) from confident matches.
    let mut times: Vec<Option<(u32, u32)>> = vec![None; n];
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        match back[i * w + j] {
            Move::Diag => {
                let sim = similarity(&lw[i - 1].key, &rec[j - 1].1);
                if sim >= ANCHOR_SIM {
                    times[i - 1] = Some((rec[j - 1].0.t0, rec[j - 1].0.t1));
                }
                i -= 1;
                j -= 1;
            }
            Move::OneTwo => {
                let k = concat(&rec[j - 2].1, &rec[j - 1].1);
                if similarity(&lw[i - 1].key, &k) >= ANCHOR_SIM {
                    times[i - 1] = Some((rec[j - 2].0.t0, rec[j - 1].0.t1));
                }
                i -= 1;
                j -= 2;
            }
            Move::TwoOne => {
                let k = concat(&lw[i - 2].key, &lw[i - 1].key);
                if similarity(&k, &rec[j - 1].1) >= ANCHOR_SIM {
                    // Split the heard word between the two lyric words by length.
                    let r = rec[j - 1].0;
                    let (a, b) = (lw[i - 2].key.len().max(1) as f32, lw[i - 1].key.len().max(1) as f32);
                    let mid = r.t0 + ((r.t1.saturating_sub(r.t0)) as f32 * a / (a + b)) as u32;
                    times[i - 2] = Some((r.t0, mid));
                    times[i - 1] = Some((mid, r.t1));
                }
                i -= 2;
                j -= 1;
            }
            Move::SkipL => i -= 1,
            Move::SkipR => j -= 1,
            Move::None => break,
        }
    }

    // Keep the anchors monotonic: drop the ones outside the longest non-decreasing run.
    keep_monotonic(&mut times);
    let matched = times.iter().filter(|t| t.is_some()).count();
    let last_anchor_line = (0..n).rev().find(|&k| times[k].is_some()).map(|k| lw[k].line);

    // Fill in the rest between anchors.
    let filled = fill_gaps(&lw, &times);

    // Build the lines.
    let mut out: Vec<LyricLine> = lines.iter().map(|t| LyricLine { t: 0, d: None, text: t.clone(), words: None }).collect();
    let mut per_line: Vec<Vec<Word>> = vec![vec![]; lines.len()];
    for (k, l) in lw.iter().enumerate() {
        let (t, e) = filled[k];
        per_line[l.line].push(Word { t, d: e.saturating_sub(t).max(1), text: l.text.clone() });
    }
    for (li, ws) in per_line.into_iter().enumerate() {
        if ws.is_empty() {
            continue;
        }
        let start = ws[0].t;
        let end = ws.iter().map(|w| w.t + w.d).max().unwrap_or(start);
        out[li].t = start;
        out[li].d = Some(end.saturating_sub(start).max(1));
        // Spaces between words are part of the word texts.
        out[li].words = Some(ws);
    }
    place_empty_lines(&mut out);
    AlignResult { lines: out, matched, total: n, last_anchor_line }
}

fn keep_monotonic(times: &mut [Option<(u32, u32)>]) {
    let idx: Vec<usize> = (0..times.len()).filter(|&k| times[k].is_some()).collect();
    if idx.is_empty() {
        return;
    }
    let val: Vec<u32> = idx.iter().map(|&k| times[k].unwrap().0).collect();
    // Patience sorting for the longest non-decreasing subsequence.
    let mut tails: Vec<usize> = vec![]; // positions into idx
    let mut prev: Vec<Option<usize>> = vec![None; idx.len()];
    for p in 0..idx.len() {
        let pos = tails.partition_point(|&q| val[q] <= val[p]);
        if pos > 0 {
            prev[p] = Some(tails[pos - 1]);
        }
        if pos == tails.len() {
            tails.push(p);
        } else {
            tails[pos] = p;
        }
    }
    let mut keep = vec![false; idx.len()];
    let mut cur = tails.last().copied();
    while let Some(p) = cur {
        keep[p] = true;
        cur = prev[p];
    }
    for (p, &k) in idx.iter().enumerate() {
        if !keep[p] {
            times[k] = None;
        }
    }
}

/// Every lyric word gets (start, end). Anchors keep their times (ends capped so words do not
/// overlap or linger); runs between anchors are laid out at a natural pace next to the
/// anchor of the same line, and whole unheard lines are spread through the remaining gap.
fn fill_gaps(lw: &[LWord], times: &[Option<(u32, u32)>]) -> Vec<(u32, u32)> {
    let n = lw.len();
    let mut out: Vec<(u32, u32)> = vec![(0, 0); n];
    let anchors: Vec<usize> = (0..n).filter(|&k| times[k].is_some()).collect();
    if anchors.is_empty() {
        return out;
    }
    for (a, &k) in anchors.iter().enumerate() {
        let (t0, t1) = times[k].unwrap();
        let next_start = anchors.get(a + 1).map(|&q| times[q].unwrap().0).unwrap_or(u32::MAX);
        let mut end = t1.max(t0 + 80).min(t0 + MAX_WORD_MS);
        if next_start > t0 {
            end = end.min(next_start);
        }
        out[k] = (t0, end.max(t0 + 1));
    }

    // Runs of unanchored words: before the first anchor, between anchors, after the last.
    let mut bounds: Vec<(Option<usize>, Option<usize>)> = vec![(None, Some(anchors[0]))];
    for p in anchors.windows(2) {
        bounds.push((Some(p[0]), Some(p[1])));
    }
    bounds.push((Some(*anchors.last().unwrap()), None));

    for (prev, next) in bounds {
        let lo = prev.map(|p| p + 1).unwrap_or(0);
        let hi = next.unwrap_or(n);
        if lo >= hi {
            continue;
        }
        let run: Vec<usize> = (lo..hi).collect();
        let nat: Vec<u32> = run.iter().map(|&k| natural_ms(&lw[k].text)).collect();
        let gap_start = prev.map(|p| out[p].1).unwrap_or(0);
        let gap_end = next.map(|q| out[q].0).unwrap_or(u32::MAX);

        let total: u32 = nat.iter().sum();
        let gap = gap_end.saturating_sub(gap_start);
        if prev.is_some() && next.is_some() && gap <= total + total / 2 {
            // Tight: spread the run across the whole gap.
            let mut acc = 0u64;
            for (x, &k) in run.iter().enumerate() {
                let s = gap_start + ((acc * gap as u64) / total.max(1) as u64) as u32;
                acc += nat[x] as u64;
                let e = gap_start + ((acc * gap as u64) / total.max(1) as u64) as u32;
                out[k] = (s, e.max(s + 1));
            }
            continue;
        }
        // Loose: words of the previous anchor's line follow it, words of the next anchor's
        // line lead into it, whole lines in between share the middle.
        let prev_line = prev.map(|p| lw[p].line);
        let next_line = next.map(|q| lw[q].line);
        let mut head = 0; // count following prev in its line
        while head < run.len() && Some(lw[run[head]].line) == prev_line {
            head += 1;
        }
        let mut tail = run.len(); // index where next's line starts
        while tail > head && Some(lw[run[tail - 1]].line) == next_line {
            tail -= 1;
        }
        let mut t = gap_start;
        for x in 0..head {
            out[run[x]] = (t, t + nat[x]);
            t += nat[x];
        }
        let mid_start = t;
        let tail_total: u32 = nat[tail..].iter().sum();
        let mut te = if next.is_some() { gap_end.saturating_sub(tail_total).max(mid_start) } else { mid_start };
        let mid_end = te;
        // Only before the very first anchor can the room be shorter than the words.
        let tail_squeeze = if next.is_some() && tail_total > 0 { (gap_end.saturating_sub(te) as f32 / tail_total as f32).min(1.0) } else { 1.0 };
        for x in tail..run.len() {
            let d = ((nat[x] as f32 * tail_squeeze) as u32).max(1);
            out[run[x]] = (te, te + d);
            te += d;
        }
        // Middle: whole lines nobody heard.
        let mid: Vec<usize> = (head..tail).collect();
        if mid.is_empty() {
            continue;
        }
        let mid_total: u32 = mid.iter().map(|&x| nat[x]).sum::<u32>().max(1);
        let room = mid_end.saturating_sub(mid_start);
        if next.is_none() || prev.is_none() || room <= mid_total {
            // Before the first anchor: end right at it. After the last: natural pace.
            // Between anchors with little room: squeeze into it.
            let (mut t, squeeze) = match (prev, next) {
                (None, Some(_)) if mid_total <= mid_end => (mid_end - mid_total, 1.0),
                (None, Some(_)) => (0, mid_end as f32 / mid_total as f32),
                (_, None) => (mid_start, 1.0),
                _ => (mid_start, room as f32 / mid_total as f32),
            };
            for &x in &mid {
                let d = ((nat[x] as f32) * squeeze).max(1.0) as u32;
                out[run[x]] = (t, t + d);
                t += d;
            }
        } else {
            // Spread the unheard lines evenly through the gap, each at a natural pace.
            let mut groups: Vec<Vec<usize>> = vec![];
            for &x in &mid {
                match groups.last_mut() {
                    Some(g) if lw[run[g[0]]].line == lw[run[x]].line => g.push(x),
                    _ => groups.push(vec![x]),
                }
            }
            let slot = room / groups.len() as u32;
            for (gi, g) in groups.iter().enumerate() {
                let g_total: u32 = g.iter().map(|&x| nat[x]).sum();
                let squeeze = if g_total > slot { slot as f32 / g_total as f32 } else { 1.0 };
                let mut t = mid_start + gi as u32 * slot + slot.saturating_sub(g_total) / 2;
                for &x in g {
                    let d = ((nat[x] as f32) * squeeze).max(1.0) as u32;
                    out[run[x]] = (t, t + d);
                    t += d;
                }
            }
        }
    }
    out
}

/// Empty lines (instrumental breaks) blank the screen shortly after the previous line ends.
fn place_empty_lines(lines: &mut [LyricLine]) {
    for i in 0..lines.len() {
        if lines[i].words.is_some() {
            continue;
        }
        let prev_end = lines[..i].iter().rev().find(|l| l.words.is_some()).map(|l| l.t + l.d.unwrap_or(0));
        let next_start = lines[i + 1..].iter().find(|l| l.words.is_some()).map(|l| l.t);
        let t = match (prev_end, next_start) {
            (Some(p), Some(n)) if n > p + 400 => p + 300.min((n - p) / 2),
            (Some(p), Some(n)) => p + (n.saturating_sub(p)) / 2,
            (Some(p), None) => p + 300,
            (None, Some(_)) => 0,
            (None, None) => 0,
        };
        lines[i].t = t;
        lines[i].d = next_start.map(|n| n.saturating_sub(t).max(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(words: &[(&str, u32)]) -> Vec<RecWord> {
        words.iter().map(|(w, t)| RecWord { t0: *t, t1: *t + 300, text: w.to_string() }).collect()
    }

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn word_t(r: &AlignResult, line: usize, word: usize) -> u32 {
        r.lines[line].words.as_ref().unwrap()[word].t
    }

    #[test]
    fn normalizes_and_compares() {
        assert_eq!(normalize("Ёлка, ЁЖ!"), "елкаеж");
        assert_eq!(normalize("Don't—"), "dont");
        let a: Vec<char> = "привет".chars().collect();
        let b: Vec<char> = "привед".chars().collect();
        assert!(similarity(&a, &b) > 0.8);
        assert_eq!(similarity(&a, &[]), 0.0);
    }

    #[test]
    fn splits_words_keeping_spaces_and_punctuation() {
        assert_eq!(split_words("Hello, world — again"), vec!["Hello, ", "world — ", "again"]);
        assert_eq!(split_words("— so it goes"), vec!["— so ", "it ", "goes"]);
        assert_eq!(split_words("   "), Vec::<String>::new());
        assert_eq!(split_words("Наконец-то  да"), vec!["Наконец-то  ", "да"]);
    }

    #[test]
    fn exact_transcript_gives_exact_times() {
        let l = lines(&["one two three", "four five"]);
        let r = rec(&[("one", 1000), ("two", 1500), ("three", 2000), ("four", 5000), ("five", 5600)]);
        let a = align(&l, &[None, None], &r);
        assert_eq!(a.matched, 5);
        assert_eq!(a.lines[0].t, 1000);
        assert_eq!(word_t(&a, 0, 2), 2000);
        assert_eq!(a.lines[1].t, 5000);
        assert_eq!(a.lines[0].d, Some(1300)); // until the end of "three"
        // Word texts rebuild the line.
        let joined: String = a.lines[0].words.as_ref().unwrap().iter().map(|w| w.text.as_str()).collect();
        assert_eq!(joined, "one two three");
    }

    #[test]
    fn tolerates_misrecognition_extra_and_missing_words() {
        let l = lines(&["Я иду по улице ночной", "И фонари горят"]);
        // "улице" misheard, "ночной" missed, junk inserted, "фонари" split in two.
        let r = rec(&[
            ("я", 1000),
            ("иду", 1300),
            ("по", 1700),
            ("улицы", 1900),
            ("эээ", 3000),
            ("и", 6000),
            ("фона", 6300),
            ("ри", 6500),
            ("горят", 7000),
        ]);
        let a = align(&l, &[None, None], &r);
        assert_eq!(word_t(&a, 0, 3), 1900, "misheard word still matches");
        let noch = word_t(&a, 0, 4);
        assert!(noch > 1900 && noch < 6000, "missing word interpolated: {noch}");
        assert_eq!(a.lines[1].t, 6000);
        assert_eq!(word_t(&a, 1, 1), 6300, "split word matched as one");
        assert_eq!(word_t(&a, 1, 2), 7000);
        assert!(a.match_rate() > 0.8);
    }

    #[test]
    fn repeated_chorus_uses_the_prior_to_pick_the_right_repeat() {
        let l = lines(&["verse words here", "la la chorus", "second verse now", "la la chorus"]);
        // Only the second chorus was heard; the prior says it is near 30 s.
        let r = rec(&[("verse", 1000), ("words", 1400), ("here", 1800), ("la", 30_000), ("la", 30_400), ("chorus", 30_800)]);
        let pri = [
            Some(Prior { t: 1000, d: 2000, slack: 3000 }),
            Some(Prior { t: 10_000, d: 2000, slack: 3000 }),
            Some(Prior { t: 20_000, d: 2000, slack: 3000 }),
            Some(Prior { t: 30_000, d: 2000, slack: 3000 }),
        ];
        let a = align(&l, &pri, &r);
        assert_eq!(a.lines[3].t, 30_000);
        assert!(a.lines[1].t < 30_000);
        // Lines stay in order.
        for w in a.lines.windows(2) {
            assert!(w[0].t <= w[1].t, "{} > {}", w[0].t, w[1].t);
        }
    }

    #[test]
    fn whole_unheard_lines_spread_through_gap() {
        let l = lines(&["start here", "nobody heard this", "nor this one", "end now"]);
        let r = rec(&[("start", 1000), ("here", 1400), ("end", 21_000), ("now", 21_400)]);
        let a = align(&l, &[None; 4], &r);
        let (t1, t2) = (a.lines[1].t, a.lines[2].t);
        assert!(t1 > 1700 && t1 < 11_000, "{t1}");
        assert!(t2 > t1 && t2 < 21_000, "{t2}");
        assert_eq!(a.matched, 4);
    }

    #[test]
    fn non_monotonic_matches_are_dropped() {
        let l = lines(&["alpha beta gamma delta"]);
        // "gamma" was timestamped before "beta" (recogniser glitch).
        let r = rec(&[("alpha", 1000), ("beta", 2000), ("gamma", 500), ("delta", 3000)]);
        let a = align(&l, &[None], &r);
        let ws = a.lines[0].words.as_ref().unwrap();
        for p in ws.windows(2) {
            assert!(p[0].t <= p[1].t);
        }
        assert_eq!(a.matched, 3);
    }

    #[test]
    fn garbage_gives_low_match_rate() {
        let l = lines(&["the quick brown fox", "jumps over the lazy dog"]);
        let r = rec(&[("music", 1000), ("playing", 2000), ("subtitles", 3000), ("by", 4000)]);
        let a = align(&l, &[None, None], &r);
        assert!(a.match_rate() < 0.35, "{}", a.match_rate());
    }

    #[test]
    fn empty_lines_blank_after_previous_line() {
        let l = lines(&["one two", "", "three four"]);
        let r = rec(&[("one", 1000), ("two", 1400), ("three", 20_000), ("four", 20_400)]);
        let a = align(&l, &[None; 3], &r);
        assert!(a.lines[1].words.is_none());
        assert!(a.lines[1].t >= 1700 && a.lines[1].t < 3000, "{}", a.lines[1].t);
        assert!(a.lines[2].t == 20_000);
    }

    #[test]
    fn word_durations_are_capped() {
        let l = lines(&["hold", "next"]);
        let r = vec![RecWord { t0: 1000, t1: 15_000, text: "hold".into() }, RecWord { t0: 16_000, t1: 16_500, text: "next".into() }];
        let a = align(&l, &[None; 2], &r);
        assert!(a.lines[0].words.as_ref().unwrap()[0].d <= MAX_WORD_MS);
    }
}
