<script lang="ts">
  import { app, player, trackById, refreshPlaylists, toast } from "../store.svelte";
  import { api, coverUrl, fmtDuration, plural, type Track } from "../api";
  import TrackList from "../components/TrackList.svelte";
  import { Play, Shuffle, Trash, Pencil } from "../icons";

  const id = $derived(Number(app.focus?.key));
  const meta = $derived(app.playlists.find((p) => p.id === id));
  let ids = $state<string[]>([]);
  const tracks = $derived(ids.map((x) => trackById(x)).filter(Boolean) as Track[]);
  const total = $derived(tracks.reduce((s, t) => s + t.durationMs, 0));
  let editing = $state(false);
  let name = $state("");
  let confirmDelete = $state(false);

  async function load() { ids = await api.playlistTracks(id); }
  $effect(() => { id; app.playlists; load(); });

  function reorder(from: number, to: number) {
    const next = [...ids];
    const [x] = next.splice(from, 1);
    next.splice(to, 0, x);
    ids = next; // optimistic
    api.playlistSet(id, next);
  }
  async function rename() {
    editing = false;
    if (name.trim() && name.trim() !== meta?.name) await api.playlistRename(id, name.trim());
  }
  async function remove() {
    if (!confirmDelete) { confirmDelete = true; setTimeout(() => (confirmDelete = false), 3000); return; }
    await api.playlistDelete(id);
    await refreshPlaylists();
    toast(`Плейлист «${meta?.name}» удалён`);
    app.view = "home";
    app.focus = null;
  }
  function shuffled() {
    const s = [...ids];
    for (let i = s.length - 1; i > 0; i--) { const j = Math.floor(Math.random() * (i + 1)); [s[i], s[j]] = [s[j], s[i]]; }
    player.playList(s, 0);
  }
</script>

{#if meta}
  <div class="pl">
    <header>
      <div class="mosaic" class:single={meta.covers.length < 4}>
        {#each meta.covers.slice(0, meta.covers.length < 4 ? 1 : 4) as c}<img src={coverUrl(c)} alt="" />{:else}<div class="empty"></div>{/each}
      </div>
      <div class="meta">
        <div class="eyebrow">Плейлист</div>
        {#if editing}
          <form onsubmit={(e) => { e.preventDefault(); rename(); }}>
            <!-- svelte-ignore a11y_autofocus -->
            <input class="name-input" bind:value={name} onblur={rename} autofocus />
          </form>
        {:else}
          <button class="title" onclick={() => { name = meta.name; editing = true; }} title="Переименовать">
            <h1>{meta.name}</h1><Pencil size={16} />
          </button>
        {/if}
        <div class="muted">{meta.count} {plural(meta.count, "трек", "трека", "треков")} · {fmtDuration(total)}</div>
        <div class="acts">
          <button class="pill solid" disabled={!ids.length} onclick={() => player.playList(ids, 0)}><Play size={15} fill="currentColor" strokeWidth={0} /> Слушать</button>
          <button class="pill ghost" disabled={!ids.length} onclick={shuffled}><Shuffle size={15} /> Перемешать</button>
          <button class="pill ghost danger" onclick={remove}><Trash size={15} /> {confirmDelete ? "Точно удалить?" : "Удалить"}</button>
        </div>
      </div>
    </header>
    <div class="body">
      {#if tracks.length}
        <TrackList {tracks} playlistId={id} onReorder={reorder} />
      {:else}
        <div class="hint muted">Плейлист пуст. Добавляйте треки через ⋯ или правый клик → «Добавить в плейлист».</div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .pl { height: 100%; display: flex; flex-direction: column; min-height: 0; }
  header { display: flex; gap: 26px; align-items: flex-end; padding: 28px 32px 20px; }
  .mosaic { width: 184px; height: 184px; flex: none; display: grid; grid-template-columns: 1fr 1fr; border-radius: 16px; overflow: hidden; box-shadow: 0 30px 60px -30px rgb(0 0 0 / 0.8); background: var(--surface); }
  .mosaic.single { grid-template-columns: 1fr; }
  .mosaic img { width: 100%; height: 100%; object-fit: cover; }
  .mosaic .empty { background: linear-gradient(135deg, rgb(var(--accent-rgb, var(--accent-default)) / 0.6), var(--surface-2)); }
  :root[data-theme="paper"] .mosaic { border-radius: 0; box-shadow: none; filter: grayscale(1); }
  .meta { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .title { display: flex; align-items: center; gap: 12px; text-align: left; color: var(--text); }
  .title :global(svg) { opacity: 0; transition: opacity 0.15s; flex: none; }
  .title:hover :global(svg) { opacity: 0.5; }
  h1 { margin: 4px 0; font-family: var(--font-display); font-size: clamp(28px, 3.2vw, 52px); font-weight: 800; letter-spacing: -0.035em; line-height: 1.02; }
  :root[data-theme="paper"] h1 { font-style: italic; }
  .name-input { font: inherit; font-family: var(--font-display); font-size: clamp(28px, 3.2vw, 52px); font-weight: 800; letter-spacing: -0.035em; background: transparent; border: 0; border-bottom: 2px solid var(--accent); outline: none; width: min(640px, 100%); margin: 4px 0; }
  .acts { display: flex; gap: 10px; margin-top: 14px; }
  .danger:hover { color: #ff6b6b; border-color: #ff6b6b; }
  .body { flex: 1; min-height: 0; padding: 0 18px; }
  .hint { padding: 30px 14px; }
  button:disabled { opacity: 0.4; pointer-events: none; }
</style>
