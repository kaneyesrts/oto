import type { Track } from "./api";

export interface Album {
  key: string;
  title: string;
  artist: string;
  year?: number | null;
  cover?: string | null;
  tracks: Track[];
  durationMs: number;
  addedAt: number;
}

export interface Artist {
  name: string;
  cover?: string | null;
  tracks: Track[];
  albums: number;
}

export const albumKey = (t: Track) => `${t.albumArtist}\u001f${t.album || "—"}`;

export function albums(tracks: Track[]): Album[] {
  const m = new Map<string, Album>();
  for (const t of tracks) {
    const k = albumKey(t);
    let a = m.get(k);
    if (!a) {
      a = { key: k, title: t.album || "Без альбома", artist: t.albumArtist, year: t.year, cover: t.cover, tracks: [], durationMs: 0, addedAt: 0 };
      m.set(k, a);
    }
    a.tracks.push(t);
    a.durationMs += t.durationMs;
    a.addedAt = Math.max(a.addedAt, t.addedAt);
    if (!a.cover && t.cover) a.cover = t.cover;
  }
  for (const a of m.values()) a.tracks.sort((x, y) => (x.trackNo ?? 999) - (y.trackNo ?? 999));
  return [...m.values()].sort((a, b) => a.artist.localeCompare(b.artist) || a.title.localeCompare(b.title));
}

export function artists(tracks: Track[]): Artist[] {
  const m = new Map<string, Artist & { set: Set<string> }>();
  for (const t of tracks) {
    const name = t.albumArtist || t.artist;
    let a = m.get(name);
    if (!a) { a = { name, cover: t.cover, tracks: [], albums: 0, set: new Set() }; m.set(name, a); }
    a.tracks.push(t);
    a.set.add(t.album);
    if (!a.cover && t.cover) a.cover = t.cover;
  }
  return [...m.values()]
    .map(({ set, ...a }) => ({ ...a, albums: set.size }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

export function search(tracks: Track[], q: string): Track[] {
  const s = q.trim().toLowerCase();
  if (!s) return tracks;
  const terms = s.split(/\s+/);
  return tracks.filter((t) => {
    const hay = `${t.title} ${t.artist} ${t.album} ${t.genre ?? ""}`.toLowerCase();
    return terms.every((x) => hay.includes(x));
  });
}
