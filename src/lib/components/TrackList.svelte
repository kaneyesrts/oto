<script lang="ts">
  // Virtualised track table: only visible rows are in the DOM, so 20k-track libraries scroll smoothly.
  import { app, player } from "../store.svelte";
  import { coverUrl, fmtTime, type Track } from "../api";
  import { Heart, Play, ListPlus, ListEnd } from "../icons";
  import Spectrum from "./Spectrum.svelte";

  let { tracks, showAlbum = true, numbered = false }: { tracks: Track[]; showAlbum?: boolean; numbered?: boolean } = $props();

  const ROW = 56;
  let viewport: HTMLDivElement;
  let scrollTop = $state(0);
  let height = $state(600);
  const start = $derived(Math.max(0, Math.floor(scrollTop / ROW) - 8));
  const end = $derived(Math.min(tracks.length, Math.ceil((scrollTop + height) / ROW) + 8));
  const ids = $derived(tracks.map((t) => t.id));
  const current = $derived(app.player.track?.id);
  const paper = $derived(app.settings?.theme === "paper");

  function play(i: number) {
    player.playList(ids, i);
  }
</script>

<div class="list" bind:this={viewport} bind:clientHeight={height} onscroll={() => (scrollTop = viewport.scrollTop)}>
  <div class="head">
    <span class="c-n">#</span>
    <span>Название</span>
    {#if showAlbum}<span class="c-al">Альбом</span>{/if}
    <span class="c-act"></span>
    <span class="c-d">Время</span>
  </div>
  <div class="spacer" style="height:{tracks.length * ROW}px">
    {#each tracks.slice(start, end) as t, k (t.id)}
      {@const i = start + k}
      <div
        class="row"
        class:cur={t.id === current}
        style="transform:translateY({i * ROW}px)"
        ondblclick={() => play(i)}
        role="row"
        tabindex="-1"
      >
        <span class="c-n num">
          {#if t.id === current && app.player.playing}
            <span class="eq"><Spectrum bars={3} gap={2} radius={1} /></span>
          {:else}
            <span class="idx">{numbered ? (t.trackNo ?? i + 1) : i + 1}</span>
          {/if}
          <button class="hover-play" onclick={() => play(i)} aria-label="Играть"><Play size={14} fill="currentColor" strokeWidth={0} /></button>
        </span>
        <span class="c-t">
          {#if !paper}
            {#if coverUrl(t.cover)}<img class="cover" src={coverUrl(t.cover)} alt="" loading="lazy" />{:else}<span class="cover"></span>{/if}
          {/if}
          <span class="tt">
            <span class="name ellipsis">{t.title}</span>
            <span class="by ellipsis">{t.artist}</span>
          </span>
        </span>
        {#if showAlbum}<span class="c-al ellipsis muted">{t.album}</span>{/if}
        <span class="c-act">
          <button class="icon-btn mini" onclick={() => player.playNext([t.id])} aria-label="Сыграть следующим"><ListPlus size={15} /></button>
          <button class="icon-btn mini" onclick={() => player.enqueue([t.id])} aria-label="В конец очереди"><ListEnd size={15} /></button>
          <button class="icon-btn mini like" class:on={t.liked} onclick={() => player.like(t.id)} aria-label="В избранное">
            <Heart size={15} fill={t.liked ? "currentColor" : "none"} />
          </button>
        </span>
        <span class="c-d num muted">{fmtTime(t.durationMs)}</span>
      </div>
    {/each}
  </div>
</div>

<style>
  .list { position: relative; height: 100%; overflow-y: auto; overflow-x: hidden; }
  .head, .row {
    display: grid;
    grid-template-columns: 44px minmax(0, 2.2fr) minmax(0, 1.3fr) 104px 56px;
    align-items: center;
    gap: 14px;
    padding: 0 14px;
  }
  .list:not(:has(.c-al)) .head, .list:not(:has(.c-al)) .row { grid-template-columns: 44px minmax(0, 1fr) 104px 56px; }
  .head {
    position: sticky; top: 0; z-index: 2; height: 34px;
    font-size: 11px; letter-spacing: 0.08em; text-transform: uppercase; color: var(--text-3);
    background: var(--bg);
    border-bottom: 1px solid var(--line);
  }
  :root[data-theme="mist"] .head, :root[data-theme="liquid"] .head, :root[data-theme="aura"] .head { background: transparent; backdrop-filter: blur(24px); }
  :root[data-theme="paper"] .head { font-family: var(--font-num); color: var(--text); background: var(--bg); }
  .spacer { position: relative; }
  .row {
    position: absolute; left: 0; right: 0; height: 56px;
    border-radius: 12px;
    transition: background 0.12s;
  }
  .row:hover { background: var(--hover); }
  .row.cur .name { color: var(--accent); }
  :root[data-theme="paper"] .row { border-radius: 0; border-bottom: 1px solid color-mix(in srgb, var(--text) 12%, transparent); }
  :root[data-theme="paper"] .row:hover { background: var(--surface-2); }

  .c-n { position: relative; display: grid; place-items: center; color: var(--text-3); font-size: 12px; }
  .hover-play { position: absolute; inset: 0; display: none; place-items: center; color: var(--text); }
  .row:hover .hover-play { display: grid; }
  .row:hover .idx, .row:hover .eq { visibility: hidden; }
  .eq { width: 16px; height: 14px; color: var(--accent); }
  .c-t { display: flex; align-items: center; gap: 12px; min-width: 0; }
  .c-t .cover { width: 40px; height: 40px; border-radius: 7px; flex: none; }
  .tt { display: flex; flex-direction: column; min-width: 0; }
  .name { font-weight: 550; font-size: 13.5px; }
  .by { font-size: 12px; color: var(--text-2); }
  :root[data-theme="paper"] .name { font-family: var(--font-display); font-size: 16px; font-weight: 600; }
  .c-al { font-size: 13px; }
  .c-act { display: flex; justify-content: flex-end; gap: 2px; }
  .c-act .icon-btn { opacity: 0; width: 30px; height: 30px; }
  .row:hover .c-act .icon-btn, .c-act .like.on { opacity: 1; }
  .c-d { text-align: right; font-size: 12px; }
</style>
