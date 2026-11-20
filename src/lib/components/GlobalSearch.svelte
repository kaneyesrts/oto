<script lang="ts">
  // "Всё": the library and every service at once. Same song found on several services is
  // one row with a badge per service; artists are cards that open full artist pages.
  import { app, libraryTracks, player, playRemote, openArtist, trackById } from "../store.svelte";
  import { api, coverUrl, fmtTime, type GlobalResults, type Track } from "../api";
  import { search } from "../groups";
  import SourceBadge from "./SourceBadge.svelte";
  import { Play, Info, Heart } from "../icons";

  let { query }: { query: string } = $props();

  let res = $state<GlobalResults | null>(null);
  let loading = $state(false);
  let seq = 0;
  let timer = 0;
  $effect(() => {
    const q = query.trim();
    clearTimeout(timer);
    if (q.length < 2) { res = null; return; }
    loading = true;
    const my = ++seq;
    timer = window.setTimeout(async () => {
      const r = await api.searchAll(q).catch(() => null);
      if (my === seq) { res = r; loading = false; }
    }, 450);
  });

  const local = $derived(search(libraryTracks(), query).slice(0, 6));
  const best = $derived(res?.artists[0] && res.tracks[0] && normalize(res.artists[0].name) === normalize(query) ? "artist" : res?.tracks[0] ? "track" : null);
  function normalize(s: string) { return s.toLowerCase().replace(/[^\p{L}\p{N}]/gu, ""); }

  function play(i: number) {
    if (!res) return;
    const tracks = res.tracks.map((h) => h.track);
    const extra = res.tracks.flatMap((h) => h.others);
    playRemote(tracks, i, extra);
  }
  const notConnected = $derived(app.sources.filter((s) => !s.connected).map((s) => s.name));
</script>

