import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, type LyricsDoc, type PlayerState, type Playlist, type Settings, type SpectrumFrame, type Track } from "./api";
import { applyPalette } from "./color";

export type View = "home" | "library" | "albums" | "artists" | "liked" | "stats" | "settings" | "playlist";

const emptyPlayer: PlayerState = {
  track: null, playing: false, posMs: 0, durationMs: 0, volume: 0.8,
  shuffle: false, repeat: "off", queue: [], index: 0, at: Date.now(),
};

export const app = $state({
  ready: false,
  settings: null as Settings | null,
  tracks: [] as Track[],
  player: emptyPlayer as PlayerState,
  view: "home" as View,
  search: "",
  lyricsOpen: false,
  queueOpen: false,
  scan: null as null | { done: number; total: number; finished: boolean },
  lyrics: null as LyricsDoc | null,
  lyricsLoading: false,
  toast: null as null | { text: string; at: number },
  /** Album or artist opened from a grid. */
  focus: null as null | { kind: "album" | "artist" | "playlist"; key: string },
  playlists: [] as Playlist[],
  /** Context menu for one or more tracks, opened by right click or the ⋯ button. */
  menu: null as null | { x: number; y: number; ids: string[]; playlistId?: number; index?: number },
  fullscreen: false,
});

/** Written 60×/s by the analyser; read by canvases in their own rAF loops. Not reactive on purpose. */
export const frame: SpectrumFrame & { t: number } = {
  b: new Array(48).fill(0), lvl: 0, bass: 0, mid: 0, high: 0, beat: 0, pos: 0, t: 0,
};

/** Current playback position, extrapolated between backend updates. */
export function posNow(): number {
  const p = app.player;
  if (!p.track) return 0;
  if (!p.playing) return p.posMs;
  return Math.min(p.durationMs || Infinity, p.posMs + (Date.now() - p.at));
}

export const clock = $state({ pos: 0 });
let ticking = false;
function tick() {
  clock.pos = posNow();
  requestAnimationFrame(tick);
}

export const isDark = () => ["aura", "noir", "liquid"].includes(app.settings?.theme ?? "aura");

export function trackById(id: string): Track | undefined {
  return trackIndex.get(id);
}
let trackIndex = new Map<string, Track>();
function reindex() {
  trackIndex = new Map(app.tracks.map((t) => [t.id, t]));
}

export function toast(text: string) {
  app.toast = { text, at: Date.now() };
  setTimeout(() => {
    if (app.toast && Date.now() - app.toast.at >= 2900) app.toast = null;
  }, 3000);
}

function applyTheme() {
  const s = app.settings;
  if (!s) return;
  const root = document.documentElement;
  root.dataset.theme = s.theme;
  root.dataset.motion = s.reducedMotion ? "reduced" : "full";
  // Paper keeps its signal red: ink-on-paper identity matters more than per-track colour.
  applyPalette(app.player.track?.palette, isDark(), s.accentFromCover && s.theme !== "paper");
}

let lyricsFor = "";
export async function loadLyrics(force = false) {
  const t = app.player.track;
  if (!t) { app.lyrics = null; lyricsFor = ""; return; }
  if (!force && lyricsFor === t.id) return;
  lyricsFor = t.id;
  app.lyricsLoading = true;
  if (!force) app.lyrics = null;
  try {
    const doc = await api.lyrics(t.id, force);
    if (lyricsFor === t.id) app.lyrics = doc;
  } finally {
    if (lyricsFor === t.id) app.lyricsLoading = false;
  }
}

export async function refreshPlaylists() {
  app.playlists = await api.playlists();
}

export async function refreshTracks() {
  app.tracks = await api.tracks();
  reindex();
}

export async function saveSettings(mut: (s: Settings) => void) {
  if (!app.settings) return;
  const next = $state.snapshot(app.settings) as Settings;
  mut(next);
  app.settings = next;
  applyTheme();
  const errors = await api.setSettings(next);
  if (errors.length) toast(`Не удалось назначить: ${errors.join(", ")}`);
}

