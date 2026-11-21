import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, type LyricsDoc, type PlayerState, type Playlist, type Settings, type SourceStatus, type SpectrumFrame, type Track, type WordSyncProgress, type WordSyncModelProgress } from "./api";
import { applyPalette } from "./color";

export type View = "home" | "library" | "albums" | "artists" | "liked" | "stats" | "settings" | "playlist" | "artist" | "remote-album";

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
  /** Search tab: "all" (everything), "library", or a source id. */
  searchWhere: "all",
  lyricsOpen: false,
  queueOpen: false,
  scan: null as null | { done: number; total: number; finished: boolean },
  lyrics: null as LyricsDoc | null,
  lyricsLoading: false,
  /** Word sync running for a track: which stage, 0–1. */
  wordsync: null as null | WordSyncProgress,
  /** Speech model download. */
  wordsyncModel: null as null | WordSyncModelProgress,
  toast: null as null | { text: string; at: number },
  /** Album or artist opened from a grid. */
  focus: null as null | { kind: "album" | "artist" | "playlist" | "remote-artist" | "remote-album"; key: string; name?: string },
  /** Track shown in the "about this track" card. */
  info: null as Track | null,
  /** Library artist name → cached photo path. */
  artistImages: {} as Record<string, string>,
  /** Navigation history for the back button. */
  history: [] as { view: View; focus: any }[],
  playlists: [] as Playlist[],
  sources: [] as SourceStatus[],
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
/** Asks the backend to re-time the current track's lyrics (it only starts on a lyrics request). */
export function requestWordSync() {
  const t = app.player.track;
  if (t && app.settings?.lyrics.wordSync) api.lyrics(t.id, false).catch(() => {});
}

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

// ---------------------------------------------------------------- «Поток»

export const STATION_MODES: { id: import("./api").StationMode; name: string; note: string }[] = [
  { id: "style", name: "В моём стиле", note: "похожее на то, что вы слушаете чаще всего" },
  { id: "discover", name: "Открытия", note: "только незнакомые исполнители" },
  { id: "favorites", name: "Любимое", note: "ваши частые и забытые треки" },
  { id: "random", name: "Случайно", note: "что угодно, без оглядки на вкус" },
];

export const station = $state({
  active: false,
  loading: false,
  startedAt: 0,
  basis: [] as string[],
  /** Covers of the latest batch, for the card. */
  covers: [] as string[],
  played: [] as string[],
});

async function stationBatch() {
  const cfg = app.settings!.station;
  const b = await api.stationNext({
    mode: cfg.mode,
    novelty: cfg.novelty,
    excludeRecent: cfg.excludeRecent,
    exclude: [...station.played, ...app.player.queue],
    sessionStart: station.startedAt,
  });
  station.basis = b.basis;
  station.covers = b.tracks.map((t) => t.cover).filter(Boolean).slice(0, 5) as string[];
  station.played.push(...b.tracks.map((t) => t.id));
  return b.tracks;
}

export async function startStation() {
  if (station.loading) return;
  station.loading = true;
  station.startedAt = Date.now();
  station.played = [];
  try {
    const tracks = await stationBatch();
    if (!tracks.length) { toast("Пока не из чего собрать поток: послушайте что-нибудь"); return; }
    await playRemote(tracks, 0, [], true);
    station.active = true;
  } finally {
    station.loading = false;
  }
}

export function stopStation() { station.active = false; }

/** Keeps the station queue topped up. Called on every player update. */
async function stationRefill() {
  if (!station.active || station.loading) return;
  const left = app.player.queue.length - app.player.index - 1;
  if (left > 3) return;
  station.loading = true;
  try {
    const tracks = await stationBatch();
    const unknown = tracks.filter((t) => !trackIndex.has(t.id));
    if (unknown.length) {
      await api.sourceStore(unknown, false);
      for (const t of unknown) trackIndex.set(t.id, t);
    }
    if (tracks.length) api.player({ type: "enqueue", ids: tracks.map((t) => t.id) });
  } finally {
    station.loading = false;
  }
}

/** Opens an artist page. `ref` is "<source>:<id>" when known; otherwise the name is looked up. */
export function openArtist(name: string, ref?: string) {
  go("artist", { kind: "remote-artist", key: ref ?? "", name });
}

export function openRemoteAlbum(ref: string, name?: string) {
  go("remote-album", { kind: "remote-album", key: ref, name });
}

