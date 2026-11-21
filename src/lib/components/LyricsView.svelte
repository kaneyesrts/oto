<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { fade } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { app, frame, posNow, player, loadLyrics, toggleFullscreen, setFullscreen } from "../store.svelte";
  import { api, coverUrl } from "../api";
  import { particles } from "../gl/particles";
  import { scene, vividPalette, SCENE_LIST } from "../gl/scenes";
  import { shaderPalette, accentFrom } from "../color";
  import { activeIndex, lyricTime, wordsOf, sourceLabel, lineRoles } from "../lyrics";
  import Seek from "./Seek.svelte";
  import LyricsFx from "./LyricsFx.svelte";
  import { Play, Pause, SkipBack, SkipForward, Heart, ChevronLeft, Refresh, Minimize, Maximize, Sparkles } from "../icons";

  const t = $derived(app.player.track);
  const doc = $derived(app.lyrics);
  const lines = $derived(doc?.lines ?? []);
  const timed = $derived(doc?.kind === "synced" || doc?.kind === "estimated");
  const theme = $derived(app.settings?.theme ?? "noir");
  const words = $derived(lines.map(wordsOf));
  const L = $derived(app.settings!.lyrics);
  // Unknown ids (a scene that was renamed) fall back to the aura.
  const sceneId = $derived.by(() => {
    const v = app.settings?.visual.scene ?? "aura";
    return SCENE_LIST.some((s) => s.id === v) ? v : "aura";
  });
  // Scenes other than the aura are night skies: light themes switch the text to white there.
  const look = $derived(theme === "mist" && sceneId !== "aura" ? "noir" : theme);
  const edit = $derived(L.style === "edit" && timed);
  const roles = $derived(lineRoles(lines));
  const acc = $derived.by(() => {
    const v = vividPalette(t?.palette);
    return `${Math.round(v[0] * 255)} ${Math.round(v[1] * 255)} ${Math.round(v[2] * 255)}`;
  });

  let bgCanvas: HTMLCanvasElement | undefined = $state();
  let fxCanvas: HTMLCanvasElement | undefined = $state();
  let scroller: HTMLDivElement | undefined = $state();
  let artEl: HTMLDivElement | undefined = $state();
  let editEl: HTMLDivElement | undefined = $state();
  let lineEls: HTMLElement[] = $state([]);
  let active = $state(-1);
  let manual = $state(0);
  let manualTimer = 0;
  let idle = $state(false);
  let idleTimer = 0;
  let fxOpen = $state(false);
  // Read by the scene every frame (not reactive on purpose): the finale brightens the space.
  let boost = 1;
  function close() {
    if (app.fullscreen) setFullscreen(false);
    app.lyricsOpen = false;
  }
  let offsets = $state<number[]>([]);
  let viewH = $state(600);
  let viewW = $state(800);

  const finaleOn = $derived(L.hlFinale && active >= 0 && roles.finale[active]);
  const chorusOn = $derived(L.hlChorus && active >= 0 && roles.chorus[active]);

  // The space behind the text. Re-created only when the scene or the canvas changes.
  $effect(() => {
    const id = sceneId;
    if (!bgCanvas) return;
    const c = bgCanvas;
    return untrack(() =>
      scene(c, {
        id,
        palette: () => app.player.track?.palette,
        auraPalette: () => shaderPalette(app.player.track?.palette, [[40, 40, 60], [80, 50, 70], [30, 50, 70], [70, 40, 50]]),
        dark: () => look !== "mist",
        intensity: () => (app.settings?.visual.intensity ?? 1) * boost,
        speed: () => app.settings?.visual.sceneSpeed ?? 1,
        low: () => app.settings?.visual.quality === "low",
      }),
    );
  });

  onMount(() => {
    loadLyrics();
    const stopFx = fxCanvas
      ? particles(fxCanvas, {
          count: app.settings?.visual.quality === "low" ? 110 : 260,
          enabled: () => !!app.settings?.visual.particles && app.player.playing,
          intensity: () => (app.settings?.visual.intensity ?? 1) * boost,
          color: () => {
            const a = accentFrom(app.player.track?.palette ?? [], true) ?? [255, 200, 180];
            const m = look === "mist" ? 1 : 0.85;
            return [(a[0] / 255) * 0.5 + 0.5 * m, (a[1] / 255) * 0.5 + 0.5 * m, (a[2] / 255) * 0.5 + 0.5 * m];
          },
          focus: () => (edit ? [0.5, 0.45] : [0.27, 0.45]),
        })
      : () => {};

    let raf = 0;
    const loop = () => {
      raf = requestAnimationFrame(loop);
      const amt = app.settings?.visual.intensity ?? 1;
      boost += ((finaleOn ? 1.4 : 1) - boost) * 0.03;
      // Cover breathes with the bass.
      if (artEl) {
        const s = 1 + frame.bass * 0.025 * amt + frame.beat * 0.012;
        artEl.style.setProperty("--pulse", s.toFixed(4));
        artEl.style.setProperty("--glow", (0.35 + frame.lvl * 0.5).toFixed(3));
      }
      if (!timed || !lines.length) return;
      const tm = lyricTime(posNow(), doc);
      const idx = activeIndex(lines, tm);
      if (idx !== active) active = idx;
      if (idx < 0) return;
      const beat = ((frame.beat * 0.75 + frame.bass * 0.35) * (L.beat ?? 0)).toFixed(3);
      const ws = words[idx];
      if (edit) {
        // Words pop in as they are sung and glow while they last.
        if (!editEl) return;
        editEl.style.setProperty("--beat", beat);
        editEl.style.setProperty("--lv", frame.lvl.toFixed(3));
        const spans = editEl.querySelectorAll<HTMLElement>("span.ew");
        let k = 0;
        for (const w of ws) {
          if (!w.text.trim()) continue;
          const el = spans[k++];
          if (!el) break;
          const a = Math.min(1, Math.max(0, (tm - w.t + 90) / 240));
          const end = w.t + Math.max(120, w.d);
          const g = tm < w.t ? 0 : tm <= end ? 1 : Math.max(0, 1 - (tm - end) / 900);
          el.style.setProperty("--a", a.toFixed(3));
          el.style.setProperty("--g", g.toFixed(3));
        }
        return;
      }
      // Karaoke fill on the active line only; direct style writes, no re-render.
      const el = lineEls[idx];
      if (el) {
        el.style.setProperty("--beat", beat);
        el.style.setProperty("--lv", frame.lvl.toFixed(3));
        const spans = el.querySelectorAll<HTMLElement>("span.w");
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
      if (fxOpen) fxOpen = false;
      else if (app.fullscreen) setFullscreen(false);
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
      cancelAnimationFrame(raf); stopFx(); clearTimeout(idleTimer);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("pointermove", wake);
    };
  });

  async function measure() {
    await tick();
    offsets = lineEls.map((e) => (e ? e.offsetTop + e.offsetHeight / 2 : 0));
  }
  // Re-measure when the text or the viewport changes (fullscreen, resize, font, layout).
  $effect(() => { lines; viewH; viewW; L.fontScale; L.align; L.style; measure(); });

  const shift = $derived.by(() => {
    if (!scroller || !offsets.length) return 0;
    const anchor = viewH * (L.align === "center" ? 0.45 : 0.4);
    const y = active >= 0 ? offsets[active] ?? 0 : (offsets[0] ?? 0) - 120;
    return anchor - y + manual;
  });

  function wheel(e: WheelEvent) {
    if (!timed || edit) return;
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
    const blur = L.depthBlur && timed ? Math.min(ad * 1.1, 5) : 0;
    const op = !timed ? 0.9 : d === 0 ? 1 : d < 0 ? Math.max(0.16, 0.42 - ad * 0.05) : Math.max(0.14, 0.5 - ad * 0.07);
    const delay = d > 0 ? Math.min(d, 6) * 34 : 0;
    // Sung-line afterglow: 1 for the line just finished, fading over four lines.
    const k = L.hlSung && d < 0 && ad <= 4 ? (5 - ad) / 5 : 0;
    return `--blur:${blur}px;--op:${op};--delay:${delay}ms;--k:${k.toFixed(2)}`;
  }

  const nextText = $derived(active >= 0 ? lines.slice(active + 1).find((l) => l.text.trim())?.text ?? "" : lines.find((l) => l.text.trim())?.text ?? "");

  // Edit-style line change: the old line lifts away and blurs out, the new one rises in.
  function lineIn(_: Element) {
    return { duration: 420, easing: cubicOut, css: (u: number) => `opacity:${u};transform:translateY(${(1 - u) * 28}px) scale(${0.96 + u * 0.04});filter:blur(${(1 - u) * 12}px)` };
  }
  function lineOut(_: Element) {
    return { duration: 340, easing: cubicOut, css: (u: number) => `opacity:${u};transform:translateY(${(u - 1) * 46}px) scale(${1.04 - u * 0.04});filter:blur(${(1 - u) * 10}px)` };
  }
</script>

<section
  aria-label="Текст песни"
  class="lyr"
  class:immersive={app.fullscreen && idle && !fxOpen}
  class:finale={finaleOn}
  class:chorus-on={chorusOn}
  class:center={L.align === "center"}
  ondblclick={(e) => { if ((e.target as HTMLElement).closest(".line, button, .fxpanel")) return; toggleFullscreen(); }}
  data-theme-v={look}
  style="--fs:{L.fontScale ?? 1};--gl:{L.glow ?? 0.25};--acc:{acc};--hlc:{L.color === 'cover' ? acc : 'var(--txt)'}"
  transition:fade={{ duration: 260 }}
>
  {#if theme !== "paper"}
    <canvas class="bg" bind:this={bgCanvas}></canvas>
    <canvas class="fx" bind:this={fxCanvas}></canvas>
  {/if}

  <header class="top" data-tauri-drag-region>
    <button class="icon-btn" onclick={close} aria-label="Закрыть"><ChevronLeft size={22} /></button>
    <div class="src faint" data-tauri-drag-region>{sourceLabel(doc)}</div>
    <div class="tr">
      <button class="icon-btn" class:on={fxOpen} onclick={() => (fxOpen = !fxOpen)} aria-label="Эффекты" title="Эффекты и пространство"><Sparkles size={18} /></button>
      <button class="icon-btn" onclick={toggleFullscreen} aria-label="Полный экран" title="Полный экран (F11, двойной клик)">
        {#if app.fullscreen}<Minimize size={18} />{:else}<Maximize size={18} />{/if}
      </button>
    </div>
  </header>

  <div class="stage" class:edit>
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

    <div class="right" bind:this={scroller} bind:clientHeight={viewH} bind:clientWidth={viewW} onwheel={wheel} class:static={!timed} class:editmode={edit}>
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
      {:else if edit}
        <div class="edit-stage">
          {#key active}
            <div
              class="eline"
              class:chorus={L.hlChorus && roles.chorus[active]}
              class:fin={L.hlFinale && roles.finale[active]}
              bind:this={editEl}
              in:lineIn
              out:lineOut
            >
              {#if active < 0 || !lines[active]?.text.trim()}
                <span class="breath"><i></i><i></i><i></i></span>
              {:else}
                {#each words[active] as w}{#if w.text.trim()}<span class="ew">{w.text}</span>{:else}{" "}{/if}{/each}
              {/if}
            </div>
          {/key}
          {#if nextText}<div class="enext">{nextText}</div>{/if}
        </div>
      {:else}
        <div class="col" style="transform:translateY({timed ? shift : 0}px)">
          {#each lines as l, i (i)}
            <button
              class="line"
              class:on={i === active}
              class:past={i < active}
              class:gap={!l.text.trim()}
              class:sung={L.hlSung && i < active && active - i <= 4}
              class:chorus={L.hlChorus && roles.chorus[i]}
              class:fin={L.hlFinale && roles.finale[i]}
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

  {#if fxOpen}<div class="fxpanel"><LyricsFx onclose={() => (fxOpen = false)} /></div>{/if}
</section>

<style>
  .lyr {
    position: fixed; inset: 0; z-index: 50; overflow: hidden;
    --txt: 255 255 255;
    --lc: rgb(var(--txt));
    --ldim: rgb(var(--txt) / 0.32);
    --beat: 0;
    color: #fff;
    background: #08080a;
  }
  .lyr[data-theme-v="mist"] { --txt: 43 24 32; color: #2b1820; background: #f6d3de; }
  .lyr[data-theme-v="paper"] { --txt: 17 17 17; color: var(--text); background: var(--bg); }
  .bg, .fx { position: absolute; inset: 0; width: 100%; height: 100%; }
  .lyr[data-theme-v="mist"] .bg { opacity: 0.55; mix-blend-mode: multiply; }
  .fx { pointer-events: none; }
  .lyr[data-theme-v="mist"] .fx { mix-blend-mode: screen; }

  .top, .left :global(.seek), .ctl, .src { transition: opacity 0.6s ease; }
  .immersive { cursor: none; }
  .immersive .top, .immersive .left :global(.seek), .immersive .ctl { opacity: 0; pointer-events: none; }
  .top { position: relative; z-index: 3; height: 52px; display: flex; align-items: center; justify-content: space-between; padding: 0 14px; }
  .top .icon-btn { color: inherit; opacity: 0.7; }
  .top .icon-btn:hover, .top .icon-btn.on { opacity: 1; background: rgb(127 127 127 / 0.15); }
  .tr { display: flex; gap: 4px; }
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
  .play { flex: none; width: 56px; height: 56px; border-radius: 50%; display: grid; place-items: center; background: currentColor; transition: transform 0.25s var(--ease-spring); }
  .play :global(svg) { color: #111; }
  .lyr[data-theme-v="mist"] .play :global(svg), .lyr[data-theme-v="paper"] .play :global(svg) { color: #fff; }
  .lyr[data-theme-v="paper"] .play { border-radius: 0; }
  .play:hover { transform: scale(1.06); }
  .offset { margin-left: auto; display: flex; align-items: center; gap: 2px; font-size: 12px; opacity: 0.55; border-radius: 999px; padding: 2px; border: 1px solid rgb(127 127 127 / 0.3); }
  .offset:hover { opacity: 1; }
  .offset button { width: 26px; height: 26px; border-radius: 50%; }
  .offset button:hover { background: rgb(127 127 127 / 0.2); }
  .offset span { min-width: 40px; text-align: center; }

  /* Edit style: the words own the screen; the player shrinks to a corner card. */
  .stage.edit { grid-template-columns: 1fr; padding: 0 clamp(24px, 5vw, 90px) 40px; }
  .stage.edit .left {
    position: absolute; z-index: 3; left: clamp(20px, 3vw, 48px); bottom: 24px;
    width: 340px; max-width: calc(100% - 40px);
    display: grid; grid-template-columns: 64px 1fr; align-items: center; gap: 10px 14px;
  }
  .stage.edit .art::after { inset: 0; filter: blur(24px); }
  .stage.edit .art .cover { border-radius: 10px; }
  .stage.edit .ti { font-size: 16px; }
  .stage.edit .ar { font-size: 13px; margin-top: 2px; }
  .stage.edit .left :global(.seek), .stage.edit .ctl { grid-column: 1 / -1; }
  .stage.edit .play { width: 44px; height: 44px; }
  .stage.edit .icon-btn.big { width: 38px; height: 38px; }
  .immersive .stage.edit .left { opacity: 0.5; }

  .right {
    position: relative; overflow: hidden; height: 100%;
    mask-image: linear-gradient(180deg, transparent 0, #000 16%, #000 70%, transparent 100%);
  }
  .right.editmode { mask-image: none; overflow: visible; }
  .right.static { overflow-y: auto; mask-image: none; padding: 40px 0; }
  .col { display: flex; flex-direction: column; align-items: flex-start; gap: 6px; padding: 0 0 50vh; transition: transform 0.7s var(--ease-out); will-change: transform; }
  .center .col { align-items: center; }
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
    transition: transform 0.6s var(--ease-spring) var(--delay), opacity 0.5s var(--delay), filter 0.5s var(--delay), color 0.3s, text-shadow 0.5s;
    max-width: min(1200px, 100%);
  }
  .center .line { text-align: center; transform-origin: center; margin-left: 0; }
  .line:hover { filter: none; opacity: 0.9; background: rgb(127 127 127 / 0.1); }
  /* "scale" is separate from "transform" and has no transition: the beat pulse is instant. */
  .line.on { transform: scale(1); color: var(--ldim); scale: calc(1 + var(--beat) * 0.045); }
  .line.on { text-shadow: 0 0 calc(var(--gl) * 34px) rgb(var(--hlc) / calc(var(--gl) * 0.6 + var(--lv, 0) * 0.25)); }
  .static .line { color: var(--lc); transform: none; font-size: clamp(20px, 2vw, 28px); }
  .lyr[data-theme-v="paper"] .line { font-weight: 600; font-style: italic; letter-spacing: -0.02em; }
  .lyr[data-theme-v="paper"] .line.on .w { --c: var(--accent); }
  .w {
    --p: 0;
    --c: rgb(var(--hlc));
    background: linear-gradient(90deg, var(--c) calc(var(--p) * 100% - 6%), var(--ldim) calc(var(--p) * 100% + 6%));
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
  }
  .line:not(.on) .w { background: none; color: inherit; }
  .line.past .w { color: inherit; }

  /* Highlights */
  .line.sung { color: rgb(var(--hlc) / calc(0.3 + var(--k) * 0.6)); opacity: calc(0.4 + var(--k) * 0.6); filter: blur(calc(var(--blur) * (1 - var(--k)))); text-shadow: 0 0 calc(var(--gl) * 22px * var(--k)) rgb(var(--hlc) / calc(var(--k) * 0.45)); }
  .line.chorus:not(.on):not(.sung) { color: rgb(var(--acc) / 0.42); }
  .line.chorus.on .w { --c: rgb(var(--acc)); }
  .line.chorus.on { text-shadow: 0 0 calc((var(--gl) + 0.25) * 40px) rgb(var(--acc) / calc(var(--gl) * 0.7 + 0.2)); }
  .line.fin:not(.on):not(.past) { color: rgb(var(--acc) / 0.5); }
  .line.fin.on { letter-spacing: 0.005em; text-shadow: 0 0 calc((var(--gl) + 0.4) * 50px) rgb(var(--acc) / 0.75), 0 0 calc((var(--gl) + 0.4) * 14px) rgb(255 255 255 / 0.5); }
  .line.fin.on .w { --c: #fff; }
  .finale .art::after { opacity: calc(var(--glow) * 1.4); background: rgb(var(--acc)); }

  /* Edit style line */
  .edit-stage { position: absolute; inset: 0 0 18%; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; }
  .lyr:not(.center) .edit-stage { align-items: flex-start; text-align: left; }
  .eline {
    position: absolute;
    max-width: min(1300px, 92%);
    font-family: var(--font-lyrics);
    font-weight: 820;
    font-size: calc(clamp(40px, 5.4vw, 124px) * var(--fs, 1));
    line-height: 1.08;
    letter-spacing: -0.03em;
    color: rgb(var(--txt));
    scale: calc(1 + var(--beat) * 0.035);
    filter: drop-shadow(calc(var(--beat) * 3px) 0 0 rgb(255 60 120 / calc(var(--beat) * 0.3))) drop-shadow(calc(var(--beat) * -3px) 0 0 rgb(60 200 255 / calc(var(--beat) * 0.3)));
    text-wrap: balance;
  }
  .ew {
    --a: 0; --g: 0;
    display: inline-block;
    opacity: var(--a);
    transform: translateY(calc((1 - var(--a)) * 0.28em)) scale(calc(1.12 - var(--a) * 0.12));
    filter: blur(calc((1 - var(--a)) * 8px));
    color: color-mix(in srgb, rgb(var(--hlc)) calc(var(--g) * 100%), rgb(var(--txt) / 0.92));
    text-shadow: 0 0 calc(var(--g) * var(--gl) * 46px + 2px) rgb(var(--hlc) / calc(var(--g) * (0.25 + var(--gl) * 0.6)));
  }
  .eline.chorus .ew { color: color-mix(in srgb, rgb(var(--acc)) calc(var(--g) * 100%), rgb(var(--txt) / 0.92)); text-shadow: 0 0 calc(var(--g) * (var(--gl) + 0.3) * 60px + 2px) rgb(var(--acc) / calc(var(--g) * 0.8)); }
  .eline.fin { font-size: calc(clamp(44px, 6.2vw, 140px) * var(--fs, 1)); }
  .eline.fin .ew { text-shadow: 0 0 calc((var(--gl) + 0.5) * 70px) rgb(var(--acc) / calc(0.35 + var(--g) * 0.6)), 0 0 calc(var(--g) * 16px) rgb(255 255 255 / 0.6); }
  .enext { position: absolute; bottom: 4%; font-size: calc(clamp(16px, 1.4vw, 26px) * var(--fs, 1)); font-weight: 650; opacity: 0.32; max-width: 80%; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  .gap { padding-top: 18px; padding-bottom: 18px; }
  .breath { display: inline-flex; gap: 10px; }
  .breath i { width: 10px; height: 10px; border-radius: 50%; background: currentColor; opacity: 0.4; }
  .line.on .breath i, .eline .breath i { animation: breathe 1.6s ease-in-out infinite; color: var(--lc); }
  .eline .breath i { width: 16px; height: 16px; }
  .line.on .breath i:nth-child(2), .eline .breath i:nth-child(2) { animation-delay: 0.2s; }
  .line.on .breath i:nth-child(3), .eline .breath i:nth-child(3) { animation-delay: 0.4s; }
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
  @media (prefers-reduced-motion: reduce) { .eline, .line.on { scale: 1; filter: none; } }
</style>