export const player = {
  playList(ids: string[], start = 0) { api.player({ type: "playList", ids, start }); },
  toggle() { api.player({ type: "toggle" }); },
  next() { api.player({ type: "next" }); },
  prev() { api.player({ type: "prev" }); },
  seekBy(ms: number) {
    const d = app.player.durationMs;
    if (!app.player.track) return;
    player.seek(Math.max(0, Math.min(d - 500, posNow() + ms)));
  },
  seek(ms: number) {
    app.player.posMs = ms;
    app.player.at = Date.now();
    api.player({ type: "seek", ms: Math.round(ms) });
  },
  volume(v: number) { app.player.volume = v; api.player({ type: "volume", value: v }); },
  shuffle() { api.player({ type: "shuffle", on: !app.player.shuffle }); },
  repeat() {
    const order = ["off", "all", "one"] as const;
    const next = order[(order.indexOf(app.player.repeat) + 1) % 3];
    api.player({ type: "repeat", mode: next });
  },
  queueRemove(index: number) { api.player({ type: "queueRemove", index }); },
  queueMove(from: number, to: number) { api.player({ type: "queueMove", from, to }); },
  queueClear() { api.player({ type: "queueClear" }); },
  enqueue(ids: string[]) { api.player({ type: "enqueue", ids }); toast("Добавлено в очередь"); },
  playNext(ids: string[]) { api.player({ type: "playNext", ids }); toast("Сыграет следующим"); },
  jumpTo(index: number) { api.player({ type: "jumpTo", index }); },
  async like(id?: string) {
    const target = id ?? app.player.track?.id;
    if (target) await api.toggleLike(target);
  },
};

/** Wires backend events. `withLibrary` is false for the small windows (widget, flyout). */
export async function init(withLibrary: boolean) {
  const [settings, state] = await Promise.all([api.settings(), api.playerState()]);
  app.settings = settings;
  app.player = state;
  applyTheme();
  if (withLibrary) await refreshTracks();

  let lastTrack = state.track?.id;
  await listen<PlayerState>("oto://state", (e) => {
    app.player = e.payload;
    if (e.payload.track?.id !== lastTrack) {
      lastTrack = e.payload.track?.id;
      applyTheme();
      if (app.lyricsOpen || app.settings?.widget.style === "lyrics" || withLibrary) loadLyrics();
    }
  });
  await listen<SpectrumFrame>("oto://spectrum", (e) => {
    Object.assign(frame, e.payload);
    frame.t = performance.now();
  });
  await listen<Settings>("oto://settings", (e) => {
    app.settings = e.payload;
    applyTheme();
  });
  await listen<{ id: string; liked: boolean }>("oto://liked", (e) => {
    const t = trackIndex.get(e.payload.id);
    if (t) t.liked = e.payload.liked;
    app.tracks = app.tracks.map((x) => (x.id === e.payload.id ? { ...x, liked: e.payload.liked } : x));
    reindex();
    if (app.player.track?.id === e.payload.id) app.player.track = { ...app.player.track, liked: e.payload.liked };
  });
  await listen<string>("oto://error", (e) => toast(e.payload));
  if (withLibrary) {
    await listen<{ done: number; total: number; finished: boolean }>("oto://scan", (e) => {
      app.scan = e.payload;
      if (e.payload.finished) setTimeout(() => (app.scan = null), 1500);
    });
    await listen("oto://library", () => refreshTracks());
    await listen("oto://playlists", () => refreshPlaylists());
    await refreshPlaylists();
  }
  if (app.player.track) loadLyrics();
  if (!ticking) { ticking = true; requestAnimationFrame(tick); }
  app.ready = true;
}

/** Window fullscreen. F11 anywhere, or the buttons in the stage and lyrics views. */
export async function setFullscreen(on: boolean) {
  app.fullscreen = on;
  try { await getCurrentWindow().setFullscreen(on); } catch { /* browser preview */ }
}
export const toggleFullscreen = () => setFullscreen(!app.fullscreen);