export function go(view: View, focus: typeof app.focus = null) {
  app.history = [...app.history.slice(-30), { view: app.view, focus: app.focus }];
  app.view = view;
  app.focus = focus;
}

export function back() {
  const h = app.history.pop();
  if (h) { app.view = h.view; app.focus = h.focus; }
  else { app.view = "home"; app.focus = null; }
}

let imgPending = new Set<string>();
/** Asks for photos of these artists; results arrive into app.artistImages. */
export async function wantArtistImages(names: string[]) {
  const ask = names.filter((n) => n && !(n in app.artistImages) && !imgPending.has(n));
  if (!ask.length) return;
  ask.forEach((n) => imgPending.add(n));
  const have = await api.artistImages(ask);
  app.artistImages = { ...app.artistImages, ...have };
}

/** Library rows: saved, and only the best copy of each recording. */
export const libraryTracks = () => app.tracks.filter((t) => t.saved !== false && !t.hidden);

export async function refreshSources() {
  app.sources = await api.sources();
}

/** Plays tracks that may not be stored yet (search results): store them first.
 *  `extra` are other-service copies, stored so playback can fall back to them. */
export async function playRemote(tracks: Track[], start: number, extra: Track[] = [], fromStation = false) {
  const unknown = [...tracks, ...extra].filter((t) => !trackIndex.has(t.id));
  if (unknown.length) {
    await api.sourceStore(unknown, false);
    for (const t of unknown) trackIndex.set(t.id, t);
  }
  if (fromStation) api.player({ type: "playList", ids: tracks.map((t) => t.id), start });
  else player.playList(tracks.map((t) => t.id), start);
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
  /** Playing anything else ends the station. */
  playList(ids: string[], start = 0) { station.active = false; api.player({ type: "playList", ids, start }); },
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
  /** Like a track that may not be stored yet (search results, artist pages). */
  async likeTrack(t: Track) {
    if (!trackIndex.has(t.id)) {
      await api.sourceStore([t], false);
      trackIndex.set(t.id, t);
    }
    const liked = await api.toggleLike(t.id);
    t.liked = liked;
    t.saved = liked || t.source === "local";
    return liked;
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
    if (withLibrary) stationRefill();
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
    app.tracks = app.tracks.map((x) => (x.id === e.payload.id ? { ...x, liked: e.payload.liked, saved: e.payload.liked || x.source === "local" } : x));
    reindex();
    if (app.player.track?.id === e.payload.id) app.player.track = { ...app.player.track, liked: e.payload.liked };
  });
  await listen<string>("oto://error", (e) => toast(e.payload));
  // Word-exact lyrics: the refined text replaces the current one in place.
  await listen<{ id: string; doc: LyricsDoc }>("oto://lyrics-refined", (e) => {
    const doc = e.payload.doc;
    // Timing now comes from the singing itself: a manual nudge would only push it off.
    if (doc.userOffsetMs) {
      api.setLyricsOffset(e.payload.id, 0);
      doc.userOffsetMs = 0;
    }
    if (app.player.track?.id === e.payload.id) app.lyrics = doc;
  });
  await listen<WordSyncProgress>("oto://wordsync-progress", (e) => {
    app.wordsync = ["done", "failed", "cancelled"].includes(e.payload.stage) ? null : e.payload;
  });
  await listen<WordSyncModelProgress>("oto://wordsync-model", (e) => {
    app.wordsyncModel = e.payload;
    if (e.payload.error) toast(`Модель не скачалась: ${e.payload.error}`);
    if (e.payload.done && !e.payload.error) requestWordSync();
  });
  if (withLibrary) {
    await listen<{ done: number; total: number; finished: boolean }>("oto://scan", (e) => {
      app.scan = e.payload;
      if (e.payload.finished) setTimeout(() => (app.scan = null), 1500);
    });
    await listen("oto://library", () => refreshTracks());
    await listen("oto://playlists", () => refreshPlaylists());
    await refreshPlaylists();
    await listen("oto://sources", () => refreshSources());
    await listen<Record<string, string>>("oto://artist-images", (e) => {
      app.artistImages = { ...app.artistImages, ...e.payload };
    });
    await listen<{ source: string; playlist: number; count: number }>("oto://imported", (e) => {
      toast(`Импортировано ${e.payload.count}: порядок как в сервисе — в плейлисте «Мне нравится»`);
    });
    refreshSources();
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
