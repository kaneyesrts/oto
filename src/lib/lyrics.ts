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

const normLine = (s: string) => s.toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");

/** Which lines are the chorus (text that repeats) and which are the finale (the song's
 *  last few sung lines). Used for optional highlighting. */
export function lineRoles(lines: LyricLine[]): { chorus: boolean[]; finale: boolean[] } {
  const keys = lines.map((l) => normLine(l.text));
  const count = new Map<string, number>();
  for (const k of keys) if (k.length >= 6) count.set(k, (count.get(k) ?? 0) + 1);
  const chorus = keys.map((k) => (count.get(k) ?? 0) >= 2);
  const sung = lines.map((l, i) => (l.text.trim() ? i : -1)).filter((i) => i >= 0);
  const n = Math.max(2, Math.min(6, Math.ceil(sung.length * 0.12)));
  const fin = new Set(sung.slice(-n));
  return { chorus, finale: lines.map((_, i) => fin.has(i)) };
}
