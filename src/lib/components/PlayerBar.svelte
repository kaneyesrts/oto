<script lang="ts">
  import { app, player, toggleFullscreen } from "../store.svelte";
  import { coverUrl } from "../api";
  import { glass } from "../glass";
  import Seek from "./Seek.svelte";
  import Spectrum from "./Spectrum.svelte";
  import { Play, Pause, SkipBack, SkipForward, Shuffle, Repeat, Repeat1, Heart, Volume2, Volume1, VolumeX, ListMusic, MicVocal, Maximize, Minimize } from "../icons";

  const t = $derived(app.player.track);
  const liquid = $derived(app.settings?.theme === "liquid");
  let muted = 0;

  function wheel(e: WheelEvent) {
    e.preventDefault();
    player.volume(Math.min(1, Math.max(0, app.player.volume - Math.sign(e.deltaY) * 0.04)));
  }
  function mute() {
    if (app.player.volume > 0) { muted = app.player.volume; player.volume(0); }
    else player.volume(muted || 0.6);
  }
  let volEl: HTMLDivElement;
  function volDrag(e: PointerEvent) {
    if (e.type === "pointerdown") volEl.setPointerCapture(e.pointerId);
    if (e.buttons !== 1 && e.type !== "pointerdown") return;
    const r = volEl.getBoundingClientRect();
    player.volume(Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)));
  }
</script>

