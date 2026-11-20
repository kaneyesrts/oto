<script lang="ts">
  // One artist page for everything: what you have of them locally, plus their page on a
  // service (photo, popular tracks, discography, similar artists, bio).
  import { app, libraryTracks, player, playRemote, openArtist, openRemoteAlbum, back, wantArtistImages } from "../store.svelte";
  import { api, coverUrl, fmtTime, plural, type ArtistView, type Track } from "../api";
  import { albums as groupAlbums } from "../groups";
  import TrackList from "../components/TrackList.svelte";
  import SourceBadge from "../components/SourceBadge.svelte";
  import { Play, Shuffle, ArrowLeft } from "../icons";

  const name = $derived(app.focus?.name ?? "");
  let ref = $state("");
  let page = $state<ArtistView | null>(null);
  let loading = $state(true);
  let error = $state("");
  let showAllBio = $state(false);
  let albumKind = $state("album");

  $effect(() => {
    const r0 = app.focus?.key ?? "";
    const n = name;
    page = null;
    error = "";
    loading = true;
    (async () => {
      let r = r0 || (await api.artistRef(n).catch(() => null)) || "";
      ref = r;
      if (!r) { loading = false; return; }
      try {
        const p = await api.artistPage(r);
        if ((app.focus?.key ?? "") === r0 && name === n) page = p;
      } catch (e) {
        error = String(e);
      } finally {
        loading = false;
      }
    })();
  });

  const display = $derived(page?.artist.name ?? name);
  const norm = (s: string) => s.toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");
  const mine = $derived.by(() => {
    const k = norm(display);
    return libraryTracks().filter((t) => norm(t.albumArtist) === k || t.artist.split(/,|&|feat\.?/i).some((a) => norm(a) === k));
  });
  const myAlbums = $derived(groupAlbums(mine));
  const photo = $derived(page?.artist.imageUrl ?? (app.artistImages[name] ? coverUrl(app.artistImages[name]) : null) ?? coverUrl(mine[0]?.cover));
  $effect(() => { if (name && !page?.artist.imageUrl) wantArtistImages([name]); });

  const top = $derived(page?.topTracks ?? []);
  const kinds = $derived([...new Set((page?.albums ?? []).map((a) => a.kind ?? "album"))]);
  const shownAlbums = $derived((page?.albums ?? []).filter((a) => (a.kind ?? "album") === albumKind));
  $effect(() => { if (kinds.length && !kinds.includes(albumKind)) albumKind = kinds[0]; });
  const kindLabel = (k: string) => ({ album: "Альбомы", single: "Синглы", compilation: "Сборники", ep: "EP", playlist: "Плейлисты" } as Record<string, string>)[k] ?? k;

  function playTop(i: number) { playRemote(top, i); }
  function playAll(shuffle = false) {
    const list: Track[] = mine.length ? mine : top;
    const ids = list.map((t) => t.id);
    if (shuffle) for (let i = ids.length - 1; i > 0; i--) { const j = Math.floor(Math.random() * (i + 1)); [ids[i], ids[j]] = [ids[j], ids[i]]; }
    if (mine.length) player.playList(ids, 0);
    else playRemote(shuffle ? ids.map((id) => list.find((t) => t.id === id)!) : list, 0);
  }
  const fmtNum = (n: number) => n >= 1e6 ? `${(n / 1e6).toFixed(1)} млн` : n >= 1e3 ? `${Math.round(n / 1e3)} тыс.` : String(n);
</script>

