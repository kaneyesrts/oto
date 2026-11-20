import { invoke, convertFileSrc } from "@tauri-apps/api/core";

export type RGB = [number, number, number];

export interface Track {
  id: string;
  path: string;
  title: string;
  artist: string;
  album: string;
  albumArtist: string;
  durationMs: number;
  trackNo?: number | null;
  year?: number | null;
  genre?: string | null;
  cover?: string | null;
  palette: RGB[];
  liked: boolean;
  addedAt: number;
  source: string;
}

export interface PlayerState {
  track: Track | null;
  playing: boolean;
  posMs: number;
  durationMs: number;
  volume: number;
  shuffle: boolean;
  repeat: "off" | "all" | "one";
  queue: string[];
  index: number;
  at: number;
}

export interface SpectrumFrame {
  b: number[];
  lvl: number;
  bass: number;
  mid: number;
  high: number;
  beat: number;
  pos: number;
}

export interface Word { t: number; d: number; text: string }
export interface LyricLine { t: number; d?: number | null; text: string; words?: Word[] }
export interface LyricsDoc {
  kind: "synced" | "estimated" | "plain" | "instrumental" | "none";
  source: string;
  lines: LyricLine[];
  autoShiftMs: number;
  autoScale: number;
  userOffsetMs: number;
}

export interface WidgetSettings {
  enabled: boolean;
  layer: "desktop" | "overlay";
  style: "compact" | "lyrics" | "spectrum";
  opacity: number;
  blur: number;
  scale: number;
  clickThrough: boolean;
  glow: boolean;
  particles: boolean;
  spectrum: boolean;
  x?: number | null;
  y?: number | null;
}

export interface Hotkeys {
  playPause: string;
  next: string;
  prev: string;
  volumeUp: string;
  volumeDown: string;
  like: string;
  toggleWidget: string;
  toggleClickThrough: string;
  seekForward: string;
  seekBack: string;
}

export type ThemeId = "aura" | "noir" | "mist" | "liquid" | "paper";

export interface Settings {
  theme: ThemeId;
  accentFromCover: boolean;
  reducedMotion: boolean;
  closeToTray: boolean;
  folders: string[];
  volume: number;
  lyrics: { fontScale: number; depthBlur: boolean; autoAlign: boolean; online: boolean };
  visual: { particles: boolean; intensity: number; quality: "low" | "high" };
  widget: WidgetSettings;
  hotkeys: Hotkeys;
}

export interface Stats {
  rangeMs: number;
  todayMs: number;
  plays: number;
  skips: number;
  uniqueTracks: number;
  streakDays: number;
  topTracks: { track: Track; plays: number; ms: number }[];
  topArtists: { artist: string; plays: number; ms: number; cover?: string | null }[];
  heat: number[][];
  daily: { day: string; ms: number }[];
  rising?: { artist: string; plays: number; ms: number; cover?: string | null } | null;
}

export type PlayerReq =
  | { type: "playList"; ids: string[]; start: number }
  | { type: "enqueue"; ids: string[] }
  | { type: "playNext"; ids: string[] }
  | { type: "jumpTo"; index: number }
  | { type: "toggle" | "play" | "pause" | "next" | "prev" }
  | { type: "seek"; ms: number }
  | { type: "seekBy"; ms: number }
  | { type: "volume"; value: number }
  | { type: "shuffle"; on: boolean }
  | { type: "repeat"; mode: string };

export const api = {
  settings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<string[]>("set_settings", { settings }),
  tracks: () => invoke<Track[]>("get_tracks"),
  addFolder: (path: string) => invoke("add_folder", { path }),
  removeFolder: (path: string) => invoke("remove_folder", { path }),
  rescan: () => invoke("rescan"),
  toggleLike: (id: string) => invoke<boolean>("toggle_like", { id }),
  player: (req: PlayerReq) => invoke("player", { req }),
  playerState: () => invoke<PlayerState>("player_state"),
  lyrics: (id: string, force = false) => invoke<LyricsDoc | null>("get_lyrics", { id, force }),
  setLyricsOffset: (id: string, ms: number) => invoke("set_lyrics_offset", { id, ms }),
  stats: (range: string) => invoke<Stats>("get_stats", { range }),
  showMain: () => invoke("show_main"),
  hideFlyout: () => invoke("hide_flyout"),
  toggleWidget: () => invoke("toggle_widget"),
};

const coverCache = new Map<string, string>();
export function coverUrl(path?: string | null): string | null {
  if (!path) return null;
  if (path.startsWith("data:")) return path; // dev mock covers
  let u = coverCache.get(path);
  if (!u) {
    u = convertFileSrc(path);
    coverCache.set(path, u);
  }
  return u;
}

export function fmtTime(ms: number): string {
  if (!isFinite(ms) || ms < 0) ms = 0;
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  const h = Math.floor(m / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m % 60).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

export function fmtDuration(ms: number): string {
  const min = Math.round(ms / 60000);
  if (min < 60) return `${min} мин`;
  const h = Math.floor(min / 60);
  return `${h} ч ${min % 60} мин`;
}

export function plural(n: number, one: string, few: string, many: string) {
  const m10 = n % 10, m100 = n % 100;
  if (m10 === 1 && m100 !== 11) return one;
  if (m10 >= 2 && m10 <= 4 && (m100 < 10 || m100 >= 20)) return few;
  return many;
}
