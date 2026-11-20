<script lang="ts">
  // "About this track": everything we know, and every copy we can play it from.
  import { fade, scale } from "svelte/transition";
  import { app, trackById, openArtist, openRemoteAlbum, player, playRemote } from "../store.svelte";
  import { coverUrl, fmtTime, SOURCE_LABEL, type Track } from "../api";
  import SourceBadge from "./SourceBadge.svelte";
  import { X, Play } from "../icons";

  const t = $derived(app.info!);
  const copies = $derived([t, ...(t.alts ?? []).map((id) => trackById(id)).filter(Boolean) as Track[]]);
  const close = () => (app.info = null);
  function playCopy(c: Track) {
    close();
    if (trackById(c.id)) player.playList([c.id], 0);
    else playRemote([c], 0);
  }
  const rows = $derived(
    [
      ["Исполнитель", t.artist],
      ["Альбом", t.album],
      ["Год", t.year ? String(t.year) : ""],
      ["Жанр", t.genre ?? ""],
      ["Длительность", fmtTime(t.durationMs)],
      ["Номер", t.trackNo ? String(t.trackNo) : ""],
      ["Файл", t.source === "local" ? t.path : ""],
    ].filter(([, v]) => v) as [string, string][]
  );
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && close()} />
<div class="scrim" role="presentation" onclick={close} transition:fade={{ duration: 150 }}></div>
<div class="card" role="dialog" aria-label="Сведения о треке" transition:scale={{ start: 0.96, duration: 220 }}>
  <button class="x" onclick={close} aria-label="Закрыть"><X size={18} /></button>
  <div class="top">
    {#if coverUrl(t.cover)}<img src={coverUrl(t.cover)} alt="" />{:else}<span class="ph"></span>{/if}
    <div class="ti">
      <div class="eyebrow">Трек</div>
      <h2>{t.title}</h2>
      <button class="ar" onclick={() => { close(); openArtist(t.artist.split(",")[0].trim(), t.artistRef || undefined); }}>{t.artist}</button>
      {#if t.albumRef}<button class="al" onclick={() => { close(); openRemoteAlbum(t.albumRef!, t.album); }}>{t.album}</button>{/if}
    </div>
  </div>
  <dl>
    {#each rows as [k, v]}<dt>{k}</dt><dd class:mono={k === "Файл"}>{v}</dd>{/each}
    <dt>В библиотеке</dt><dd>{t.saved !== false ? "да" : "нет"}{t.liked ? " · в избранном" : ""}</dd>
  </dl>
  <div class="srcs">
    <div class="eyebrow">Где есть</div>
    {#each copies as c (c.id)}
      <button class="src" onclick={() => playCopy(c)}>
        <SourceBadge source={c.source} />
        <span class="ellipsis">{c.source === "local" ? "Файл на диске" : (SOURCE_LABEL[c.source] ?? c.source)}{c.id === t.id ? " · эта копия" : ""}</span>
        <span class="num faint">{fmtTime(c.durationMs)}</span>
        <Play size={14} fill="currentColor" strokeWidth={0} />
      </button>
    {/each}
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 90; background: rgb(0 0 0 / 0.45); backdrop-filter: blur(4px); }
  .card {
    position: fixed; z-index: 91; left: 50%; top: 50%; translate: -50% -50%;
    width: min(520px, calc(100vw - 40px)); max-height: calc(100vh - 80px); overflow-y: auto;
    padding: 22px; border-radius: 22px;
    background: color-mix(in srgb, var(--bg) 92%, #000);
    box-shadow: 0 40px 100px -30px rgb(0 0 0 / 0.8), inset 0 0 0 1px var(--line);
  }
  :root[data-theme="aura"] .card, :root[data-theme="liquid"] .card { background: rgb(18 18 24 / 0.92); backdrop-filter: blur(30px); }
  :root[data-theme="mist"] .card { background: rgb(255 255 255 / 0.9); backdrop-filter: blur(30px); }
  :root[data-theme="paper"] .card { border-radius: 0; background: var(--bg); box-shadow: 6px 6px 0 var(--text), inset 0 0 0 1px var(--text); }
  .x { position: absolute; top: 14px; right: 14px; width: 32px; height: 32px; border-radius: 50%; display: grid; place-items: center; color: var(--text-2); }
  .x:hover { background: var(--hover); color: var(--text); }
  .top { display: flex; gap: 18px; align-items: flex-end; margin-bottom: 18px; }
  .top img, .ph { width: 120px; height: 120px; border-radius: 12px; object-fit: cover; background: var(--surface-2); flex: none; }
  .ti { min-width: 0; display: flex; flex-direction: column; align-items: flex-start; gap: 2px; }
  h2 { margin: 2px 0; font-family: var(--font-display); font-size: 24px; font-weight: 800; letter-spacing: -0.02em; line-height: 1.1; }
  .ar { font-weight: 650; color: var(--accent); text-align: left; }
  .al { font-size: 13px; color: var(--text-2); text-align: left; }
  .al:hover, .ar:hover { text-decoration: underline; }
  dl { display: grid; grid-template-columns: 120px 1fr; gap: 8px 14px; margin: 0 0 18px; font-size: 13px; }
  dt { color: var(--text-3); }
  dd { margin: 0; min-width: 0; overflow-wrap: anywhere; }
  .mono { font-family: var(--font-num); font-size: 12px; color: var(--text-2); }
  .srcs { display: flex; flex-direction: column; gap: 4px; }
  .srcs .eyebrow { margin-bottom: 4px; }
  .src { display: grid; grid-template-columns: auto 1fr auto auto; gap: 10px; align-items: center; height: 38px; padding: 0 10px; border-radius: 10px; background: var(--surface); text-align: left; font-size: 13px; }
  .src:hover { background: var(--surface-2); }
</style>
