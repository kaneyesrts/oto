<script lang="ts">
  import { app, libraryTracks, player, openArtist, wantArtistImages } from "../store.svelte";
  import { coverUrl, fmtDuration, plural } from "../api";
  import { albums, artists } from "../groups";
  import TrackList from "../components/TrackList.svelte";
  import { Play, Shuffle, ChevronLeft } from "../icons";

  let { mode = "albums" }: { mode?: "albums" | "artists" } = $props();

  const all = $derived(albums(libraryTracks()));
  const arts = $derived(artists(libraryTracks()));
  const album = $derived(app.focus?.kind === "album" ? all.find((a) => a.key === app.focus!.key) : null);
  const artist = $derived(app.focus?.kind === "artist" ? arts.find((a) => a.name === app.focus!.key) : null);
  const artistAlbums = $derived(artist ? all.filter((a) => a.artist === artist.name) : []);
  $effect(() => { if (mode === "artists") wantArtistImages(arts.map((a) => a.name)); });
  const photo = (name: string, fallback?: string | null) => app.artistImages[name] ? coverUrl(app.artistImages[name]) : coverUrl(fallback);

  function playShuffled(ids: string[]) {
    const s = [...ids];
    for (let i = s.length - 1; i > 0; i--) { const j = Math.floor(Math.random() * (i + 1)); [s[i], s[j]] = [s[j], s[i]]; }
    player.playList(s, 0);
  }
</script>

