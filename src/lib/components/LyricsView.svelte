<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade } from "svelte/transition";
  import { app, frame, posNow, player, loadLyrics, toggleFullscreen, setFullscreen } from "../store.svelte";
  import { api, coverUrl } from "../api";
  import { mesh } from "../gl/mesh";
  import { particles } from "../gl/particles";
  import { shaderPalette, accentFrom } from "../color";
  import { activeIndex, lyricTime, wordsOf, sourceLabel } from "../lyrics";
  import Seek from "./Seek.svelte";
  import { Play, Pause, SkipBack, SkipForward, Heart, ChevronLeft, Refresh, Minimize, Maximize } from "../icons";

  const t = $derived(app.player.track);
  const doc = $derived(app.lyrics);
  const lines = $derived(doc?.lines ?? []);
  const timed = $derived(doc?.kind === "synced" || doc?.kind === "estimated");
  const theme = $derived(app.settings?.theme ?? "noir");
  const words = $derived(lines.map(wordsOf));

  let bgCanvas: HTMLCanvasElement | undefined = $state();
  let fxCanvas: HTMLCanvasElement | undefined = $state();
  let scroller: HTMLDivElement | undefined = $state();
  let column: HTMLDivElement | undefined = $state();
  let artEl: HTMLDivElement | undefined = $state();
  let lineEls: HTMLElement[] = $state([]);
  let active = $state(-1);
  let manual = $state(0);
  let manualTimer = 0;
  let idle = $state(false);
  let idleTimer = 0;
  function close() {
    if (app.fullscreen) setFullscreen(false);
    app.lyricsOpen = false;
  }
  let offsets = $state<number[]>([]);
  let viewH = $state(600);
  let viewW = $state(800);

  onMount(() => {
    loadLyrics();
    const stopBg = bgCanvas
      ? mesh(bgCanvas, {
          palette: () => shaderPalette(app.player.track?.palette, [[40, 40, 60], [80, 50, 70], [30, 50, 70], [70, 40, 50]]),
          dark: () => theme !== "mist",
          intensity: () => app.settings?.visual.intensity ?? 1,
          scale: 0.4,
        })
      : () => {};
    const stopFx = fxCanvas
      ? particles(fxCanvas, {
          count: app.settings?.visual.quality === "low" ? 110 : 260,
          enabled: () => !!app.settings?.visual.particles && app.player.playing,
          intensity: () => app.settings?.visual.intensity ?? 1,
          color: () => {
            const a = accentFrom(app.player.track?.palette ?? [], true) ?? [255, 200, 180];
            const m = theme === "mist" ? 1 : 0.85;
            return [(a[0] / 255) * 0.5 + 0.5 * m, (a[1] / 255) * 0.5 + 0.5 * m, (a[2] / 255) * 0.5 + 0.5 * m];
          },
          focus: () => [0.27, 0.45],
        })
      : () => {};

    let raf = 0;
    const loop = () => {
      raf = requestAnimationFrame(loop);
      // Cover breathes with the bass.
      if (artEl) {
        const s = 1 + frame.bass * 0.025 * (app.settings?.visual.intensity ?? 1) + frame.beat * 0.012;
        artEl.style.setProperty("--pulse", s.toFixed(4));
        artEl.style.setProperty("--glow", (0.35 + frame.lvl * 0.5).toFixed(3));
      }
      if (!timed || !lines.length) return;
      const tm = lyricTime(posNow(), doc);
      const idx = activeIndex(lines, tm);
      if (idx !== active) active = idx;
      // Karaoke fill on the active line only; direct style writes, no re-render.
      const el = lineEls[idx];
      if (el && idx >= 0) {
        const spans = el.querySelectorAll<HTMLElement>("span.w");
        const ws = words[idx];
        let k = 0;
        for (const w of ws) {
          if (!w.text.trim()) continue;
          const p = Math.min(1, Math.max(0, (tm - w.t) / Math.max(80, w.d)));
          spans[k++]?.style.setProperty("--p", p.toFixed(3));
        }
      }
    };
    raf = requestAnimationFrame(loop);
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopImmediatePropagation();
      if (app.fullscreen) setFullscreen(false);
      else app.lyricsOpen = false;
    };
    // Immersive: in fullscreen the chrome and cursor fade out after a moment of stillness.
    const wake = () => {
      idle = false;
      clearTimeout(idleTimer);
      idleTimer = window.setTimeout(() => (idle = true), 2600);
    };
    wake();
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("pointermove", wake);
    return () => {
      cancelAnimationFrame(raf); stopBg(); stopFx(); clearTimeout(idleTimer);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("pointermove", wake);
    };
  });

  async function measure() {
    await tick();
    offsets = lineEls.map((e) => (e ? e.offsetTop + e.offsetHeight / 2 : 0));
  }
  // Re-measure when the text or the viewport changes (fullscreen, resize, font scale).
  $effect(() => { lines; viewH; viewW; app.settings?.lyrics.fontScale; measure(); });

  const shift = $derived.by(() => {
    if (!scroller || !offsets.length) return 0;
    const anchor = viewH * 0.4;
    const y = active >= 0 ? offsets[active] ?? 0 : (offsets[0] ?? 0) - 120;
    return anchor - y + manual;
  });

  function wheel(e: WheelEvent) {
    if (!timed) return;
    e.preventDefault();
    manual -= e.deltaY;
    clearTimeout(manualTimer);
    manualTimer = window.setTimeout(() => (manual = 0), 2500);
  }

  function seekTo(i: number) {
    if (!timed) return;
    manual = 0;
    player.seek(Math.max(0, lines[i].t - (doc?.userOffsetMs ?? 0)));
  }

  function nudge(ms: number) {
    if (!doc || !t) return;
    doc.userOffsetMs += ms;
    api.setLyricsOffset(t.id, doc.userOffsetMs);
  }

  function lineStyle(i: number) {
    const d = active < 0 ? i + 1 : i - active;
    const ad = Math.abs(d);
    const blur = app.settings?.lyrics.depthBlur && timed ? Math.min(ad * 1.1, 5) : 0;
    const op = !timed ? 0.9 : d === 0 ? 1 : d < 0 ? Math.max(0.16, 0.42 - ad * 0.05) : Math.max(0.14, 0.5 - ad * 0.07);
    const delay = d > 0 ? Math.min(d, 6) * 34 : 0;
    return `--blur:${blur}px;--op:${op};--delay:${delay}ms`;
  }
