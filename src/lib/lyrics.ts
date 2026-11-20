import type { LyricLine, LyricsDoc } from "./api";

/** Lines light up slightly before the voice — reads as "in time" to the eye. */
export const LEAD_MS = 140;

export function lyricTime(posMs: number, doc: LyricsDoc | null): number {
  return posMs + (doc?.userOffsetMs ?? 0) + LEAD_MS;
}

export function activeIndex(lines: LyricLine[], t: number): number {
  let lo = 0, hi = lines.length - 1, ans = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (lines[mid].t <= t) { ans = mid; lo = mid + 1; } else hi = mid - 1;
  }
  return ans;
}

export interface WordTiming { text: string; t: number; d: number }

/** Word timings for a line: real ones from enhanced LRC, otherwise spread across the line
 *  by character count. Singing usually ends before the next line starts, so we only fill
 *  ~85% of a long gap. */
export function wordsOf(line: LyricLine): WordTiming[] {
  if (line.words?.length) return line.words.map((w) => ({ text: w.text, t: w.t, d: w.d }));
  const parts = line.text.split(/(\s+)/).filter((p) => p.length);
  const dur = Math.min(line.d ?? 3000, Math.max(1200, (line.d ?? 3000) * 0.85), 9000);
  const total = parts.reduce((s, p) => s + (p.trim() ? p.length + 2 : 0), 0) || 1;
  let acc = 0;
  return parts.map((p) => {
    const w = p.trim() ? p.length + 2 : 0;
    const out = { text: p, t: line.t + (acc / total) * dur, d: (w / total) * dur };
    acc += w;
    return out;
  });
}

export const sourceLabel = (doc: LyricsDoc | null) => {
  if (!doc) return "";
  const kind =
    doc.kind === "synced" ? "синхронный" :
    doc.kind === "estimated" ? "≈ авто-синхронизация" :
    doc.kind === "plain" ? "без таймкодов" : "";
  const fix = doc.autoShiftMs || (doc.autoScale && Math.abs(doc.autoScale - 1) > 0.0005)
    ? ` · подогнан ${doc.autoShiftMs > 0 ? "+" : ""}${(doc.autoShiftMs / 1000).toFixed(1)} с` : "";
  return [doc.source, kind].filter(Boolean).join(" · ") + fix;
};
