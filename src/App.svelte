<script lang="ts">
  import { fly } from "svelte/transition";
  import { app, player, toggleFullscreen, setFullscreen } from "./lib/store.svelte";
  import Background from "./lib/components/Background.svelte";
  import TitleBar from "./lib/components/TitleBar.svelte";
  import Sidebar from "./lib/components/Sidebar.svelte";
  import PlayerBar from "./lib/components/PlayerBar.svelte";
  import Aside from "./lib/components/Aside.svelte";
  import Queue from "./lib/components/Queue.svelte";
  import LyricsView from "./lib/components/LyricsView.svelte";
  import Home from "./lib/views/Home.svelte";
  import Library from "./lib/views/Library.svelte";
  import Albums from "./lib/views/Albums.svelte";
  import Stats from "./lib/views/Stats.svelte";
  import SettingsView from "./lib/views/SettingsView.svelte";
  import PlaylistView from "./lib/views/PlaylistView.svelte";
  import ContextMenu from "./lib/components/ContextMenu.svelte";
  import TrackInfo from "./lib/components/TrackInfo.svelte";
  import ArtistPage from "./lib/views/ArtistPage.svelte";
  import RemoteAlbumPage from "./lib/views/RemoteAlbumPage.svelte";

  function keys(e: KeyboardEvent) {
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA") return;
    if (e.key === "F11") { e.preventDefault(); toggleFullscreen(); return; }
    if (e.key === "Escape" && app.fullscreen && !app.lyricsOpen) { setFullscreen(false); return; }
    if (e.code === "Space") { e.preventDefault(); player.toggle(); }
    else if (!e.ctrlKey && e.key === "ArrowRight") { e.preventDefault(); player.seekBy(e.shiftKey ? 15000 : 5000); }
    else if (!e.ctrlKey && e.key === "ArrowLeft") { e.preventDefault(); player.seekBy(e.shiftKey ? -15000 : -5000); }
    else if (e.ctrlKey && e.key === "ArrowRight") player.next();
    else if (e.ctrlKey && e.key === "ArrowLeft") player.prev();
    else if (e.key.toLowerCase() === "l" && !e.ctrlKey) app.lyricsOpen = !app.lyricsOpen;
    else if (e.key.toLowerCase() === "q" && !e.ctrlKey) app.queueOpen = !app.queueOpen;
  }
</script>

<svelte:window onkeydown={keys} />

<Background />

<div class="shell" class:home={app.view === "home"} class:fs={app.fullscreen}>
  <TitleBar />
  <Sidebar />
  <main class="content panel">
    {#key app.view + (app.focus?.key ?? "") + (app.focus?.name ?? "")}
      <div class="view" in:fly={{ y: 10, duration: 380, opacity: 0 }}>
        {#if app.view === "home"}<Home />
        {:else if app.view === "library"}<Library />
        {:else if app.view === "liked"}<Library liked />
        {:else if app.view === "albums"}<Albums mode="albums" />
        {:else if app.view === "artists"}<Albums mode="artists" />
        {:else if app.view === "stats"}<Stats />
        {:else if app.view === "settings"}<SettingsView />
        {:else if app.view === "playlist"}<PlaylistView />
        {:else if app.view === "artist"}<ArtistPage />
        {:else if app.view === "remote-album"}<RemoteAlbumPage />
        {/if}
      </div>
    {/key}
    {#if app.queueOpen}<Queue />{/if}
  </main>
  <Aside />
  <PlayerBar />
</div>

{#if app.lyricsOpen}<LyricsView />{/if}
{#if app.menu}<ContextMenu />{/if}
{#if app.info}<TrackInfo />{/if}

{#if app.toast}
  {#key app.toast.at}
    <div class="toast" transition:fly={{ y: 16, duration: 300 }}>{app.toast.text}</div>
  {/key}
{/if}

<style>
  .shell {
    position: relative;
    z-index: 1;
    height: 100vh;
    display: grid;
    grid-template-columns: var(--sidebar-w) minmax(0, 1fr) var(--aside-w);
    grid-template-rows: var(--title-h) minmax(0, 1fr) var(--bar-h);
    grid-template-areas: "title title title" "side main aside" "bar bar bar";
    column-gap: var(--gap);
    padding: 0 var(--gap);
  }
  :root[data-theme="paper"] .shell, .shell.home { grid-template-columns: var(--sidebar-w) minmax(0, 1fr) 0; }
  .shell.home > :global(.aside) { display: none; }
  .shell.fs { grid-template-rows: 0 minmax(0, 1fr) var(--bar-h); }
  .shell.fs > :global(.title) { overflow: hidden; }

  /* Aura: no boxes. Sidebar and content float on the living background; the player bar
     is the only solid-ish object, a glass capsule. */
  :root[data-theme="aura"] .content { background: transparent; }
  :root[data-theme="aura"] :global(.side) { background: transparent; margin-bottom: var(--gap); }
  :root[data-theme="aura"] :global(.aside) { margin-bottom: var(--gap); }
  :root[data-theme="mist"] .shell, :root[data-theme="liquid"] .shell { padding-bottom: 0; }
  .shell > :global(.title) { margin: 0 calc(var(--gap) * -1); }
  .shell > :global(.bar) { margin: 0 calc(var(--gap) * -1); }
  .content { grid-area: main; position: relative; overflow: hidden; min-height: 0; margin-bottom: var(--gap); }
  :root[data-theme="noir"] .content { margin: 0; background: var(--bg); }
  :root[data-theme="mist"] .content { background: rgb(255 255 255 / 0.3); }
  :root[data-theme="liquid"] .content { background: rgb(8 10 16 / 0.35); backdrop-filter: blur(40px) saturate(1.3); border-radius: 30px; box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.1), inset 0 0 0 1px rgb(255 255 255 / 0.06); }
  :root[data-theme="mist"] :global(.side), :root[data-theme="liquid"] :global(.side) { margin-bottom: var(--gap); }
  :root[data-theme="liquid"] :global(.side) { background: rgb(8 10 16 / 0.3); backdrop-filter: blur(40px); border-radius: 30px; box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.1); }
  :root[data-theme="mist"] :global(.aside), :root[data-theme="liquid"] :global(.aside) { margin-bottom: var(--gap); }
  .view { position: absolute; inset: 0; }

  .toast {
    position: fixed; left: 50%; bottom: calc(var(--bar-h) + 18px); transform: translateX(-50%); z-index: 60;
    padding: 10px 16px; border-radius: 12px; font-size: 13px;
    background: var(--text); color: var(--bg);
    box-shadow: 0 16px 40px -16px rgb(0 0 0 / 0.5);
  }
</style>
