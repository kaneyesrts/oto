<script lang="ts">
  // Search results from a streaming service. Click plays (tracks are stored on the fly),
  // + puts a track into the library.
  import { app, playRemote, trackById, player } from "../store.svelte";
  import { api, coverUrl, fmtTime, type Track } from "../api";
  import { Play, Heart } from "../icons";

  let { source, query }: { source: string; query: string } = $props();

  let results = $state<Track[]>([]);
  let loading = $state(false);
  let error = $state("");
  let timer = 0;
  let seq = 0;

  $effect(() => {
    const q = query.trim();
    const src = source;
    clearTimeout(timer);
    if (q.length < 2) { results = []; return; }
    loading = true;
    error = "";
    const my = ++seq;
    timer = window.setTimeout(async () => {
      try {
        const r = await api.sourceSearch(src, q);
        if (my === seq) results = r;
      } catch (e) {
        if (my === seq) { error = String(e); results = []; }
      } finally {
        if (my === seq) loading = false;
      }
    }, 450);
  });

  const current = $derived(app.player.track?.id);
</script>

<div class="online">
  {#if source === "yandex" && !app.sources.find((s) => s.id === "yandex")?.connected}
    <div class="hint">Без входа Яндекс Музыка даёт только 30-секундные фрагменты. <button onclick={() => api.sourceLogin("yandex")}>Войти</button></div>
  {/if}
  {#if loading && !results.length}
    <div class="msg muted">Ищем…</div>
  {:else if error}
    <div class="msg muted">Не получилось: {error}</div>
  {:else if !results.length}
    <div class="msg muted">{query.trim().length < 2 ? "Введите запрос" : "Ничего не найдено"}</div>
  {:else}
    {#each results as t, i (t.id)}
      {@const known = trackById(t.id)}
      <div class="r" class:cur={t.id === current} role="row" tabindex="-1" ondblclick={() => playRemote(results, i)}
        oncontextmenu={(e) => { if (known) { e.preventDefault(); app.menu = { x: e.clientX, y: e.clientY, ids: [t.id] }; } }}>
        <button class="art" onclick={() => playRemote(results, i)} aria-label="Играть">
          {#if coverUrl(t.cover)}<img src={coverUrl(t.cover)} alt="" loading="lazy" />{:else}<span class="ph"></span>{/if}
          <span class="pl"><Play size={16} fill="currentColor" strokeWidth={0} /></span>
        </button>
        <span class="tt"><span class="name ellipsis">{t.title}</span><span class="by ellipsis">{t.artist}{t.album ? ` · ${t.album}` : ""}</span></span>
        <span class="act">
          <button class="icon-btn" class:on={(known ?? t).liked} onclick={async () => { await player.likeTrack(known ?? t); results = [...results]; }} title={(known ?? t).liked ? "Убрать из библиотеки" : "Лайк: трек попадёт в библиотеку"} aria-label="Лайк"><Heart size={16} fill={(known ?? t).liked ? "currentColor" : "none"} /></button>
        </span>
        <span class="d num muted">{fmtTime(t.durationMs)}</span>
      </div>
    {/each}
  {/if}
</div>

<style>
  .online { height: 100%; overflow-y: auto; padding: 4px 0 30px; }
  .msg { padding: 30px 14px; }
  .hint { margin: 4px 14px 10px; padding: 10px 14px; border-radius: 12px; background: var(--accent-soft); font-size: 13px; display: flex; align-items: center; gap: 12px; }
  .hint button { margin-left: auto; height: 28px; padding: 0 12px; border-radius: 999px; background: var(--text); color: var(--bg); font-weight: 600; font-size: 12px; }
  .r { display: grid; grid-template-columns: 44px 1fr auto 52px; gap: 14px; align-items: center; height: 58px; padding: 0 14px; border-radius: 12px; }
  .r:hover { background: var(--hover); }
  .r.cur .name { color: var(--accent); }
  .art { position: relative; width: 44px; height: 44px; border-radius: 8px; overflow: hidden; }
  .art img, .ph { width: 100%; height: 100%; object-fit: cover; display: block; background: var(--surface-2); }
  .pl { position: absolute; inset: 0; display: grid; place-items: center; background: rgb(0 0 0 / 0.45); color: #fff; opacity: 0; transition: opacity 0.15s; }
  .r:hover .pl { opacity: 1; }
  .tt { display: flex; flex-direction: column; min-width: 0; }
  .name { font-weight: 550; font-size: 13.5px; }
  .by { font-size: 12px; color: var(--text-2); }
  .act { display: flex; align-items: center; gap: 2px; }
  .d { text-align: right; font-size: 12px; }
</style>
