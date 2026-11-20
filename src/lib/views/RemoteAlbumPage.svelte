<script lang="ts">
  import { app, playRemote, openArtist, back, toast, player } from "../store.svelte";
  import { api, fmtDuration, fmtTime, plural, type AlbumView } from "../api";
  import SourceBadge from "../components/SourceBadge.svelte";
  import { Play, Shuffle, ArrowLeft, Heart } from "../icons";

  let view = $state<AlbumView | null>(null);
  let error = $state("");
  $effect(() => {
    const r = app.focus?.key ?? "";
    view = null;
    error = "";
    api.albumPage(r).then((v) => { if (app.focus?.key === r) view = v; }).catch((e) => (error = String(e)));
  });
  const total = $derived(view?.tracks.reduce((s, t) => s + t.durationMs, 0) ?? 0);

  function shuffled() {
    if (!view) return;
    const t = [...view.tracks];
    for (let i = t.length - 1; i > 0; i--) { const j = Math.floor(Math.random() * (i + 1)); [t[i], t[j]] = [t[j], t[i]]; }
    playRemote(t, 0);
  }
  async function likeAll() {
    if (!view) return;
    for (const t of view.tracks) if (!t.liked) await player.likeTrack(t);
    toast(`Альбом «${view.album.title}» в избранном и в библиотеке`);
  }
</script>

<div class="ra">
  <header>
    {#if view?.album.coverUrl}<img class="bg" src={view.album.coverUrl} alt="" />{/if}
    <button class="back" onclick={back} aria-label="Назад"><ArrowLeft size={18} /></button>
    <div class="in">
      {#if view?.album.coverUrl}<img class="cover" src={view.album.coverUrl} alt="" />{:else}<div class="cover"></div>{/if}
      <div class="meta">
        <div class="eyebrow">{view?.album.kind === "single" ? "Сингл" : "Альбом"}{#if view} · <SourceBadge source={view.album.source} />{/if}</div>
        <h1>{view?.album.title ?? app.focus?.name ?? "…"}</h1>
        {#if view}
          <button class="artist" onclick={() => openArtist(view!.album.artist.split(",")[0].trim())}>{view.album.artist}</button>
          <div class="muted">{view.album.year ?? ""}{view.album.year ? " · " : ""}{view.tracks.length} {plural(view.tracks.length, "трек", "трека", "треков")} · {fmtDuration(total)}</div>
          <div class="acts">
            <button class="pill solid" onclick={() => playRemote(view!.tracks, 0)}><Play size={15} fill="currentColor" strokeWidth={0} /> Слушать</button>
            <button class="pill ghost" onclick={shuffled}><Shuffle size={15} /> Перемешать</button>
            <button class="pill ghost" onclick={likeAll}><Heart size={15} /> Лайкнуть альбом</button>
          </div>
        {/if}
      </div>
    </div>
  </header>
  {#if error}<div class="pad faint">Не удалось загрузить альбом: {error}</div>{/if}
  {#if view}
    <div class="list">
      {#each view.tracks as t, i (t.id + i)}
        <button class="row" class:cur={app.player.track?.id === t.id} onclick={() => playRemote(view!.tracks, i)} oncontextmenu={(e) => { e.preventDefault(); app.info = t; }}>
          <span class="n num faint">{i + 1}</span>
          <span class="tt"><span class="ellipsis nm">{t.title}</span><span class="ellipsis faint">{t.artist}</span></span>
          <span class="num faint">{fmtTime(t.durationMs)}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .ra { height: 100%; overflow-y: auto; }
  header { position: relative; overflow: hidden; padding: 56px 36px 26px; }
  .bg { position: absolute; inset: -40%; width: 180%; height: 180%; object-fit: cover; mask-image: linear-gradient(180deg, #000 45%, transparent 62%); filter: blur(70px) saturate(1.4) brightness(0.55); opacity: 0.5; pointer-events: none; }
  :root[data-theme="paper"] .bg { display: none; }
  .back { position: absolute; top: 16px; left: 22px; z-index: 2; width: 34px; height: 34px; border-radius: 50%; display: grid; place-items: center; background: rgb(0 0 0 / 0.25); color: var(--text); }
  .in { position: relative; display: flex; gap: 28px; align-items: flex-end; }
  .cover { width: 210px; height: 210px; border-radius: 16px; object-fit: cover; box-shadow: 0 30px 60px -30px rgb(0 0 0 / 0.8); background: var(--surface-2); flex: none; }
  :root[data-theme="paper"] .cover { border-radius: 0; filter: grayscale(1); }
  .meta { display: flex; flex-direction: column; gap: 5px; min-width: 0; }
  .eyebrow { display: flex; align-items: center; gap: 6px; }
  h1 { margin: 0; font-family: var(--font-display); font-size: clamp(30px, 4vw, 60px); font-weight: 800; letter-spacing: -0.035em; line-height: 1; }
  :root[data-theme="paper"] h1 { font-style: italic; }
  .artist { align-self: flex-start; font-weight: 650; color: var(--accent); }
  .acts { display: flex; gap: 10px; margin-top: 12px; }
  .pad { padding: 20px 36px; }
  .list { padding: 6px 22px 40px; display: flex; flex-direction: column; }
  .row { display: grid; grid-template-columns: 30px 1fr auto; gap: 12px; align-items: center; height: 54px; padding: 0 12px; border-radius: 12px; text-align: left; }
  .row:hover { background: var(--hover); }
  .row.cur .nm { color: var(--accent); }
  .tt { display: flex; flex-direction: column; min-width: 0; font-size: 12px; }
  .nm { font-weight: 600; font-size: 13.5px; }
  .n { text-align: center; font-size: 12px; }
</style>
