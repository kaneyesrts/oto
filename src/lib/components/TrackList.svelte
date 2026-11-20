<script lang="ts">
  // Virtualised track table: only visible rows are in the DOM, so 20k-track libraries scroll smoothly.
  import { app, player, openArtist } from "../store.svelte";
  import { coverUrl, fmtTime, SOURCE_LABEL, type Track } from "../api";
  import { Heart, Play, Ellipsis, Grip } from "../icons";
  import Spectrum from "./Spectrum.svelte";

  let {
    tracks,
    showAlbum = true,
    numbered = false,
    playlistId,
    onReorder,
  }: {
    tracks: Track[];
    showAlbum?: boolean;
    numbered?: boolean;
    /** When set, the context menu offers "remove from playlist". */
    playlistId?: number;
    /** When set, rows get a drag handle and can be reordered. */
    onReorder?: (from: number, to: number) => void;
  } = $props();

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

  function menu(e: MouseEvent, i: number) {
    e.preventDefault();
    e.stopPropagation();
    app.menu = { x: e.clientX, y: e.clientY, ids: [tracks[i].id], playlistId, index: playlistId != null ? i : undefined };
  }

  // Pointer-driven reorder that works with virtualisation: rows are absolutely placed by
  // index, so while dragging we just shift the rows between `from` and `to` by one slot.
  let drag = $state<{ from: number; to: number; y: number; grab: number } | null>(null);
  function dragStart(e: PointerEvent, i: number) {
    if (!onReorder) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const top = viewport.getBoundingClientRect().top + 34 - viewport.scrollTop;
    drag = { from: i, to: i, y: i * ROW, grab: e.clientY - top - i * ROW };
  }
  function dragMove(e: PointerEvent) {
    if (!drag) return;
    const top = viewport.getBoundingClientRect().top + 34 - viewport.scrollTop;
    const y = e.clientY - top - drag.grab;
    drag.y = Math.max(0, Math.min((tracks.length - 1) * ROW, y));
    drag.to = Math.round(drag.y / ROW);
    // Auto-scroll near the edges.
    const r = viewport.getBoundingClientRect();
    if (e.clientY < r.top + 50) viewport.scrollTop -= 12;
    else if (e.clientY > r.bottom - 40) viewport.scrollTop += 12;
  }
  function dragEnd(e?: PointerEvent) {
    if (e && drag) dragMove(e);
    if (drag && drag.from !== drag.to) onReorder?.(drag.from, drag.to);
    drag = null;
  }
  const rowY = (i: number) => {
    if (!drag) return i * ROW;
    if (i === drag.from) return drag.y;
    if (drag.from < drag.to && i > drag.from && i <= drag.to) return (i - 1) * ROW;
    if (drag.from > drag.to && i >= drag.to && i < drag.from) return (i + 1) * ROW;
    return i * ROW;
  };
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
        class:dragging={drag?.from === i}
        class:shifting={!!drag && drag.from !== i}
        style="transform:translateY({rowY(i)}px)"
        ondblclick={() => play(i)}
        oncontextmenu={(e) => menu(e, i)}
        role="row"
        tabindex="-1"
      >
        <span class="c-n num">
          {#if onReorder}
            <span class="grip" role="button" tabindex="-1" aria-label="Перетащить" onpointerdown={(e) => dragStart(e, i)} onpointermove={dragMove} onpointerup={dragEnd} onpointercancel={dragEnd}><Grip size={14} /></span>
          {/if}
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
            <span class="by ellipsis">{#if t.source !== "local"}<i class="src s-{t.source}">{SOURCE_LABEL[t.source] ?? t.source}</i>{:else if t.alts?.length}<i class="src">+{t.alts.length}</i>{/if}<button class="alink" onclick={(e) => { e.stopPropagation(); openArtist((t.albumArtist || t.artist).split(",")[0].trim(), t.artistRef || undefined); }}>{t.artist}</button></span>
          </span>
        </span>
        {#if showAlbum}<span class="c-al ellipsis muted">{t.album}</span>{/if}
        <span class="c-act">
          <button class="icon-btn mini" onclick={(e) => menu(e, i)} aria-label="Ещё"><Ellipsis size={16} /></button>
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
    grid-template-columns: 44px minmax(0, 2.2fr) minmax(0, 1.3fr) 76px 56px;
    align-items: center;
    gap: 14px;
    padding: 0 14px;
  }
  .list:not(:has(.c-al)) .head, .list:not(:has(.c-al)) .row { grid-template-columns: 44px minmax(0, 1fr) 76px 56px; }
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
  .grip { position: absolute; left: -16px; top: 50%; translate: 0 -50%; display: grid; place-items: center; width: 18px; height: 30px; cursor: grab; color: var(--text-3); opacity: 0; touch-action: none; }
  .row:hover .grip, .dragging .grip { opacity: 1; }
  .dragging { z-index: 3; background: var(--surface-2) !important; box-shadow: 0 16px 40px -16px rgb(0 0 0 / 0.6); cursor: grabbing; }
  .shifting { transition: transform 0.22s var(--ease-out); }
  .hover-play { position: absolute; inset: 0; display: none; place-items: center; color: var(--text); }
  .row:hover .hover-play { display: grid; }
  .row:hover .idx, .row:hover .eq { visibility: hidden; }
  .eq { width: 16px; height: 14px; color: var(--accent); }
  .c-t { display: flex; align-items: center; gap: 12px; min-width: 0; }
  .c-t .cover { width: 40px; height: 40px; border-radius: 7px; flex: none; }
  .tt { display: flex; flex-direction: column; min-width: 0; }
  .name { font-weight: 550; font-size: 13.5px; }
  .alink { color: inherit; font: inherit; }
  .alink:hover { color: var(--text); text-decoration: underline; }
  .src { font-style: normal; font-size: 9.5px; font-weight: 700; letter-spacing: 0.04em; text-transform: uppercase; padding: 1px 5px; margin-right: 6px; border-radius: 4px; background: var(--surface-2); color: var(--text-2); vertical-align: 1px; }
  .s-yandex { background: rgb(255 204 0 / 0.18); color: #ffcc33; }
  .s-soundcloud { background: rgb(255 85 0 / 0.18); color: #ff7a33; }
  .s-spotify { background: rgb(30 215 96 / 0.18); color: #3ee07a; }
  :root[data-theme="mist"] .src, :root[data-theme="paper"] .src { color: var(--text); }
  .by { font-size: 12px; color: var(--text-2); }
  :root[data-theme="paper"] .name { font-family: var(--font-display); font-size: 16px; font-weight: 600; }
  .c-al { font-size: 13px; }
  .c-act { display: flex; justify-content: flex-end; gap: 2px; padding-right: 2px; }
  .c-act .icon-btn { opacity: 0; width: 30px; height: 30px; }
  .row:hover .c-act .icon-btn, .c-act .like.on { opacity: 1; }
  .c-d { text-align: right; font-size: 12px; }
</style>
