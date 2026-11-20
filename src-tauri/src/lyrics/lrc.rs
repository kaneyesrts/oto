//! LRC parser: multiple timestamps per line, `[offset:]`, and enhanced word timing `<mm:ss.xx>`.

use super::{LyricLine, Word};
use regex::Regex;
use std::sync::LazyLock;

static TS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[(\d{1,3}):(\d{1,2})(?:[.:](\d{1,3}))?\]").unwrap());
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<(\d{1,3}):(\d{1,2})(?:[.:](\d{1,3}))?>").unwrap());
static OFFSET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\[offset:\s*([+-]?\d+)\s*\]").unwrap());

fn to_ms(m: &str, s: &str, frac: Option<&str>) -> u32 {
    let m: u32 = m.parse().unwrap_or(0);
    let s: u32 = s.parse().unwrap_or(0);
    let f = frac
        .map(|f| match f.len() {
            1 => f.parse::<u32>().unwrap_or(0) * 100,
            2 => f.parse::<u32>().unwrap_or(0) * 10,
            _ => f[..3].parse::<u32>().unwrap_or(0),
        })
        .unwrap_or(0);
    m * 60_000 + s * 1000 + f
}

pub fn looks_synced(text: &str) -> bool {
    text.lines().filter(|l| TS.is_match(l)).take(3).count() >= 3
}

pub fn parse(text: &str) -> Vec<LyricLine> {
    let mut offset: i64 = 0;
    let mut out: Vec<LyricLine> = vec![];
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(c) = OFFSET.captures(line) {
            // Positive offset in LRC means lyrics should appear earlier.
            offset = -c[1].parse::<i64>().unwrap_or(0);
            continue;
        }
        let mut stamps = vec![];
        let mut rest = line;
        while let Some(c) = TS.captures(rest) {
            let m = c.get(0).unwrap();
            if m.start() != 0 {
                break;
            }
            stamps.push(to_ms(&c[1], &c[2], c.get(3).map(|x| x.as_str())));
            rest = &rest[m.end()..];
        }
        if stamps.is_empty() {
            continue;
        }
        let (text, words) = parse_words(rest);
        for t in stamps {
            out.push(LyricLine { t: shift(t, offset), d: None, text: text.clone(), words: words.clone().map(|ws| {
                ws.into_iter().map(|w| Word { t: shift(w.t, offset), ..w }).collect()
            }) });
        }
    }
    out.sort_by_key(|l| l.t);
    fill_durations(&mut out);
    out
}

fn shift(t: u32, by: i64) -> u32 {
    (t as i64 + by).max(0) as u32
}

fn parse_words(s: &str) -> (String, Option<Vec<Word>>) {
    if !WORD.is_match(s) {
        return (s.trim().to_string(), None);
    }
    let mut words = vec![];
    let marks: Vec<_> = WORD.captures_iter(s).collect();
    for (i, c) in marks.iter().enumerate() {
        let m = c.get(0).unwrap();
        let end = marks.get(i + 1).map(|n| n.get(0).unwrap().start()).unwrap_or(s.len());
        let w = &s[m.end()..end];
        if !w.trim().is_empty() {
            words.push(Word { t: to_ms(&c[1], &c[2], c.get(3).map(|x| x.as_str())), d: 0, text: w.to_string() });
        }
    }
    for i in 0..words.len() {
        let next = words.get(i + 1).map(|n| n.t).unwrap_or(words[i].t + 600);
        words[i].d = next.saturating_sub(words[i].t).min(3000);
    }
    let text = words.iter().map(|w| w.text.as_str()).collect::<String>().trim().to_string();
    (text, if words.is_empty() { None } else { Some(words) })
}

pub fn fill_durations(lines: &mut [LyricLine]) {
    for i in 0..lines.len() {
        let next = lines.get(i + 1).map(|n| n.t);
        lines[i].d = Some(match next {
            Some(n) => n.saturating_sub(lines[i].t),
            None => 6000,
        });
    }
}

pub fn plain_lines(text: &str) -> Vec<String> {
    text.lines().map(|l| TS.replace_all(l, "").trim().to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multi_stamps_offset_and_words() {
        let text = "[ar:Someone]\n[offset:+500]\n[00:12.50][01:02.00]Hello there\n[00:20.1]<00:20.10>One <00:20.60>two\n[00:30.00]\n";
        let l = parse(text);
        assert_eq!(l.len(), 4);
        assert_eq!(l[0].t, 12_000); // +500 offset means earlier
        assert_eq!(l[0].text, "Hello there");
        assert_eq!(l[1].words.as_ref().unwrap().len(), 2);
        assert_eq!(l[1].text, "One two");
        assert_eq!(l[2].text, "");
        assert_eq!(l[3].t, 61_500);
        assert_eq!(l[0].d, Some(19_600 - 12_000));
        assert!(looks_synced(text));
    }
}
