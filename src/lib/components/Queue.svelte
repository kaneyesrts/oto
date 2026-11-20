<script lang="ts">
  import { fly } from "svelte/transition";
  import { app, player, trackById } from "../store.svelte";
  import { coverUrl, fmtTime } from "../api";
  import { X } from "../icons";

  const items = $derived(app.player.queue.map((id, i) => ({ i, t: trackById(id) })));
  let list: HTMLDivElement;
  $effect(() => {
    // Keep the current track in view when the drawer opens.
    list?.querySelector(".cur")?.scrollIntoView({ block: "center" });
  });
</script>

<div class="drawer" transition:fly={{ x: 380, duration: 380, opacity: 1 }}>
  <header>
    <div><div class="eyebrow">Очередь</div><b>{app.player.queue.length} в очереди</b></div>
    <button class="icon-btn" onclick={() => (app.queueOpen = false)} aria-label="Закрыть"><X size={18} /></button>
  </header>
  <div class="items" bind:this={list}>
    {#each items as it (it.i)}
      {#if it.t}
        <button class="q" class:cur={it.i === app.player.index} class:past={it.i < app.player.index} onclick={() => player.jumpTo(it.i)}>
          {#if coverUrl(it.t.cover)}<img class="cover" src={coverUrl(it.t.cover)} alt="" loading="lazy" />{:else}<span class="cover"></span>{/if}
          <span class="qt"><span class="ellipsis">{it.t.title}</span><span class="ellipsis faint">{it.t.artist}</span></span>
          <span class="num faint">{fmtTime(it.t.durationMs)}</span>
        </button>
      {/if}
    {/each}
  </div>
</div>

<style>
  .drawer {
    position: absolute; top: 0; right: 0; bottom: 0; width: 360px; z-index: 20;
    display: flex; flex-direction: column;
    background: color-mix(in srgb, var(--bg) 82%, transparent);
    backdrop-filter: blur(30px) saturate(1.3);
    border-left: 1px solid var(--line);
    box-shadow: -30px 0 60px -30px rgb(0 0 0 / 0.5);
  }
  header { display: flex; align-items: center; justify-content: space-between; padding: 18px 16px 12px 20px; }
  .items { flex: 1; overflow-y: auto; padding: 0 10px 20px; }
  .q { width: 100%; display: grid; grid-template-columns: 40px 1fr auto; gap: 12px; align-items: center; padding: 7px 10px; border-radius: 12px; text-align: left; font-size: 13px; }
  .q:hover { background: var(--hover); }
  .q.cur { background: var(--accent-soft); }
  .q.cur .qt span:first-child { color: var(--accent); }
  .q.past { opacity: 0.45; }
  .q .cover { width: 40px; height: 40px; border-radius: 7px; }
  .qt { display: flex; flex-direction: column; min-width: 0; font-weight: 550; }
  .qt span:last-child { font-size: 12px; font-weight: 400; }
  .num { font-size: 11px; }
</style>
