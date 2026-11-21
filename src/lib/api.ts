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
  remoteId?: string;
  /** In the library (remote tracks played from search are not, until saved). */
  saved: boolean;
  /** Other copies of the same recording, best first. */
  alts?: string[];
  /** A better copy exists; lists show that one. */
  hidden?: boolean;
  /** "<source>:<id>" of the artist / album at a service. */
  artistRef?: string;
  albumRef?: string;
}

export interface RemoteArtist {
  source: string;
  remoteId: string;
  name: string;
  imageUrl?: string | null;
  genres: string[];
  followers?: number | null;
}

export interface RemoteAlbum {
  source: string;
  remoteId: string;
  title: string;
  artist: string;
  year?: number | null;
  coverUrl?: string | null;
  trackCount?: number | null;
  kind?: string | null;
}

export interface GlobalHit { track: Track; others: Track[] }
export interface GlobalResults { tracks: GlobalHit[]; artists: RemoteArtist[]; errors: [string, string][] }
export interface ArtistView {
  artist: RemoteArtist;
  description?: string | null;
  topTracks: Track[];
  albums: RemoteAlbum[];
  similar: RemoteArtist[];
}
export interface AlbumView { album: RemoteAlbum; tracks: Track[] }

export interface SourceStatus {
  id: string;
  name: string;
  connected: boolean;
  worksAnonymously: boolean;
  account?: string | null;
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
  buffering?: boolean;
  format?: string;
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
  scene: "cover" | SceneId;
  lyricsStyle: "line" | "edit";
  textGlow: number;
  color: "white" | "cover";
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

export type SceneId = "aura" | "space" | "silk" | "aurora" | "depth" | "smoke" | "void";

export interface LyricsSettings {
  fontScale: number;
  depthBlur: boolean;
  autoAlign: boolean;
  online: boolean;
  style: "classic" | "edit";
  align: "left" | "center";
  hlSung: boolean;
  hlChorus: boolean;
  hlFinale: boolean;
  glow: number;
  beat: number;
  color: "white" | "cover";
  wordSync?: boolean;
}

export type ThemeId = "aura" | "noir" | "mist" | "liquid" | "paper";

export interface Settings {
  theme: ThemeId;
  accentFromCover: boolean;
  reducedMotion: boolean;
  closeToTray: boolean;
  folders: string[];
  volume: number;
  lyrics: LyricsSettings;
  visual: { particles: boolean; intensity: number; quality: "low" | "high"; scene: SceneId; sceneSpeed: number };
  widget: WidgetSettings;
  hotkeys: Hotkeys;
  audio: AudioSettings;
  station: StationSettings;
}

export type StationMode = "style" | "discover" | "favorites" | "random";
export interface StationSettings { mode: StationMode; novelty: number; excludeRecent: boolean }

export interface AudioSettings {
  crossfadeMs: number;
  gapless: boolean;
  eqEnabled: boolean;
  eqPreamp: number;
  eqGains: number[];
  eqPreset: string;
  /** "lossless" (FLAC where available) | "high" (320 kbps) | "normal". */
  streamQuality: string;
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
  | { type: "repeat"; mode: string }
  | { type: "queueRemove"; index: number }
  | { type: "queueMove"; from: number; to: number }
  | { type: "queueClear" };

export interface Playlist {
  id: number;
  name: string;
  count: number;
  durationMs: number;
  covers: string[];
  updatedAt: number;
}

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
  sources: () => invoke<SourceStatus[]>("sources_status"),
  sourceLogin: (id: string) => invoke("source_login", { id }),
  sourceLogout: (id: string) => invoke("source_logout", { id }),
  sourceSearch: (id: string, query: string) => invoke<Track[]>("source_search", { id, query }),
  sourceStore: (tracks: Track[], saved: boolean) => invoke("source_store", { tracks, saved }),
  sourceImportLiked: (id: string) => invoke("source_import_liked", { id }),
  trackSetSaved: (id: string, saved: boolean) => invoke("track_set_saved", { id, saved }),
  searchAll: (query: string) => invoke<GlobalResults>("search_all", { query }),
  artistPage: (reference: string) => invoke<ArtistView>("artist_page", { reference }),
  albumPage: (reference: string) => invoke<AlbumView>("album_page", { reference }),
  artistRef: (name: string) => invoke<string | null>("artist_ref", { name }),
  artistImages: (names: string[]) => invoke<Record<string, string>>("artist_images", { names }),
  stationNext: (req: { mode: string; novelty: number; excludeRecent: boolean; exclude: string[]; sessionStart: number }) =>
    invoke<{ tracks: Track[]; basis: string[] }>("station_next", { req }),
  stationBasis: () => invoke<string[]>("station_basis"),
  playlists: () => invoke<Playlist[]>("playlists"),
  playlistTracks: (id: number) => invoke<string[]>("playlist_tracks", { id }),
  playlistCreate: (name: string, ids?: string[]) => invoke<number>("playlist_create", { name, ids }),
  playlistRename: (id: number, name: string) => invoke("playlist_rename", { id, name }),
  playlistDelete: (id: number) => invoke("playlist_delete", { id }),
  playlistAdd: (id: number, ids: string[]) => invoke<number>("playlist_add", { id, ids }),
  playlistSet: (id: number, ids: string[]) => invoke("playlist_set", { id, ids }),
};

const coverCache = new Map<string, string>();
export function coverUrl(path?: string | null): string | null {
  if (!path) return null;
  if (path.startsWith("data:") || path.startsWith("http")) return path; // mock / not yet cached
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

export const SOURCE_LABEL: Record<string, string> = { local: "Файл", yandex: "Яндекс", soundcloud: "SoundCloud", spotify: "Spotify" };
