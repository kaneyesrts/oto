<script lang="ts">
  // Home "stage": the current song, given the room. Cover breathes with the bass,
  // the lyric line sings in place, everything is one click from the full lyrics view.
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import { app, frame, player, toggleFullscreen, openArtist } from "../store.svelte";
  import { coverUrl } from "../api";
  import { sourceLabel } from "../lyrics";
  import LiveLine from "./LiveLine.svelte";
  import Seek from "./Seek.svelte";
  import Spectrum from "./Spectrum.svelte";
  import { Play, Pause, SkipBack, SkipForward, Heart, MicVocal, Maximize } from "../icons";

  const t = $derived(app.player.track!);
  const cover = $derived(coverUrl(t?.cover));
  const timed = $derived(app.lyrics?.kind === "synced" || app.lyrics?.kind === "estimated");
  let art: HTMLButtonElement;

  onMount(() => {
    let raf = 0;
    const loop = () => {
      raf = requestAnimationFrame(loop);
      if (!art) return;
      const k = app.settings?.visual.intensity ?? 1;
      art.style.setProperty("--pulse", (1 + frame.bass * 0.022 * k + frame.beat * 0.01 * k).toFixed(4));
      art.style.setProperty("--glow", (0.3 + frame.lvl * 0.55).toFixed(3));
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  });
</script>

<section class="stage">
  <button class="art" bind:this={art} onclick={() => (app.lyricsOpen = true)} aria-label="Открыть текст">
    {#key cover}
      {#if cover}<img class="cover" src={cover} alt="" in:fade={{ duration: 600 }} />{:else}<div class="cover"></div>{/if}
    {/key}
  </button>

  <div class="info">
    <div class="eyebrow">{app.player.playing ? "Сейчас играет" : "На паузе"}{t.album ? ` · ${t.album}` : ""}</div>
    {#key t.id}
      <h1 class="title" in:fade={{ duration: 400 }}>{t.title}</h1>
    {/key}
    <button class="artist" onclick={() => openArtist((t.albumArtist || t.artist).split(",")[0].trim(), t.artistRef || undefined)}>{t.artist}</button>

    <button class="lyric" onclick={() => (app.lyricsOpen = true)}>
      {#if timed}
        <LiveLine size={28} />
      {:else}
        <div class="wave"><Spectrum bars={48} mode="wave" color="currentColor" /></div>
      {/if}
    </button>

    <div class="seek"><Seek /></div>

    <div class="ctl">
      <button class="icon-btn" class:on={t.liked} onclick={() => player.like()} aria-label="В избранное"><Heart size={19} fill={t.liked ? "currentColor" : "none"} /></button>
      <button class="icon-btn big" onclick={player.prev} aria-label="Назад"><SkipBack size={22} fill="currentColor" /></button>
      <button class="play" onclick={player.toggle} aria-label="Играть/пауза">
        {#if app.player.playing}<Pause size={24} fill="currentColor" strokeWidth={0} />{:else}<Play size={24} fill="currentColor" strokeWidth={0} />{/if}
      </button>
      <button class="icon-btn big" onclick={player.next} aria-label="Вперёд"><SkipForward size={22} fill="currentColor" /></button>
      <span class="sp"></span>
      <button class="chip" onclick={() => (app.lyricsOpen = true)}><MicVocal size={15} /> Текст</button>
      <button class="chip" onclick={() => { app.lyricsOpen = true; toggleFullscreen(); }}><Maximize size={15} /> На весь экран</button>
    </div>
    <div class="src faint">
      {#if app.player.format}<span class="fmt">{app.player.format}</span>{/if}
      {#if app.lyrics?.source}<span>текст: {sourceLabel(app.lyrics)}</span>{/if}
    </div>
  </div>
</section>

<style>
  .stage {
    display: grid;
    grid-template-columns: minmax(220px, 340px) minmax(0, 1fr);
    gap: clamp(28px, 4vw, 56px);
    align-items: center;
    padding: 8px 0 4px;
  }
  .art { position: relative; --pulse: 1; --glow: 0.35; transform: scale(var(--pulse)); will-change: transform; }
  .art .cover {
    width: 100%;
    border-radius: 18px;
    box-shadow: 0 40px 80px -36px rgb(0 0 0 / calc(var(--glow) + 0.3)), 0 0 0 1px rgb(255 255 255 / 0.06);
    transition: transform 0.4s var(--ease-spring);
  }
  .art:hover .cover { transform: translateY(-3px); }
  .art::after {
    content: ""; position: absolute; inset: 10%; z-index: -1; border-radius: 30%;
    background: rgb(var(--accent-rgb, var(--accent-default)));
    filter: blur(60px); opacity: calc(var(--glow) * 0.75);
  }
  :root[data-theme="paper"] .art .cover { border-radius: 0; box-shadow: none; filter: grayscale(1) contrast(1.05); }
  :root[data-theme="paper"] .art::after { display: none; }
  :root[data-theme="mist"] .art::after { opacity: calc(var(--glow) * 0.45); }

  .info { min-width: 0; display: flex; flex-direction: column; }
  .title {
    margin: 8px 0 4px;
    font-family: var(--font-display);
    font-size: clamp(34px, 4.4vw, 68px);
    font-weight: 800;
    line-height: 0.98;
    letter-spacing: -0.04em;
    display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;
  }
  :root[data-theme="noir"] .title { font-weight: 600; letter-spacing: -0.02em; }
  :root[data-theme="paper"] .title { font-style: italic; font-size: clamp(44px, 5.4vw, 84px); }
  .artist { align-self: flex-start; font-size: 17px; font-weight: 600; color: var(--text-2); transition: color 0.15s; }
  .artist:hover { color: var(--text); }
  .lyric { text-align: left; margin: 26px 0 18px; min-height: 76px; color: var(--text); }
  :root[data-theme="paper"] .lyric :global(.cur) { font-family: var(--font-lyrics); font-style: italic; font-weight: 600; }
  .wave { height: 56px; color: var(--accent); opacity: 0.8; }
  .seek { max-width: 560px; }
  .ctl { display: flex; align-items: center; gap: 8px; margin-top: 10px; }
  .icon-btn.big { width: 44px; height: 44px; color: var(--text); }
  .play { width: 58px; height: 58px; border-radius: 50%; display: grid; place-items: center; background: var(--text); color: var(--bg); transition: transform 0.25s var(--ease-spring), box-shadow 0.2s; }
  .play:hover { transform: scale(1.06); box-shadow: 0 12px 30px -12px var(--text); }
  :root[data-theme="aura"] .play { color: #0b0b0e; }
  :root[data-theme="paper"] .play { border-radius: 0; }
  :root[data-theme="mist"] .play { background: var(--accent); color: #fff; }
  .sp { flex: 1; }
  .chip { display: inline-flex; align-items: center; gap: 7px; height: 34px; padding: 0 14px; border-radius: 999px; font-size: 13px; font-weight: 600; background: var(--surface); color: var(--text-2); transition: background 0.15s, color 0.15s; }
  .chip:hover { background: var(--surface-2); color: var(--text); }
  :root[data-theme="paper"] .chip { border-radius: 0; border: 1px solid var(--line); background: none; }
  .src { margin-top: 12px; font-size: 11px; display: flex; gap: 10px; align-items: center; }
  .fmt { font-family: var(--font-num); letter-spacing: 0.04em; padding: 2px 6px; border-radius: 5px; border: 1px solid var(--line); color: var(--text-2); }

  @media (max-width: 1100px) {
    .stage { grid-template-columns: 220px 1fr; }
    .chip { padding: 0 10px; }
  }
</style>