<div class="gs">
  {#if local.length}
    <section>
      <div class="sh"><h3>В библиотеке</h3><button class="faint" onclick={() => (app.searchWhere = "library")}>Все совпадения</button></div>
      <div class="rows">
        {#each local as t, i (t.id)}
          <button class="row" ondblclick={() => player.playList(local.map((x) => x.id), i)} onclick={() => player.playList(local.map((x) => x.id), i)}>
            {#if coverUrl(t.cover)}<img class="cv" src={coverUrl(t.cover)} alt="" />{:else}<span class="cv"></span>{/if}
            <span class="tt"><span class="ellipsis nm">{t.title}</span><span class="ellipsis faint">{t.artist}</span></span>
            <span class="badges"><SourceBadge source={t.source} /></span>
            <span class="num faint d">{fmtTime(t.durationMs)}</span>
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if loading && !res}
    <div class="msg muted">Ищем в {app.sources.map((s) => s.name).join(", ")}…</div>
  {:else if res}
    {#if res.artists.length}
      <section>
        <div class="sh"><h3>Исполнители</h3></div>
        <div class="artists">
          {#each res.artists as a (a.source + a.remoteId)}
            <button class="ac" onclick={() => openArtist(a.name, `${a.source}:${a.remoteId}`)}>
              {#if a.imageUrl}<img src={a.imageUrl} alt="" loading="lazy" />{:else}<span class="ph">{a.name.slice(0, 1)}</span>{/if}
              <span class="an ellipsis">{a.name}</span>
              <span class="am"><SourceBadge source={a.source} />{#if a.genres[0]}<span class="faint ellipsis">{a.genres[0]}</span>{/if}</span>
            </button>
          {/each}
        </div>
      </section>
    {/if}

    {#if res.tracks.length}
      <section>
        <div class="sh"><h3>Треки</h3>{#if best === "track"}<span class="faint">лучшее совпадение первым</span>{/if}</div>
        <div class="rows">
          {#each res.tracks as h, i (h.track.id)}
            {@const known = trackById(h.track.id)}
            <div class="row" class:cur={app.player.track?.id === h.track.id} role="row" tabindex="-1" ondblclick={() => play(i)}>
              <button class="cvb" onclick={() => play(i)} aria-label="Играть">
                {#if coverUrl(h.track.cover)}<img class="cv" src={coverUrl(h.track.cover)} alt="" loading="lazy" />{:else}<span class="cv"></span>{/if}
                <span class="pl"><Play size={15} fill="currentColor" strokeWidth={0} /></span>
              </button>
              <span class="tt">
                <span class="ellipsis nm">{h.track.title}</span>
                <span class="ellipsis faint">
                  <button class="link" onclick={() => openArtist(h.track.artist.split(",")[0].trim(), h.track.artistRef || undefined)}>{h.track.artist}</button>{h.track.album ? ` · ${h.track.album}` : ""}{h.track.year ? ` · ${h.track.year}` : ""}
                </span>
              </span>
              <span class="badges">
                <SourceBadge source={h.track.source} />
                {#each h.others as o}<SourceBadge source={o.source} />{/each}
              </span>
              <span class="acts">
                <button class="icon-btn mini" onclick={() => (app.info = known ?? h.track)} aria-label="Сведения"><Info size={15} /></button>
                <button class="icon-btn mini" class:on={(known ?? h.track).liked} onclick={async () => { await player.likeTrack(known ?? h.track); res = res ? { ...res } : res; }} aria-label="Лайк" title={(known ?? h.track).liked ? "Убрать из библиотеки" : "Лайк: трек попадёт в библиотеку"}><Heart size={15} fill={(known ?? h.track).liked ? "currentColor" : "none"} /></button>
              </span>
              <span class="num faint d">{fmtTime(h.track.durationMs)}</span>
            </div>
          {/each}
        </div>
      </section>
    {:else if !local.length}
      <div class="msg muted">Ничего не найдено.</div>
    {/if}

    {#if res.errors.length}
      <div class="msg faint small">Недоступно сейчас: {res.errors.map(([s]) => s).join(", ")}</div>
    {/if}
    {#if notConnected.length}
      <div class="msg faint small">Не подключены: {notConnected.join(", ")}. Подключить можно в настройках → Источники.</div>
    {/if}
  {/if}
</div>

<style>
  .gs { height: 100%; overflow-y: auto; padding: 4px 14px 40px; display: flex; flex-direction: column; gap: 26px; }
  .sh { display: flex; align-items: baseline; justify-content: space-between; margin-bottom: 10px; }
  h3 { margin: 0; font-family: var(--font-display); font-size: 17px; font-weight: 700; letter-spacing: -0.01em; }
  .sh button, .sh span { font-size: 12px; }
  .msg { padding: 10px 0; }
  .small { font-size: 12px; padding: 0; }
  .rows { display: flex; flex-direction: column; }
  .row { display: grid; grid-template-columns: 44px 1fr auto auto 46px; gap: 14px; align-items: center; height: 58px; padding: 0 10px; border-radius: 12px; text-align: left; }
  .row:hover { background: var(--hover); }
  .row.cur .nm { color: var(--accent); }
  .cvb { position: relative; width: 44px; height: 44px; border-radius: 8px; overflow: hidden; }
  .cv { width: 44px; height: 44px; border-radius: 8px; object-fit: cover; display: block; background: var(--surface-2); }
  .pl { position: absolute; inset: 0; display: grid; place-items: center; background: rgb(0 0 0 / 0.45); color: #fff; opacity: 0; transition: opacity 0.15s; }
  .row:hover .pl { opacity: 1; }
  .tt { display: flex; flex-direction: column; min-width: 0; font-size: 12.5px; }
  .nm { font-weight: 600; font-size: 13.5px; }
  .link { color: inherit; }
  .link:hover { color: var(--text); text-decoration: underline; }
  .badges { display: flex; gap: 4px; }
  .acts { display: flex; gap: 2px; }
  .acts .icon-btn:not(.on) { opacity: 0; }
  .row:hover .acts .icon-btn { opacity: 1; }
  .icon-btn.mini { width: 30px; height: 30px; }
  .d { text-align: right; font-size: 12px; }
  .artists { display: grid; grid-auto-flow: column; grid-auto-columns: 132px; gap: 16px; overflow-x: auto; padding-bottom: 6px; }
  .ac { display: flex; flex-direction: column; align-items: center; gap: 6px; text-align: center; min-width: 0; }
  .ac img, .ph { width: 120px; height: 120px; border-radius: 50%; object-fit: cover; background: var(--surface-2); transition: transform 0.3s var(--ease-spring), box-shadow 0.3s; }
  .ph { display: grid; place-items: center; font-size: 40px; font-weight: 800; color: var(--text-3); }
  .ac:hover img { transform: translateY(-3px); box-shadow: 0 18px 30px -18px rgb(0 0 0 / 0.7); }
  .an { font-weight: 650; font-size: 13.5px; max-width: 100%; }
  .am { display: flex; align-items: center; gap: 6px; font-size: 11px; max-width: 100%; min-width: 0; }
  :root[data-theme="paper"] .ac img, :root[data-theme="paper"] .ph { filter: grayscale(1); }
</style>
