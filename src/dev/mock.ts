// Dev-only stand-in backend so the UI can be designed in a plain browser (vite dev without Tauri).
// Never bundled into the app: main.ts imports it only when Tauri internals are absent.

import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { PlayerState, Settings, Track, RGB } from "../lib/api";

const palettes: RGB[][] = [
  [[232, 98, 64], [60, 22, 30], [250, 190, 120]],
  [[40, 80, 160], [220, 230, 255], [10, 20, 50]],
  [[200, 60, 120], [255, 200, 220], [70, 20, 60]],
  [[40, 140, 110], [10, 40, 35], [200, 240, 210]],
  [[150, 120, 230], [30, 20, 60], [240, 220, 255]],
  [[230, 190, 60], [60, 40, 10], [250, 240, 200]],
];
const rgb = (c: RGB) => `rgb(${c[0]},${c[1]},${c[2]})`;
function cover(p: RGB[], seed: number) {
  const shapes = seed % 3 === 0
    ? `<circle cx="${60 + (seed * 37) % 80}" cy="${70 + (seed * 13) % 60}" r="58" fill="${rgb(p[0])}"/>`
    : seed % 3 === 1
      ? `<rect x="30" y="30" width="140" height="140" rx="${seed * 7 % 60}" fill="${rgb(p[0])}" transform="rotate(${seed * 11} 100 100)"/>`
      : `<path d="M0 ${120 + seed % 40} Q100 ${40 + seed % 50} 200 ${110 + seed % 30} V200 H0Z" fill="${rgb(p[0])}"/>`;
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 200"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${rgb(p[1])}"/><stop offset="1" stop-color="${rgb(p[2])}" stop-opacity=".6"/></linearGradient></defs><rect width="200" height="200" fill="url(#g)"/>${shapes}<circle cx="150" cy="40" r="10" fill="${rgb(p[2])}" opacity=".8"/></svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

const artists_ = ["Северный Ветер", "Lumen Drift", "Полынь", "Kite Season", "Неон и Пыль", "Marrow Lane"];
const albumsByArtist = [["Тихий шторм", "Сигнал"], ["Afterglow"], ["Степь"], ["Paper Planes", "Weather"], ["Ночные смены"], ["Hollow Bright"]];
const titles = ["Ниже облаков", "Fold", "Морская соль", "Последний трамвай", "Glass Hours", "Полдень", "Static Bloom", "Тише воды", "Northbound", "Огни", "Paper Moon", "Без названия", "Low Tide", "Перрон", "Ember", "Кислород", "Satellites", "Сны о юге", "Velvet Room", "Отражения", "Coastline", "Перелёт", "Wildflower", "Пятница"];

const tracks: Track[] = titles.map((title, i) => {
  const a = i % artists_.length;
  const albs = albumsByArtist[a];
  const album = albs[i % albs.length];
  const pi = (a * 2 + albs.indexOf(album)) % palettes.length;
  return {
    id: `t${i}`, path: `C:/Music/${title}.flac`, title, artist: artists_[a], album, albumArtist: artists_[a],
    durationMs: 150_000 + ((i * 37_123) % 140_000), trackNo: (i % 9) + 1, year: 2016 + (i % 9),
    genre: ["Инди", "Электроника", "Пост-рок", "Поп"][i % 4], cover: cover(palettes[pi], pi * 7 + albs.indexOf(album)),
    palette: palettes[pi], liked: i % 5 === 0, addedAt: Date.now() - i * 86400000, source: "local", saved: true,
  };
});

const settings: Settings = {
  theme: (localStorage.getItem("oto-theme") as Settings["theme"]) || "aura",
  accentFromCover: true, reducedMotion: false, closeToTray: true, folders: ["C:/Music"], volume: 0.7,
  lyrics: { fontScale: 1, depthBlur: true, autoAlign: true, online: true },
  visual: { particles: true, intensity: 1, quality: "high" },
  widget: { enabled: false, layer: "overlay", style: "compact", opacity: 0.92, blur: 24, scale: 1, clickThrough: false, glow: true, particles: false, spectrum: true },
  station: { mode: "style", novelty: 0.7, excludeRecent: true },
  audio: { crossfadeMs: 0, gapless: true, eqEnabled: false, eqPreamp: 0, eqGains: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0], eqPreset: "flat" },
  hotkeys: { playPause: "Ctrl+Alt+Space", next: "Ctrl+Alt+Right", prev: "Ctrl+Alt+Left", volumeUp: "Ctrl+Alt+Up", volumeDown: "Ctrl+Alt+Down", like: "Ctrl+Alt+L", toggleWidget: "Ctrl+Alt+W", toggleClickThrough: "Ctrl+Alt+O", seekForward: "Ctrl+Alt+Shift+Right", seekBack: "Ctrl+Alt+Shift+Left" },
};

const state: PlayerState = {
  track: tracks[3], playing: true, posMs: 42_000, durationMs: tracks[3].durationMs, volume: 0.7,
  shuffle: false, repeat: "off", queue: tracks.map((t) => t.id), index: 3, at: Date.now(),
};

