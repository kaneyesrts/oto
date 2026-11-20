<script lang="ts">
  // Queue drawer: virtualised (a whole library can be queued), drag to reorder, × to remove.
  import { fly } from "svelte/transition";
  import { onMount } from "svelte";
  import { app, player, trackById } from "../store.svelte";
  import { coverUrl, fmtTime } from "../api";
  import { X, Grip, ListX } from "../icons";

  const ROW = 54;
  let list: HTMLDivElement;
  let scrollTop = $state(0);
  let height = $state(600);
  const q = $derived(app.player.queue);
  const start = $derived(Math.max(0, Math.floor(scrollTop / ROW) - 6));
  const end = $derived(Math.min(q.length, Math.ceil((scrollTop + height) / ROW) + 6));
  const upcoming = $derived(Math.max(0, q.length - app.player.index - 1));

  onMount(() => {
    // Open scrolled to the current track.
    list.scrollTop = Math.max(0, app.player.index * ROW - 80);
    scrollTop = list.scrollTop;
  });

  let drag = $state<{ from: number; to: number; y: number; grab: number } | null>(null);
  const top = () => list.getBoundingClientRect().top - list.scrollTop;
  function down(e: PointerEvent, i: number) {
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { from: i, to: i, y: i * ROW, grab: e.clientY - top() - i * ROW };
  }
  function move(e: PointerEvent) {
    if (!drag) return;
    drag.y = Math.max(0, Math.min((q.length - 1) * ROW, e.clientY - top() - drag.grab));
    drag.to = Math.round(drag.y / ROW);
    const r = list.getBoundingClientRect();
    if (e.clientY < r.top + 40) list.scrollTop -= 10;
    else if (e.clientY > r.bottom - 40) list.scrollTop += 10;
  }
  function up(e?: PointerEvent) {
    if (e && drag) move(e);
    if (drag && drag.from !== drag.to) player.queueMove(drag.from, drag.to);
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

<div class="drawer" transition:fly={{ x: 380, duration: 380, opacity: 1 }}>
  <header>
    <div><div class="eyebrow">Очередь</div><b>{upcoming ? `Ещё ${upcoming}` : "Это последний трек"}</b></div>
    <div class="hb">
      {#if upcoming}<button class="icon-btn" onclick={() => player.queueClear()} title="Очистить очередь" aria-label="Очистить очередь"><ListX size={17} /></button>{/if}
      <button class="icon-btn" onclick={() => (app.queueOpen = false)} aria-label="Закрыть"><X size={18} /></button>
    </div>
  </header>
  <div class="items" bind:this={list} bind:clientHeight={height} onscroll={() => (scrollTop = list.scrollTop)}>
    <div class="spacer" style="height:{q.length * ROW}px">
      {#each q.slice(start, end) as id, k (start + k + ":" + id)}
        {@const i = start + k}
        {@const t = trackById(id)}
        {#if t}
          <div
            class="q"
            class:cur={i === app.player.index}
            class:past={i < app.player.index}
            class:dragging={drag?.from === i}
            class:shifting={!!drag && drag.from !== i}
            style="transform:translateY({rowY(i)}px)"
            role="button"
            tabindex="0"
            onclick={() => player.jumpTo(i)}
            onkeydown={(e) => e.key === "Enter" && player.jumpTo(i)}
            oncontextmenu={(e) => { e.preventDefault(); app.menu = { x: e.clientX, y: e.clientY, ids: [id] }; }}
          >
            <span class="grip" role="presentation" onpointerdown={(e) => down(e, i)} onpointermove={move} onpointerup={up} onpointercancel={up} onclick={(e) => e.stopPropagation()}><Grip size={14} /></span>
            {#if coverUrl(t.cover)}<img class="cover" src={coverUrl(t.cover)} alt="" loading="lazy" />{:else}<span class="cover"></span>{/if}
            <span class="qt"><span class="ellipsis">{t.title}</span><span class="ellipsis faint">{t.artist}</span></span>
            <span class="num faint dur">{fmtTime(t.durationMs)}</span>
            <button class="rm" onclick={(e) => { e.stopPropagation(); player.queueRemove(i); }} aria-label="Убрать из очереди"><X size={14} /></button>
          </div>
        {/if}
      {/each}
    </div>
  </div>
</div>

<style>
  .drawer {
    position: absolute; top: 0; right: 0; bottom: 0; width: 370px; z-index: 20;
    display: flex; flex-direction: column;
    background: color-mix(in srgb, var(--bg) 82%, transparent);
    backdrop-filter: blur(30px) saturate(1.3);
    border-left: 1px solid var(--line);
    box-shadow: -30px 0 60px -30px rgb(0 0 0 / 0.5);
  }
  :root[data-theme="aura"] .drawer { background: rgb(10 10 14 / 0.55); border-left: 0; }
  header { display: flex; align-items: center; justify-content: space-between; padding: 18px 12px 12px 20px; }
  .hb { display: flex; gap: 2px; }
  .items { flex: 1; overflow-y: auto; padding: 0 8px 20px; }
  .spacer { position: relative; }
  .q {
    position: absolute; left: 0; right: 0; height: 54px;
    display: grid; grid-template-columns: 16px 40px 1fr auto 26px; gap: 10px; align-items: center;
    padding: 0 6px; border-radius: 12px; text-align: left; font-size: 13px; cursor: pointer;
  }
  .q:hover { background: var(--hover); }
  .q.cur { background: var(--accent-soft); }
  .q.cur .qt span:first-child { color: var(--accent); }
  .q.past { opacity: 0.45; }
  .q .cover { width: 40px; height: 40px; border-radius: 7px; }
  .qt { display: flex; flex-direction: column; min-width: 0; font-weight: 550; }
  .qt span:last-child { font-size: 12px; font-weight: 400; }
  .dur { font-size: 11px; }
  .grip { display: grid; place-items: center; height: 30px; cursor: grab; color: var(--text-3); opacity: 0; touch-action: none; }
  .rm { width: 26px; height: 26px; border-radius: 50%; display: grid; place-items: center; color: var(--text-3); opacity: 0; }
  .rm:hover { background: var(--hover); color: var(--text); }
  .q:hover .grip, .q:hover .rm, .dragging .grip { opacity: 1; }
  .dragging { z-index: 3; background: var(--surface-2) !important; box-shadow: 0 16px 40px -16px rgb(0 0 0 / 0.6); cursor: grabbing; opacity: 1 !important; }
  .shifting { transition: transform 0.22s var(--ease-out); }
</style>