<div class="ap">
  <header class="hero">
    {#if photo}<img class="bg" src={photo} alt="" />{/if}
    <button class="back" onclick={back} aria-label="Назад"><ArrowLeft size={18} /></button>
    <div class="hero-in">
      {#if photo}<img class="ph" src={photo} alt="" />{:else}<div class="ph empty">{display.slice(0, 1)}</div>{/if}
      <div class="meta">
        <div class="eyebrow">Исполнитель{#if page} · <SourceBadge source={page.artist.source} />{/if}</div>
        <h1>{display}</h1>
        <div class="facts muted">
          {#if page?.artist.genres.length}<span>{page.artist.genres.slice(0, 3).join(", ")}</span>{/if}
          {#if page?.artist.followers}<span>{fmtNum(page.artist.followers)} {page.artist.source === "yandex" ? "лайков" : "подписчиков"}</span>{/if}
          {#if mine.length}<span>{mine.length} {plural(mine.length, "трек", "трека", "треков")} в библиотеке</span>{/if}
        </div>
        <div class="acts">
          <button class="pill solid" onclick={() => playAll()} disabled={!mine.length && !top.length}><Play size={15} fill="currentColor" strokeWidth={0} /> Слушать</button>
          <button class="pill ghost" onclick={() => playAll(true)} disabled={!mine.length && !top.length}><Shuffle size={15} /> Перемешать</button>
        </div>
      </div>
    </div>
  </header>

  <div class="body">
    {#if loading}<div class="muted">Загружаем страницу исполнителя…</div>{/if}
    {#if error}<div class="faint">Страница исполнителя недоступна: {error}</div>{/if}

    {#if top.length}
      <section>
        <h3>Популярное</h3>
        <div class="top">
          {#each top.slice(0, 10) as t, i (t.id)}
            <button class="tr" class:cur={app.player.track?.id === t.id} onclick={() => playTop(i)}>
              <span class="n num faint">{i + 1}</span>
              {#if coverUrl(t.cover)}<img src={coverUrl(t.cover)} alt="" loading="lazy" />{:else}<span class="cv"></span>{/if}
              <span class="tt"><span class="ellipsis nm">{t.title}</span><span class="ellipsis faint">{t.album}</span></span>
              <span class="num faint">{fmtTime(t.durationMs)}</span>
            </button>
          {/each}
        </div>
      </section>
    {/if}

    {#if mine.length}
      <section>
        <h3>В вашей библиотеке</h3>
        <div class="mine"><TrackList tracks={mine} /></div>
      </section>
    {/if}

    {#if page?.albums.length}
      <section>
        <div class="sh">
          <h3>Дискография</h3>
          {#if kinds.length > 1}
            <div class="seg">{#each kinds as k}<button class:on={albumKind === k} onclick={() => (albumKind = k)}>{kindLabel(k)}</button>{/each}</div>
          {/if}
        </div>
        <div class="grid">
          {#each shownAlbums as a (a.remoteId)}
            <button class="tile" onclick={() => openRemoteAlbum(`${a.source}:${a.remoteId}`, a.title)}>
              {#if a.coverUrl}<img src={a.coverUrl} alt="" loading="lazy" />{:else}<span class="cv big"></span>{/if}
              <span class="ellipsis nm">{a.title}</span>
              <span class="faint small">{a.year ?? ""}{a.trackCount ? ` · ${a.trackCount} ${plural(a.trackCount, "трек", "трека", "треков")}` : ""}</span>
            </button>
          {/each}
        </div>
      </section>
    {:else if myAlbums.length}
      <section>
        <h3>Альбомы</h3>
        <div class="grid">
          {#each myAlbums as a (a.key)}
            <button class="tile" onclick={() => { app.view = "albums"; app.focus = { kind: "album", key: a.key }; }}>
              {#if coverUrl(a.cover)}<img src={coverUrl(a.cover)} alt="" loading="lazy" />{:else}<span class="cv big"></span>{/if}
              <span class="ellipsis nm">{a.title}</span>
              <span class="faint small">{a.year ?? ""}</span>
            </button>
          {/each}
        </div>
      </section>
    {/if}

    {#if page?.description}
      <section class="bio">
        <h3>Об исполнителе</h3>
        <p class:clamp={!showAllBio}>{page.description}</p>
        {#if page.description.length > 400}<button class="faint" onclick={() => (showAllBio = !showAllBio)}>{showAllBio ? "Свернуть" : "Читать полностью"}</button>{/if}
      </section>
    {/if}

    {#if page?.similar.length}
      <section>
        <h3>Похожие</h3>
        <div class="similar">
          {#each page.similar as s (s.remoteId)}
            <button class="sim" onclick={() => openArtist(s.name, `${s.source}:${s.remoteId}`)}>
              {#if s.imageUrl}<img src={s.imageUrl} alt="" loading="lazy" />{:else}<span class="cv round"></span>{/if}
              <span class="ellipsis">{s.name}</span>
            </button>
          {/each}
        </div>
      </section>
    {/if}
  </div>
</div>

<style>
  .ap { height: 100%; overflow-y: auto; }
  .hero { position: relative; overflow: hidden; padding: 56px 36px 30px; }
  .bg { position: absolute; inset: -40%; width: 180%; height: 180%; object-fit: cover; mask-image: linear-gradient(180deg, #000 45%, transparent 62%); filter: blur(70px) saturate(1.4) brightness(0.55); opacity: 0.55; pointer-events: none; }
  :root[data-theme="mist"] .bg, :root[data-theme="paper"] .bg { filter: blur(80px) saturate(1.2) brightness(1.1); opacity: 0.35; }
  :root[data-theme="paper"] .bg { display: none; }
  .back { position: absolute; top: 16px; left: 22px; z-index: 2; width: 34px; height: 34px; border-radius: 50%; display: grid; place-items: center; background: rgb(0 0 0 / 0.25); color: var(--text); }
  :root[data-theme="mist"] .back, :root[data-theme="paper"] .back { background: var(--surface); }
  .hero-in { position: relative; display: flex; align-items: flex-end; gap: 30px; }
  .ph { width: 200px; height: 200px; border-radius: 50%; object-fit: cover; box-shadow: 0 30px 60px -28px rgb(0 0 0 / 0.8); flex: none; }
  .ph.empty { display: grid; place-items: center; background: var(--surface-2); font-size: 72px; font-weight: 800; color: var(--text-3); }
  :root[data-theme="paper"] .ph { border-radius: 0; filter: grayscale(1); box-shadow: none; }
  .meta { min-width: 0; display: flex; flex-direction: column; gap: 6px; }
  .eyebrow { display: flex; align-items: center; gap: 6px; }
  h1 { margin: 0; font-family: var(--font-display); font-size: clamp(36px, 5vw, 76px); font-weight: 800; letter-spacing: -0.04em; line-height: 0.98; }
  :root[data-theme="paper"] h1 { font-style: italic; }
  .facts { display: flex; flex-wrap: wrap; gap: 4px 16px; font-size: 13px; }
  .acts { display: flex; gap: 10px; margin-top: 12px; }
  .body { padding: 8px 36px 50px; display: flex; flex-direction: column; gap: 34px; }
  section h3 { margin: 0 0 12px; font-family: var(--font-display); font-size: 19px; font-weight: 700; letter-spacing: -0.01em; }
  .sh { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 12px; }
  .sh h3 { margin: 0; }
  .seg { display: flex; padding: 3px; border-radius: 999px; background: var(--surface); }
  .seg button { height: 28px; padding: 0 12px; border-radius: 999px; font-size: 12.5px; color: var(--text-2); }
  .seg button.on { background: var(--surface-2); color: var(--text); }
  .top { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 2px 18px; }
  .tr { display: grid; grid-template-columns: 22px 44px 1fr auto; gap: 12px; align-items: center; padding: 7px 8px; border-radius: 12px; text-align: left; }
  .tr:hover { background: var(--hover); }
  .tr.cur .nm { color: var(--accent); }
  .tr img, .cv { width: 44px; height: 44px; border-radius: 8px; object-fit: cover; background: var(--surface-2); display: block; }
  .tt { display: flex; flex-direction: column; min-width: 0; font-size: 12px; }
  .nm { font-weight: 600; font-size: 13.5px; }
  .n { text-align: center; font-size: 12px; }
  .mine { height: min(60vh, calc(56px * 8 + 40px)); margin: 0 -14px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); gap: 22px 18px; }
  .tile { display: flex; flex-direction: column; gap: 3px; text-align: left; min-width: 0; }
  .tile img, .cv.big { width: 100%; aspect-ratio: 1; height: auto; border-radius: 14px; margin-bottom: 7px; object-fit: cover; transition: transform 0.3s var(--ease-spring), box-shadow 0.3s; }
  .tile:hover img { transform: translateY(-3px); box-shadow: 0 18px 32px -18px rgb(0 0 0 / 0.7); }
  :root[data-theme="paper"] .tile img { border-radius: 0; filter: grayscale(1); }
  .small { font-size: 12px; }
  .bio p { margin: 0 0 8px; max-width: 760px; line-height: 1.6; color: var(--text-2); white-space: pre-line; }
  .bio p.clamp { display: -webkit-box; -webkit-line-clamp: 5; line-clamp: 5; -webkit-box-orient: vertical; overflow: hidden; }
  .bio button { font-size: 13px; }
  .similar { display: grid; grid-auto-flow: column; grid-auto-columns: 120px; gap: 16px; overflow-x: auto; padding-bottom: 6px; }
  .sim { display: flex; flex-direction: column; align-items: center; gap: 6px; font-size: 13px; font-weight: 600; min-width: 0; }
  .sim img, .cv.round { width: 110px; height: 110px; border-radius: 50%; object-fit: cover; }
  button:disabled { opacity: 0.4; pointer-events: none; }
</style>