const lyricText = [
  "", "Город гасит окна по одному", "Я считаю их, как будто это сны", "Где-то между крыш и проводов",
  "Остаётся то, что не сказали мы", "", "Ниже облаков, ниже облаков", "Голос твой становится светлей",
  "Ниже облаков, где нет оков", "Только ветер и немного дней", "", "Утро снова соберёт мосты",
  "И трамваи вытянутся в нить", "Я оставлю свет для темноты", "Чтобы было с чем её делить",
];

const lists: { id: number; name: string; ids: string[]; updatedAt: number }[] = [
  { id: 1, name: "Ночная дорога", ids: ["t3", "t8", "t12", "t1", "t20"], updatedAt: Date.now() },
];
const plMeta = () => lists.map((l) => ({
  id: l.id, name: l.name, count: l.ids.length, updatedAt: l.updatedAt,
  durationMs: l.ids.reduce((s, id) => s + (tracks.find((t) => t.id === id)?.durationMs ?? 0), 0),
  covers: [...new Set(l.ids.map((id) => tracks.find((t) => t.id === id)?.cover).filter(Boolean))].slice(0, 4),
}));
const plChanged = () => emit("oto://playlists", 0);

function pos() { return state.playing ? state.posMs + (Date.now() - state.at) : state.posMs; }
function push() { state.posMs = pos(); state.at = Date.now(); emit("oto://state", { ...state }); }
function load(i: number) {
  state.index = (i + state.queue.length) % state.queue.length;
  state.track = tracks.find((t) => t.id === state.queue[state.index])!;
  state.durationMs = state.track.durationMs; state.posMs = 0; state.at = Date.now();
}