<footer class="bar" class:liquid>
  <div class="inner" use:glass={{ radius: 30, bezel: 26, strength: 34, enabled: liquid }}>
    <div class="now">
      {#if t}
        <button class="art" onclick={() => (app.lyricsOpen = true)} aria-label="Открыть лирику">
          {#if coverUrl(t.cover)}<img class="cover" src={coverUrl(t.cover)} alt="" />{:else}<div class="cover"></div>{/if}
        </button>
        <div class="meta">
          <div class="name ellipsis">{t.title}</div>
          <div class="by ellipsis">{t.artist}</div>
        </div>
        <button class="icon-btn" class:on={t.liked} onclick={() => player.like()} aria-label="В избранное">
          <Heart size={17} fill={t.liked ? "currentColor" : "none"} />
        </button>
      {:else}
        <div class="meta"><div class="by">Выберите трек</div></div>
      {/if}
    </div>

    <div class="center">
      <div class="ctrls">
        <button class="icon-btn small" class:on={app.player.shuffle} onclick={player.shuffle} aria-label="Перемешать"><Shuffle size={16} /></button>
        <button class="icon-btn" onclick={player.prev} aria-label="Назад"><SkipBack size={19} fill="currentColor" /></button>
        <button class="play" onclick={player.toggle} aria-label={app.player.playing ? "Пауза" : "Играть"}>
          {#if app.player.playing}<Pause size={20} fill="currentColor" strokeWidth={0} />{:else}<Play size={20} fill="currentColor" strokeWidth={0} />{/if}
        </button>
        <button class="icon-btn" onclick={player.next} aria-label="Вперёд"><SkipForward size={19} fill="currentColor" /></button>
        <button class="icon-btn small" class:on={app.player.repeat !== "off"} onclick={player.repeat} aria-label="Повтор">
          {#if app.player.repeat === "one"}<Repeat1 size={16} />{:else}<Repeat size={16} />{/if}
        </button>
      </div>
      <Seek />
    </div>

    <div class="right">
      <div class="viz"><Spectrum bars={18} gap={2} /></div>
      <button class="icon-btn" class:on={app.lyricsOpen} onclick={() => (app.lyricsOpen = !app.lyricsOpen)} aria-label="Лирика"><MicVocal size={18} /></button>
      <button class="icon-btn" onclick={toggleFullscreen} aria-label="Полный экран (F11)" title="Полный экран (F11)">
        {#if app.fullscreen}<Minimize size={17} />{:else}<Maximize size={17} />{/if}
      </button>
      <button class="icon-btn" class:on={app.queueOpen} onclick={() => (app.queueOpen = !app.queueOpen)} aria-label="Очередь"><ListMusic size={18} /></button>
      <div class="vol" onwheel={wheel}>
        <button class="icon-btn" onclick={mute} aria-label="Звук">
          {#if app.player.volume === 0}<VolumeX size={18} />{:else if app.player.volume < 0.5}<Volume1 size={18} />{:else}<Volume2 size={18} />{/if}
        </button>
        <div class="vtrack" bind:this={volEl} onpointerdown={volDrag} onpointermove={volDrag} role="slider" tabindex="0" aria-label="Громкость" aria-valuenow={Math.round(app.player.volume * 100)}>
          <div class="vrail"><div class="vfill" style="width:{app.player.volume * 100}%"></div></div>
        </div>
      </div>
    </div>
  </div>
</footer>

<style>
  .bar { grid-area: bar; position: relative; z-index: 4; min-width: 0; }
  .inner {
    height: var(--bar-h);
    display: grid;
    grid-template-columns: minmax(220px, 1fr) minmax(360px, 1.4fr) minmax(220px, 1fr);
    align-items: center;
    gap: 24px;
    padding: 0 20px;
  }
  :root[data-theme="noir"] .inner { border-top: 1px solid var(--line); background: var(--bg-2); }
  :root[data-theme="mist"] .inner { margin: 0 var(--gap) var(--gap); height: calc(var(--bar-h) - var(--gap)); border-radius: var(--radius-xl); background: rgb(255 255 255 / 0.42); backdrop-filter: blur(30px) saturate(1.4); border: 1px solid rgb(255 255 255 / 0.65); box-shadow: 0 20px 50px -30px rgb(120 40 70 / 0.5); }
  :root[data-theme="paper"] .inner { border-top: 1px solid var(--line); }
  :root[data-theme="aura"] .inner {
    margin: 0 var(--gap) var(--gap); height: calc(var(--bar-h) - var(--gap)); border-radius: 26px;
    background: rgb(12 12 16 / 0.38); backdrop-filter: blur(40px) saturate(1.5);
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.1), inset 0 0 0 1px rgb(255 255 255 / 0.05), 0 24px 60px -30px rgb(0 0 0 / 0.7);
  }
  :root[data-theme="aura"] .play { background: #fff; color: #0b0b0e; }
  .liquid .inner { margin: 0 var(--gap) var(--gap); height: calc(var(--bar-h) - var(--gap)); border-radius: 30px; }

  .now { display: flex; align-items: center; gap: 14px; min-width: 0; }
  .art { flex: none; }
  .art .cover { width: 54px; height: 54px; border-radius: 10px; box-shadow: 0 8px 24px -10px rgb(0 0 0 / 0.6); transition: transform 0.3s var(--ease-spring); }
  .art:hover .cover { transform: scale(1.06); }
  :root[data-theme="paper"] .art .cover { border-radius: 0; filter: grayscale(1) contrast(1.1); }
  .meta { min-width: 0; flex: 0 1 auto; }
  .name { font-weight: 600; font-size: 14px; }
  .by { color: var(--text-2); font-size: 12.5px; margin-top: 2px; }
  :root[data-theme="paper"] .name { font-family: var(--font-display); font-size: 17px; font-weight: 600; }

  .center { display: flex; flex-direction: column; align-items: center; gap: 4px; min-width: 0; }
  .ctrls { display: flex; align-items: center; gap: 8px; }
  .icon-btn.small { width: 32px; height: 32px; }
  .play {
    width: 42px; height: 42px; border-radius: 50%;
    display: grid; place-items: center;
    background: var(--text); color: var(--bg);
    transition: transform 0.25s var(--ease-spring), box-shadow 0.2s;
  }
  .play:hover { transform: scale(1.07); }
  .play:active { transform: scale(0.94); }
  :root[data-theme="noir"] .play { background: transparent; color: var(--text); border: 1.5px solid color-mix(in srgb, var(--text) 25%, transparent); }
  :root[data-theme="mist"] .play { background: var(--accent); color: #fff; box-shadow: 0 10px 24px -10px var(--accent); }
  :root[data-theme="liquid"] .play { background: rgb(255 255 255 / 0.92); color: #0b0b10; }
  :root[data-theme="paper"] .play { border-radius: 0; background: var(--text); color: var(--bg); }

  .right { display: flex; align-items: center; justify-content: flex-end; gap: 4px; min-width: 0; }
  .viz { width: 64px; height: 22px; margin-right: 8px; opacity: 0.9; }
  .vol { display: flex; align-items: center; gap: 2px; }
  .vtrack { width: 96px; height: 18px; display: flex; align-items: center; cursor: pointer; touch-action: none; }
  .vrail { width: 100%; height: 4px; border-radius: 4px; background: color-mix(in srgb, var(--text) 12%, transparent); overflow: hidden; }
  .vfill { height: 100%; background: var(--text); border-radius: inherit; }
  .vtrack:hover .vfill { background: var(--accent); }
</style>