{#if album}
  <div class="detail">
    <button class="back faint" onclick={() => (app.focus = null)}><ChevronLeft size={16} /> Назад</button>
    <header class="dh">
      {#if coverUrl(album.cover)}<img class="cover big" src={coverUrl(album.cover)} alt="" />{:else}<div class="cover big"></div>{/if}
      <div class="dm">
        <div class="eyebrow">Альбом{album.year ? ` · ${album.year}` : ""}</div>
        <h1>{album.title}</h1>
        <button class="artist" onclick={() => openArtist(album.artist)}>{album.artist}</button>
        <div class="muted">{album.tracks.length} {plural(album.tracks.length, "трек", "трека", "треков")} · {fmtDuration(album.durationMs)}</div>
        <div class="acts">
          <button class="pill solid" onclick={() => player.playList(album.tracks.map((t) => t.id), 0)}><Play size={15} fill="currentColor" strokeWidth={0} /> Слушать</button>
          <button class="pill ghost" onclick={() => playShuffled(album.tracks.map((t) => t.id))}><Shuffle size={15} /> Перемешать</button>
        </div>
      </div>
    </header>
    <div class="tl"><TrackList tracks={album.tracks} showAlbum={false} numbered /></div>
  </div>
{:else if artist}
  <div class="detail">
    <button class="back faint" onclick={() => (app.focus = null)}><ChevronLeft size={16} /> Назад</button>
    <header class="dh">
      {#if coverUrl(artist.cover)}<img class="cover big round" src={coverUrl(artist.cover)} alt="" />{:else}<div class="cover big round"></div>{/if}
      <div class="dm">
        <div class="eyebrow">Артист</div>
        <h1>{artist.name}</h1>
        <div class="muted">{artist.albums} {plural(artist.albums, "альбом", "альбома", "альбомов")} · {artist.tracks.length} {plural(artist.tracks.length, "трек", "трека", "треков")}</div>
        <div class="acts">
          <button class="pill solid" onclick={() => player.playList(artist.tracks.map((t) => t.id), 0)}><Play size={15} fill="currentColor" strokeWidth={0} /> Слушать</button>
          <button class="pill ghost" onclick={() => playShuffled(artist.tracks.map((t) => t.id))}><Shuffle size={15} /> Перемешать</button>
        </div>
      </div>
    </header>
    {#if artistAlbums.length > 1}
      <div class="mini">
        {#each artistAlbums as a (a.key)}
          <button class="mini-a" onclick={() => (app.focus = { kind: "album", key: a.key })}>
            {#if coverUrl(a.cover)}<img class="cover" src={coverUrl(a.cover)} alt="" loading="lazy" />{:else}<div class="cover"></div>{/if}
            <span class="ellipsis">{a.title}</span>
          </button>
        {/each}
      </div>
    {/if}
    <div class="tl"><TrackList tracks={artist.tracks} /></div>
  </div>
{:else}
  <div class="grid-wrap">
    <header class="gh">
      <div class="eyebrow">Библиотека</div>
      <h1>{mode === "albums" ? "Альбомы" : "Артисты"}</h1>
    </header>
    <div class="grid" class:people={mode === "artists"}>
      {#if mode === "albums"}
        {#each all as a (a.key)}
          <button class="tile" onclick={() => (app.focus = { kind: "album", key: a.key })}>
            {#if coverUrl(a.cover)}<img class="cover" src={coverUrl(a.cover)} alt="" loading="lazy" />{:else}<div class="cover"></div>{/if}
            <span class="tn ellipsis">{a.title}</span>
            <span class="ta ellipsis faint">{a.artist}{a.year ? ` · ${a.year}` : ""}</span>
          </button>
        {/each}
      {:else}
        {#each arts as a (a.name)}
          <button class="tile" onclick={() => openArtist(a.name)}>
            {#if photo(a.name, a.cover)}<img class="cover round" src={photo(a.name, a.cover)} alt="" loading="lazy" />{:else}<div class="cover round"></div>{/if}
            <span class="tn ellipsis">{a.name}</span>
            <span class="ta faint">{a.tracks.length} {plural(a.tracks.length, "трек", "трека", "треков")}</span>
          </button>
        {/each}
      {/if}
    </div>
  </div>
{/if}

<style>
  .grid-wrap, .detail { height: 100%; overflow-y: auto; padding: 28px 32px 40px; }
  .detail { display: flex; flex-direction: column; overflow: hidden; }
  h1 { margin: 6px 0 4px; font-family: var(--font-display); font-size: clamp(28px, 3vw, 44px); font-weight: 700; letter-spacing: -0.03em; line-height: 1.05; }
  :root[data-theme="noir"] h1 { font-weight: 500; }
  :root[data-theme="paper"] h1 { font-style: italic; font-size: clamp(40px, 5vw, 72px); }
  .gh { margin-bottom: 22px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(170px, 1fr)); gap: 26px 20px; }
  .tile { display: flex; flex-direction: column; gap: 3px; text-align: left; min-width: 0; }
  .tile .cover { width: 100%; border-radius: 14px; margin-bottom: 8px; transition: transform 0.35s var(--ease-spring), box-shadow 0.3s; }
  .tile:hover .cover { transform: translateY(-4px); box-shadow: 0 20px 36px -20px rgb(0 0 0 / 0.7); }
  .round { border-radius: 50% !important; }
  :root[data-theme="paper"] .tile .cover { border-radius: 0; filter: grayscale(1); }
  :root[data-theme="paper"] .tile:hover .cover { filter: none; }
  .tn { font-weight: 600; font-size: 13.5px; max-width: 100%; }
  :root[data-theme="paper"] .tn { font-family: var(--font-display); font-size: 16px; }
  .ta { font-size: 12px; }
  .people .tile { align-items: center; text-align: center; }

  .back { display: inline-flex; align-items: center; gap: 4px; font-size: 13px; margin-bottom: 14px; align-self: flex-start; }
  .back:hover { color: var(--text); }
  .dh { display: flex; gap: 28px; align-items: flex-end; margin-bottom: 22px; }
  .cover.big { width: 200px; height: 200px; border-radius: 18px; box-shadow: 0 30px 60px -30px rgb(0 0 0 / 0.8); flex: none; }
  :root[data-theme="paper"] .cover.big { border-radius: 0; box-shadow: none; }
  .dm { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .artist { align-self: flex-start; font-weight: 600; color: var(--accent); }
  .acts { display: flex; gap: 10px; margin-top: 14px; }
  .mini { display: flex; gap: 14px; overflow-x: auto; padding-bottom: 14px; flex: none; }
  .mini-a { width: 110px; flex: none; display: flex; flex-direction: column; gap: 6px; font-size: 12px; text-align: left; }
  .mini-a .cover { width: 110px; border-radius: 10px; }
  .tl { flex: 1; min-height: 0; margin: 0 -14px; }
</style>