</script>

<section aria-label="Текст песни" class="lyr" class:immersive={app.fullscreen && idle} ondblclick={(e) => { if ((e.target as HTMLElement).closest(".line, button")) return; toggleFullscreen(); }} data-theme-v={theme} style="--fs:{app.settings?.lyrics.fontScale ?? 1}" transition:fade={{ duration: 260 }}>
  {#if theme !== "paper"}
    <canvas class="bg" bind:this={bgCanvas}></canvas>
    <canvas class="fx" bind:this={fxCanvas}></canvas>
  {/if}

  <header class="top" data-tauri-drag-region>
    <button class="icon-btn" onclick={close} aria-label="Закрыть"><ChevronLeft size={22} /></button>
    <div class="src faint" data-tauri-drag-region>{sourceLabel(doc)}</div>
    <button class="icon-btn" onclick={toggleFullscreen} aria-label="Полный экран" title="Полный экран (F11, двойной клик)">
      {#if app.fullscreen}<Minimize size={18} />{:else}<Maximize size={18} />{/if}
    </button>
  </header>

  <div class="stage">
    <aside class="left">
      <div class="art" bind:this={artEl}>
        {#if coverUrl(t?.cover)}
          {#key t?.cover}<img class="cover" src={coverUrl(t?.cover)} alt="" in:fade={{ duration: 500 }} />{/key}
        {:else}<div class="cover"></div>{/if}
      </div>
      <div class="info">
        <div class="ti">{t?.title ?? "—"}</div>
        <div class="ar">{t?.artist ?? ""}{t?.album ? ` — ${t.album}` : ""}</div>
      </div>
      <Seek />
      <div class="ctl">
        <button class="icon-btn" class:on={t?.liked} onclick={() => player.like()} aria-label="В избранное"><Heart size={19} fill={t?.liked ? "currentColor" : "none"} /></button>
        <button class="icon-btn big" onclick={player.prev} aria-label="Назад"><SkipBack size={22} fill="currentColor" /></button>
        <button class="play" onclick={player.toggle} aria-label="Играть/пауза">
          {#if app.player.playing}<Pause size={24} fill="currentColor" strokeWidth={0} />{:else}<Play size={24} fill="currentColor" strokeWidth={0} />{/if}
        </button>
        <button class="icon-btn big" onclick={player.next} aria-label="Вперёд"><SkipForward size={22} fill="currentColor" /></button>
        <div class="offset" title="Сдвиг текста (сохраняется для трека)">
          <button onclick={() => nudge(-250)} aria-label="Текст позже">−</button>
          <span class="num">{((doc?.userOffsetMs ?? 0) / 1000).toFixed(2)}</span>
          <button onclick={() => nudge(250)} aria-label="Текст раньше">+</button>
        </div>
      </div>
    </aside>

    <div class="right" bind:this={scroller} bind:clientHeight={viewH} bind:clientWidth={viewW} onwheel={wheel} class:static={!timed}>
      {#if app.lyricsLoading && !doc}
        <div class="empty"><div class="dots"><i></i><i></i><i></i></div><p class="muted">Ищем текст…</p></div>
      {:else if !doc || doc.kind === "none"}
        <div class="empty">
          <p class="big">Текст не найден</p>
          <p class="muted">Проверили файл, теги и LRCLIB. Положите рядом .lrc с тем же именем, или поищите ещё раз.</p>
          <button class="pill ghost" onclick={() => loadLyrics(true)}><Refresh size={15} /> Искать снова</button>
        </div>
      {:else if doc.kind === "instrumental"}
        <div class="empty"><p class="big">Инструментал</p><p class="muted">В этом треке нет слов.</p></div>
      {:else}
        <div class="col" bind:this={column} style="transform:translateY({timed ? shift : 0}px)">
          {#each lines as l, i (i)}
            <button
              class="line"
              class:on={i === active}
              class:past={i < active}
              class:gap={!l.text.trim()}
              bind:this={lineEls[i]}
              style={lineStyle(i)}
              onclick={() => seekTo(i)}
            >
              {#if !l.text.trim()}
                <span class="breath"><i></i><i></i><i></i></span>
              {:else if timed}
                {#each words[i] as w}{#if w.text.trim()}<span class="w">{w.text}</span>{:else}{w.text}{/if}{/each}
              {:else}
                {l.text}
              {/if}
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</section>

<style>
  .lyr {
    position: fixed; inset: 0; z-index: 50; overflow: hidden;
    --lc: #fff;
    --ldim: rgb(255 255 255 / 0.32);
    color: #fff;
    background: #08080a;
  }
  .lyr[data-theme-v="mist"] { --lc: #2b1820; --ldim: rgb(43 24 32 / 0.3); color: #2b1820; background: #f6d3de; }
  .lyr[data-theme-v="paper"] { --lc: var(--text); --ldim: rgb(17 17 17 / 0.22); color: var(--text); background: var(--bg); }
  .bg, .fx { position: absolute; inset: 0; width: 100%; height: 100%; }
  .lyr[data-theme-v="mist"] .bg { opacity: 0.55; mix-blend-mode: multiply; }
  .fx { pointer-events: none; }
  .lyr[data-theme-v="mist"] .fx { mix-blend-mode: screen; }

  .top, .left :global(.seek), .ctl, .src { transition: opacity 0.6s ease; }
  .immersive { cursor: none; }
  .immersive .top, .immersive .left :global(.seek), .immersive .ctl { opacity: 0; pointer-events: none; }
  .top { position: relative; z-index: 2; height: 52px; display: flex; align-items: center; justify-content: space-between; padding: 0 14px; }
  .top .icon-btn { color: inherit; opacity: 0.7; }
  .top .icon-btn:hover { opacity: 1; background: rgb(127 127 127 / 0.15); }
  .src { font-size: 11px; letter-spacing: 0.06em; color: inherit; opacity: 0.55; }

  .stage {
    position: relative; z-index: 2;
    height: calc(100% - 52px);
    display: grid;
    grid-template-columns: minmax(320px, 0.8fr) 1.2fr;
    gap: clamp(32px, 6vw, 110px);
    padding: 0 clamp(32px, 6vw, 110px) 40px;
  }
  .left { align-self: center; display: flex; flex-direction: column; gap: clamp(18px, 1.6vw, 30px); max-width: clamp(360px, 30vw, 640px); width: 100%; justify-self: end; }
  .art { position: relative; --pulse: 1; --glow: 0.4; transform: scale(var(--pulse)); will-change: transform; }
  .art .cover {
    width: 100%; border-radius: 14px;
    box-shadow: 0 40px 80px -30px rgb(0 0 0 / calc(var(--glow) + 0.25)), 0 0 0 1px rgb(255 255 255 / 0.06);
  }
  .art::after {
    content: ""; position: absolute; inset: 8%; z-index: -1; border-radius: 30%;
    background: rgb(var(--accent-rgb, var(--accent-default)));
    filter: blur(60px); opacity: calc(var(--glow) * 0.8);
  }
  .lyr[data-theme-v="paper"] .art .cover { border-radius: 0; filter: grayscale(1) contrast(1.05); box-shadow: none; }
  .lyr[data-theme-v="paper"] .art::after { display: none; }
  .ti { font-family: var(--font-display); font-size: clamp(24px, 1.6vw, 38px); font-weight: 700; letter-spacing: -0.01em; line-height: 1.15; }
  .ar { opacity: 0.6; margin-top: 6px; font-size: clamp(14px, 0.9vw, 20px); }
  .lyr[data-theme-v="paper"] .ti { font-style: italic; font-size: 34px; }
  .left :global(.seek .t) { color: inherit; opacity: 0.5; }
  .left :global(.rail) { background: rgb(127 127 127 / 0.28) !important; }
  .left :global(.fill) { background: currentColor !important; }
  .ctl { display: flex; align-items: center; gap: 10px; }
  .ctl .icon-btn { color: inherit; opacity: 0.75; }
  .ctl .icon-btn:hover { opacity: 1; background: rgb(127 127 127 / 0.15); }
  .ctl .icon-btn.on { color: var(--accent); opacity: 1; }
  .icon-btn.big { width: 44px; height: 44px; }
  .play { width: 56px; height: 56px; border-radius: 50%; display: grid; place-items: center; background: currentColor; transition: transform 0.25s var(--ease-spring); }
  .play :global(svg) { color: #111; }
  .lyr[data-theme-v="mist"] .play :global(svg), .lyr[data-theme-v="paper"] .play :global(svg) { color: #fff; }
  .lyr[data-theme-v="paper"] .play { border-radius: 0; }
  .play:hover { transform: scale(1.06); }
  .offset { margin-left: auto; display: flex; align-items: center; gap: 2px; font-size: 12px; opacity: 0.55; border-radius: 999px; padding: 2px; border: 1px solid rgb(127 127 127 / 0.3); }
  .offset:hover { opacity: 1; }
  .offset button { width: 26px; height: 26px; border-radius: 50%; }
  .offset button:hover { background: rgb(127 127 127 / 0.2); }
  .offset span { min-width: 40px; text-align: center; }

  .right {
    position: relative; overflow: hidden; height: 100%;
    mask-image: linear-gradient(180deg, transparent 0, #000 16%, #000 70%, transparent 100%);
  }
  .right.static { overflow-y: auto; mask-image: none; padding: 40px 0; }
  .col { display: flex; flex-direction: column; align-items: flex-start; gap: 6px; padding: 0 0 50vh; transition: transform 0.7s var(--ease-out); will-change: transform; }
  .line {
    text-align: left;
    font-family: var(--font-lyrics);
    font-weight: 750;
    font-size: calc(clamp(26px, 2.7vw, 66px) * var(--fs, 1));
    line-height: 1.22;
    letter-spacing: -0.015em;
    padding: 10px 14px;
    margin-left: -14px;
    border-radius: 16px;
    color: var(--ldim);
    opacity: var(--op);
    filter: blur(var(--blur));
    transform-origin: left center;
    transform: scale(0.94);
    transition: transform 0.6s var(--ease-spring) var(--delay), opacity 0.5s var(--delay), filter 0.5s var(--delay), color 0.3s;
    max-width: min(1200px, 100%);
  }
  .line:hover { filter: none; opacity: 0.9; background: rgb(127 127 127 / 0.1); }
  .line.on { transform: scale(1); color: var(--ldim); }
  .line.past { color: var(--ldim); }
  .static .line { color: var(--lc); transform: none; font-size: clamp(20px, 2vw, 28px); }
  .lyr[data-theme-v="paper"] .line { font-weight: 600; font-style: italic; letter-spacing: -0.02em; }
  .lyr[data-theme-v="paper"] .line.on .w { --c: var(--accent); }
  .w {
    --p: 0;
    --c: var(--lc);
    background: linear-gradient(90deg, var(--c) calc(var(--p) * 100% - 6%), var(--ldim) calc(var(--p) * 100% + 6%));
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
  }
  .line:not(.on) .w { background: none; color: inherit; }
  .line.past .w { color: inherit; }
  .line.on { text-shadow: 0 0 calc(var(--glowpx, 0px)) rgb(255 255 255 / 0.25); }
  .gap { padding-top: 18px; padding-bottom: 18px; }
  .breath { display: inline-flex; gap: 10px; }
  .breath i { width: 10px; height: 10px; border-radius: 50%; background: currentColor; opacity: 0.4; }
  .line.on .breath i { animation: breathe 1.6s ease-in-out infinite; color: var(--lc); }
  .line.on .breath i:nth-child(2) { animation-delay: 0.2s; }
  .line.on .breath i:nth-child(3) { animation-delay: 0.4s; }
  @keyframes breathe { 50% { transform: scale(1.35); opacity: 1; } }

  .empty { position: absolute; inset: 0; display: flex; flex-direction: column; justify-content: center; gap: 14px; max-width: 520px; }
  .empty .big { font-family: var(--font-display); font-size: 38px; font-weight: 700; margin: 0; }
  .empty p { margin: 0; color: inherit; }
  .empty .muted { opacity: 0.6; }
  .empty .pill { align-self: flex-start; color: inherit; border-color: rgb(127 127 127 / 0.4); }
  .dots { display: flex; gap: 8px; }
  .dots i { width: 9px; height: 9px; border-radius: 50%; background: currentColor; animation: breathe 1.2s infinite; }
  .dots i:nth-child(2) { animation-delay: 0.15s; }
  .dots i:nth-child(3) { animation-delay: 0.3s; }
</style>