export function installMock() {
  mockWindows(new URLSearchParams(location.search).get("w") ?? "main");
  mockIPC((cmd, args: any) => {
    switch (cmd) {
      case "get_settings": return settings;
      case "set_settings": Object.assign(settings, args.settings); localStorage.setItem("oto-theme", settings.theme); return [];
      case "get_tracks": return tracks;
      case "player_state": return { ...state, posMs: pos(), at: Date.now() };
      case "toggle_like": { const t = tracks.find((x) => x.id === args.id)!; t.liked = !t.liked; emit("oto://liked", { id: t.id, liked: t.liked }); return t.liked; }
      case "get_lyrics":
        return {
          kind: "synced", source: "LRCLIB", autoShiftMs: 1800, autoScale: 1, userOffsetMs: 0,
          lines: lyricText.map((text, i) => ({ t: 8000 + i * 4200, d: 4200, text })),
        };
      case "get_stats":
        return {
          rangeMs: 41 * 3600000, todayMs: 74 * 60000, plays: 612, skips: 88, uniqueTracks: 214, streakDays: 12,
          topTracks: tracks.slice(0, 8).map((t, i) => ({ track: t, plays: 40 - i * 4, ms: (40 - i * 4) * t.durationMs })),
          topArtists: artists_.map((a, i) => ({ artist: a, plays: 120 - i * 15, ms: (9 - i) * 3600000, cover: tracks[i].cover })),
          heat: Array.from({ length: 7 }, (_, d) => Array.from({ length: 24 }, (_, h) => Math.max(0, Math.sin((h - 7) / 3.5) * 1 + (h > 19 ? 1.4 : 0) + (d > 4 ? 0.5 : 0)) * 1800000 * Math.random())),
          daily: Array.from({ length: 30 }, (_, i) => ({ day: new Date(Date.now() - (29 - i) * 86400000).toISOString().slice(0, 10), ms: Math.random() * 3 * 3600000 })),
          rising: { artist: "Полынь", plays: 18, ms: 2.4 * 3600000, cover: tracks[2].cover },
        };
      case "sources_status": return [
        { id: "yandex", name: "Яндекс Музыка", connected: true, worksAnonymously: false, account: "demo" },
        { id: "soundcloud", name: "SoundCloud", connected: false, worksAnonymously: true, account: null },
      ];
      case "source_search": return tracks.slice(0, 8).map((t, i) => ({ ...t, id: `${args.id}:${100 + i}`, source: args.id, saved: i % 3 === 0 }));
      case "source_store": for (const t of args.tracks) if (!tracks.find((x) => x.id === t.id)) tracks.push({ ...t, saved: args.saved }); emit("oto://library", 0); return null;
      case "search_all": {
        const hits = tracks.slice(0, 10).map((t, i) => ({
          track: { ...t, id: `yandex:${200 + i}`, source: "yandex", saved: false, artistRef: "yandex:1" },
          others: i % 2 ? [{ ...t, id: `soundcloud:${300 + i}`, source: "soundcloud", saved: false }] : [],
        }));
        const artists = artists_.map((a, i) => ({ source: i % 3 ? "yandex" : "soundcloud", remoteId: String(i + 1), name: a, imageUrl: tracks[i].cover, genres: ["инди"], followers: 1000 * (i + 1) }));
        return { tracks: hits, artists, errors: [] };
      }
      case "artist_ref": return "yandex:1";
      case "station_basis": return ["Северный Ветер", "Lumen Drift", "Полынь"];
      case "station_next": return { basis: ["Северный Ветер", "Lumen Drift", "Полынь"], tracks: tracks.slice(5, 17).map((t, i) => ({ ...t, id: `yandex:${600 + i + Math.floor(Math.random() * 1e6)}`, source: "yandex", saved: false })) };
      case "artist_images": return {};
      case "artist_page": return {
        artist: { source: "yandex", remoteId: "1", name: "Северный Ветер", imageUrl: tracks[0].cover, genres: ["инди", "пост-рок"], followers: 48211 },
        description: "Группа из Петербурга. Медленные гитары, длинные проигрыши и тексты о городе ночью.",
        topTracks: tracks.slice(0, 10).map((t, i) => ({ ...t, id: `yandex:${400 + i}`, source: "yandex", saved: false })),
        albums: albumsByArtist.flat().map((a, i) => ({ source: "yandex", remoteId: String(i), title: a, artist: "Северный Ветер", year: 2018 + i, coverUrl: tracks[i].cover, trackCount: 8 + i, kind: i % 3 === 2 ? "single" : "album" })),
        similar: artists_.slice(1).map((a, i) => ({ source: "yandex", remoteId: String(i + 9), name: a, imageUrl: tracks[i + 3].cover, genres: [] })),
      };
      case "album_page": return {
        album: { source: "yandex", remoteId: "1", title: "Тихий шторм", artist: "Северный Ветер", year: 2021, coverUrl: tracks[0].cover, trackCount: 9, kind: "album" },
        tracks: tracks.slice(0, 9).map((t, i) => ({ ...t, id: `yandex:${500 + i}`, source: "yandex", saved: false })),
      };
      case "playlists": return plMeta();
      case "playlist_tracks": return lists.find((l) => l.id === args.id)?.ids ?? [];
      case "playlist_create": { const id = Date.now(); lists.unshift({ id, name: args.name, ids: args.ids ?? [], updatedAt: Date.now() }); plChanged(); return id; }
      case "playlist_rename": { const l = lists.find((x) => x.id === args.id); if (l) l.name = args.name; plChanged(); return null; }
      case "playlist_delete": { const i = lists.findIndex((x) => x.id === args.id); if (i >= 0) lists.splice(i, 1); plChanged(); return null; }
      case "playlist_add": { const l = lists.find((x) => x.id === args.id)!; const before = l.ids.length; for (const t of args.ids) if (!l.ids.includes(t)) l.ids.push(t); plChanged(); return l.ids.length - before; }
      case "playlist_set": { const l = lists.find((x) => x.id === args.id); if (l) l.ids = args.ids; plChanged(); return null; }
      case "player": {
        const r = args.req;
        if (r.type === "toggle") { state.posMs = pos(); state.at = Date.now(); state.playing = !state.playing; }
        else if (r.type === "next") load(state.index + 1);
        else if (r.type === "prev") load(state.index - 1);
        else if (r.type === "seek") { state.posMs = r.ms; state.at = Date.now(); }
        else if (r.type === "volume") state.volume = r.value;
        else if (r.type === "playList") { state.queue = r.ids; load(r.start); state.playing = true; }
        else if (r.type === "jumpTo") load(r.index);
        else if (r.type === "shuffle") state.shuffle = r.on;
        else if (r.type === "repeat") state.repeat = r.mode;
        else if (r.type === "queueRemove") { state.queue.splice(r.index, 1); if (r.index < state.index) state.index--; }
        else if (r.type === "queueMove") { const [x] = state.queue.splice(r.from, 1); state.queue.splice(r.to, 0, x); const c = state.index; state.index = c === r.from ? r.to : r.from < c && r.to >= c ? c - 1 : r.from > c && r.to <= c ? c + 1 : c; }
        else if (r.type === "queueClear") { state.queue = [state.queue[state.index]]; state.index = 0; }
        push();
        return null;
      }
      default: return null;
    }
  }, { shouldMockEvents: true });

  // Fake analyser: a kick on every beat at 118 bpm, a hi-hat shimmer and a drifting mid.
  const t0 = performance.now();
  setInterval(() => {
    if (!state.playing) { emit("oto://spectrum", { b: new Array(48).fill(0), lvl: 0, bass: 0, mid: 0, high: 0, beat: 0, pos: pos() }); return; }
    const t = (performance.now() - t0) / 1000;
    const ph = (t * 118 / 60) % 1;
    const kick = Math.exp(-ph * 9);
    const b = Array.from({ length: 48 }, (_, i) => {
      const x = i / 48;
      const v = kick * Math.exp(-x * 9) * 0.9 + (0.45 + 0.2 * Math.sin(t * 1.3 + i * 0.4)) * Math.exp(-((x - 0.45) ** 2) / 0.05) + 0.25 * Math.random() * x;
      return Math.round(Math.min(1, v) * 255);
    });
    emit("oto://spectrum", { b, lvl: 0.55 + kick * 0.3, bass: kick, mid: 0.5 + 0.2 * Math.sin(t), high: 0.4 * Math.random(), beat: ph < 0.06 ? 1 : Math.exp(-ph * 6), pos: pos() });
  }, 1000 / 60);
}
