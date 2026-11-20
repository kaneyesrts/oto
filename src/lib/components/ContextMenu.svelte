<script lang="ts">
  // One context menu for tracks everywhere: right click on a row, or its ⋯ button.
  import { app, player, trackById, toast } from "../store.svelte";
  import { api } from "../api";
  import { ListPlus, ListEnd, Heart, ChevronRight, X, Disc3, Users, Plus } from "../icons";

  const m = $derived(app.menu!);
  const one = $derived(m.ids.length === 1 ? trackById(m.ids[0]) : undefined);
  let sub = $state(false);
  let naming = $state(false);
  let newName = $state("");
  let el: HTMLDivElement;
  let pos = $state({ x: 0, y: 0 });

  $effect(() => {
    // Keep the menu on screen.
    const w = el?.offsetWidth ?? 240, h = el?.offsetHeight ?? 300;
    pos = { x: Math.min(m.x, innerWidth - w - 8), y: Math.min(m.y, innerHeight - h - 8) };
  });

  const close = () => { app.menu = null; };
  const run = (f: () => void) => () => { f(); close(); };

  async function addTo(id: number, name: string) {
    const n = await api.playlistAdd(id, m.ids);
    toast(n ? `Добавлено в «${name}»` : `Уже есть в «${name}»`);
    close();
  }
  async function createWith() {
    const name = newName.trim() || "Новый плейлист";
    await api.playlistCreate(name, m.ids);
    toast(`Плейлист «${name}» создан`);
    close();
  }
  async function removeFromPlaylist() {
    if (m.playlistId == null || m.index == null) return;
    const ids = await api.playlistTracks(m.playlistId);
    ids.splice(m.index, 1);
    await api.playlistSet(m.playlistId, ids);
    close();
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && close()} onblur={close} />
<div class="scrim" role="presentation" onpointerdown={close} oncontextmenu={(e) => { e.preventDefault(); close(); }}></div>

<div class="menu" bind:this={el} style="left:{pos.x}px;top:{pos.y}px" role="menu">
  {#if one}<div class="head ellipsis">{one.title}<span class="faint"> · {one.artist}</span></div>{:else}<div class="head">{m.ids.length} треков</div>{/if}
  <button role="menuitem" onclick={run(() => player.playNext(m.ids))}><ListPlus size={15} /> Сыграть следующим</button>
  <button role="menuitem" onclick={run(() => player.enqueue(m.ids))}><ListEnd size={15} /> В конец очереди</button>
  {#if one}
    <button role="menuitem" onclick={run(() => player.like(one.id))}><Heart size={15} fill={one.liked ? "currentColor" : "none"} /> {one.liked ? "Убрать из избранного" : "В избранное"}</button>
  {/if}
  <div class="sep"></div>
  <div class="subwrap" role="presentation" onpointerenter={() => (sub = true)} onpointerleave={() => (sub = naming)}>
    <button role="menuitem" onclick={() => (sub = true)}><Plus size={15} /> Добавить в плейлист <ChevronRight size={14} class="chev" /></button>
    {#if sub}
      <div class="submenu" class:left={pos.x > innerWidth - 560} role="menu">
        {#if naming}
          <form onsubmit={(e) => { e.preventDefault(); createWith(); }}>
            <!-- svelte-ignore a11y_autofocus -->
            <input bind:value={newName} placeholder="Название плейлиста" autofocus />
          </form>
        {:else}
          <button role="menuitem" class="new" onclick={() => (naming = true)}><Plus size={14} /> Новый плейлист</button>
        {/if}
        {#each app.playlists as p (p.id)}
          <button role="menuitem" onclick={() => addTo(p.id, p.name)}><span class="ellipsis">{p.name}</span><span class="faint num">{p.count}</span></button>
        {/each}
      </div>
    {/if}
  </div>
  {#if m.playlistId != null && m.index != null}
    <button role="menuitem" onclick={removeFromPlaylist}><X size={15} /> Убрать из плейлиста</button>
  {/if}
  {#if one}
    <div class="sep"></div>
    {#if one.album}
      <button role="menuitem" onclick={run(() => { app.view = "albums"; app.focus = { kind: "album", key: `${one.albumArtist}\u001f${one.album}` }; })}><Disc3 size={15} /> К альбому</button>
    {/if}
    <button role="menuitem" onclick={run(() => { app.view = "artists"; app.focus = { kind: "artist", key: one.albumArtist || one.artist }; })}><Users size={15} /> К артисту</button>
  {/if}
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 80; }
  .menu, .submenu {
    position: fixed; z-index: 81; min-width: 230px; max-width: 300px; padding: 6px;
    border-radius: 14px;
    background: color-mix(in srgb, var(--bg) 78%, #000 0%);
    backdrop-filter: blur(30px) saturate(1.4);
    box-shadow: 0 24px 60px -20px rgb(0 0 0 / 0.6), inset 0 0 0 1px var(--line);
    animation: pop 0.16s var(--ease-out);
    font-size: 13px;
  }
  :root[data-theme="aura"] .menu, :root[data-theme="aura"] .submenu, :root[data-theme="liquid"] .menu, :root[data-theme="liquid"] .submenu { background: rgb(18 18 24 / 0.82); }
  :root[data-theme="mist"] .menu, :root[data-theme="mist"] .submenu { background: rgb(255 255 255 / 0.82); }
  :root[data-theme="paper"] .menu, :root[data-theme="paper"] .submenu { border-radius: 0; background: var(--bg); box-shadow: 4px 4px 0 var(--text), inset 0 0 0 1px var(--text); }
  @keyframes pop { from { opacity: 0; transform: scale(0.97) translateY(-4px); } }
  .head { padding: 6px 10px 8px; font-weight: 650; max-width: 280px; }
  button { width: 100%; display: flex; align-items: center; gap: 10px; height: 34px; padding: 0 10px; border-radius: 9px; text-align: left; color: var(--text); }
  button:hover { background: var(--hover); }
  :root[data-theme="paper"] button { border-radius: 0; }
  :root[data-theme="paper"] button:hover { background: var(--text); color: var(--bg); }
  button :global(.chev) { margin-left: auto; opacity: 0.5; }
  button .ellipsis { flex: 1; min-width: 0; }
  .sep { height: 1px; margin: 5px 8px; background: var(--line); }
  .subwrap { position: relative; }
  .submenu.left { left: auto; right: calc(100% + 4px); }
  .submenu { position: absolute; left: calc(100% + 4px); top: -6px; max-height: 340px; overflow-y: auto; }
  .new { color: var(--accent); font-weight: 600; }
  input { width: 100%; height: 34px; padding: 0 10px; border-radius: 9px; border: 1px solid var(--accent); background: var(--surface); outline: none; font-size: 13px; }
</style>
