<script lang="ts">
  import { untrack } from "svelte";
  import { app, libraryTracks, player } from "../store.svelte";
  import { fmtDuration, plural } from "../api";
  import { search } from "../groups";
  import TrackList from "../components/TrackList.svelte";
  import OnlineResults from "../components/OnlineResults.svelte";
  import GlobalSearch from "../components/GlobalSearch.svelte";
  import { SOURCE_LABEL } from "../api";
  import { Play, Shuffle } from "../icons";

  let { liked = false } = $props();
  type Sort = "artist" | "title" | "added" | "duration";
  // Favourites keep the order you liked them in (the service's order for imports).
  let sort = $state<Sort>(untrack(() => liked) ? "added" : "artist");
  // Search tab: everything, the library, or one service.
  const where = $derived(app.search ? app.searchWhere : "library");
  const online = $derived(app.sources.filter((s) => s.connected || s.worksAnonymously));
  // Source filter for the track list.
  let src = $state("all");
  const counts = $derived.by(() => {
    const m: Record<string, number> = {};
    for (const t of libraryTracks()) m[t.source] = (m[t.source] ?? 0) + 1;
    return m;
  });
  const srcOrder = ["local", "yandex", "spotify", "soundcloud"];

  const list = $derived.by(() => {
    let l = liked ? libraryTracks().filter((t) => t.liked) : libraryTracks();
    if (src !== "all") l = l.filter((t) => t.source === src);
    l = search(l, app.search);
    if (sort === "title") l = [...l].sort((a, b) => a.title.localeCompare(b.title));
    else if (sort === "added") l = [...l].sort((a, b) => b.addedAt - a.addedAt);
    else if (sort === "duration") l = [...l].sort((a, b) => b.durationMs - a.durationMs);
    return l;
  });
  const total = $derived(list.reduce((s, t) => s + t.durationMs, 0));
  const sorts: [Sort, string][] = [["artist", "Артист"], ["title", "Название"], ["added", "Новые"], ["duration", "Длина"]];

  function shuffleAll() {
    const ids = list.map((t) => t.id);
    for (let i = ids.length - 1; i > 0; i--) { const j = Math.floor(Math.random() * (i + 1)); [ids[i], ids[j]] = [ids[j], ids[i]]; }
    player.playList(ids, 0);
  }
</script>

<div class="lib">
  <header>
    <div>
      <div class="eyebrow">{liked ? "Коллекция" : app.search ? "Поиск" : "Библиотека"}</div>
      <h1>{liked ? "Избранное" : app.search ? `«${app.search}»` : "Все треки"}</h1>
      <div class="muted sub">{list.length} {plural(list.length, "трек", "трека", "треков")} · {fmtDuration(total)}</div>
    </div>
    {#if app.search && !liked}
      <div class="seg where">
        <button class:on={where === "all"} onclick={() => (app.searchWhere = "all")}>Всё</button>
        <button class:on={where === "library"} onclick={() => (app.searchWhere = "library")}>Библиотека <em>{list.length}</em></button>
        {#each online as s}<button class:on={where === s.id} onclick={() => (app.searchWhere = s.id)}>{s.name}</button>{/each}
      </div>
    {/if}
    <div class="actions" class:hidden={where !== "library"}>
      <div class="seg">
        {#each sorts as [k, label]}
          <button class:on={sort === k} onclick={() => (sort = k)}>{label}</button>
        {/each}
      </div>
      <button class="pill ghost" onclick={shuffleAll} disabled={!list.length}><Shuffle size={15} /> Перемешать</button>
      <button class="pill solid" onclick={() => player.playList(list.map((t) => t.id), 0)} disabled={!list.length}><Play size={15} fill="currentColor" strokeWidth={0} /> Слушать</button>
    </div>
  </header>
  {#if where === "library" && Object.keys(counts).length > 1}
    <div class="srcbar">
      <button class:on={src === "all"} onclick={() => (src = "all")}>Все <em>{Object.values(counts).reduce((a, b) => a + b, 0)}</em></button>
      {#each srcOrder.filter((s) => counts[s]) as s}
        <button class:on={src === s} onclick={() => (src = s)}><i class="dot d-{s}"></i>{s === "local" ? "Файлы" : SOURCE_LABEL[s]} <em>{counts[s]}</em></button>
      {/each}
    </div>
  {/if}
  <div class="body">
    {#if where === "all"}
      <GlobalSearch query={app.search} />
    {:else if where !== "library"}
      <OnlineResults source={where} query={app.search} />
    {:else if list.length}
      <TrackList tracks={list} />
    {:else}
      <div class="empty muted">{liked ? "Нажмите ♥ у трека, чтобы он появился здесь." : "Ничего не найдено."}</div>
    {/if}
  </div>
</div>

<style>
  .lib { height: 100%; display: flex; flex-direction: column; min-height: 0; }
  header { display: flex; align-items: flex-end; justify-content: space-between; gap: 20px; padding: 28px 32px 18px; flex-wrap: wrap; }
  h1 { margin: 6px 0 4px; font-family: var(--font-display); font-size: clamp(28px, 3vw, 40px); font-weight: 700; letter-spacing: -0.03em; line-height: 1.05; }
  :root[data-theme="noir"] h1 { font-weight: 500; }
  :root[data-theme="paper"] h1 { font-style: italic; font-size: clamp(40px, 5vw, 72px); }
  .sub { font-size: 13px; }
  .actions { display: flex; align-items: center; gap: 10px; }
  .actions.hidden { display: none; }
  .where em { font-style: normal; opacity: 0.5; margin-left: 4px; }
  .srcbar { display: flex; gap: 6px; padding: 0 32px 12px; flex-wrap: wrap; }
  .srcbar button { display: inline-flex; align-items: center; gap: 7px; height: 30px; padding: 0 12px; border-radius: 999px; font-size: 12.5px; color: var(--text-2); background: var(--surface); }
  .srcbar button:hover { color: var(--text); }
  .srcbar button.on { background: var(--text); color: var(--bg); }
  .srcbar em { font-style: normal; opacity: 0.55; }
  :root[data-theme="paper"] .srcbar button { border-radius: 0; background: none; border: 1px solid var(--line); }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--text-3); }
  .d-yandex { background: #ffcc33; } .d-soundcloud { background: #ff7a33; } .d-spotify { background: #3ee07a; } .d-local { background: var(--accent); }
  .seg { display: flex; padding: 3px; border-radius: 999px; background: var(--surface); }
  .seg button { height: 30px; padding: 0 12px; border-radius: 999px; font-size: 12.5px; color: var(--text-2); }
  .seg button.on { background: var(--surface-2); color: var(--text); }
  :root[data-theme="paper"] .seg { border-radius: 0; background: none; border: 1px solid var(--line); padding: 0; }
  :root[data-theme="paper"] .seg button { border-radius: 0; }
  :root[data-theme="paper"] .seg button.on { background: var(--text); color: var(--bg); }
  .body { flex: 1; min-height: 0; padding: 0 18px; }
  .empty { padding: 40px 14px; }
  button:disabled { opacity: 0.4; pointer-events: none; }
</style>
