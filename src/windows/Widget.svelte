<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { app, frame, player, posNow, clock } from "../lib/store.svelte";
  import { coverUrl } from "../lib/api";
  import { accentFrom } from "../lib/color";
  import { particles } from "../lib/gl/particles";
  import { scene, vividPalette, SCENE_LIST } from "../lib/gl/scenes";
  import { shaderPalette } from "../lib/color";
  import { activeIndex, lyricTime } from "../lib/lyrics";
  import Spectrum from "../lib/components/Spectrum.svelte";
  import LiveLine from "../lib/components/LiveLine.svelte";
  import Volume from "../lib/components/Volume.svelte";
  import { Play, Pause, SkipBack, SkipForward, Heart, MousePointer } from "../lib/icons";

  const w = $derived(app.settings!.widget);
  const t = $derived(app.player.track);
  const cover = $derived(coverUrl(t?.cover));
  const doc = $derived(app.lyrics);
  const idx = $derived(doc && (doc.kind === "synced" || doc.kind === "estimated") ? activeIndex(doc.lines, lyricTime(clock.pos, doc)) : -1);
  const line = $derived(idx >= 0 ? doc!.lines[idx].text || "♪" : t ? "♪" : "");
  const nextLine = $derived(idx >= 0 ? doc!.lines[idx + 1]?.text ?? "" : "");
  const progress = $derived(app.player.durationMs ? clock.pos / app.player.durationMs : 0);

  let card: HTMLDivElement;

  // Long titles glide back and forth instead of being cut to "Never Fad…".
  function marquee(node: HTMLElement, _key: unknown) {
    const run = () => {
      node.classList.remove("mq");
      node.style.removeProperty("--mq");
      const inner = node.firstElementChild as HTMLElement | null;
      if (!inner) return;
      const over = inner.scrollWidth - node.clientWidth;
      if (over > 4) {
        node.style.setProperty("--mq", `${-over}px`);
        node.style.setProperty("--mqd", `${Math.max(6, over / 18)}s`);
        node.classList.add("mq");
      }
    };
    requestAnimationFrame(run);
    return { update: () => requestAnimationFrame(run) };
  }
  let fx: HTMLCanvasElement | undefined = $state();
  let sc: HTMLCanvasElement | undefined = $state();
  const sceneId = $derived(SCENE_LIST.some((x) => x.id === w.scene) ? w.scene : null);
  const hl = $derived.by(() => {
    if (w.color !== "cover") return null;
    const v = vividPalette(t?.palette);
    return `${Math.round(v[0] * 255)} ${Math.round(v[1] * 255)} ${Math.round(v[2] * 255)}`;
  });

  // The same spaces as the lyrics view, at low resolution: the widget is small.
  $effect(() => {
    const id = sceneId;
    if (!sc || !id) return;
    const c = sc;
    return untrack(() =>
      scene(c, {
        id: id as any,
        palette: () => app.player.track?.palette,
        auraPalette: () => shaderPalette(app.player.track?.palette, [[40, 40, 60], [80, 50, 70], [30, 50, 70], [70, 40, 50]]),
        intensity: () => (app.player.playing ? 0.9 : 0.4),
        speed: () => app.settings?.visual.sceneSpeed ?? 1,
        low: () => true,
      }),
    );
  });

  onMount(() => {
    let raf = 0;
    const loop = () => {
      raf = requestAnimationFrame(loop);
      if (!card) return;
      const g = app.settings?.widget.glow && app.player.playing ? frame.bass * 0.8 + frame.beat * 0.6 : 0;
      card.style.setProperty("--g", g.toFixed(3));
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  });

  $effect(() => {
    if (fx && w.particles) {
      const c = fx;
      return untrack(() => particles(c, {
        count: 46,
        enabled: () => app.player.playing,
        intensity: () => 0.8,
        color: () => {
          const a = accentFrom(app.player.track?.palette ?? [], true) ?? [255, 220, 200];
          return [a[0] / 255, a[1] / 255, a[2] / 255];
        },
        focus: () => [0.12, 0.5],
      }));
    }
  });
  void posNow;
</script>

<div class="wrap" style="--op:{w.opacity};--blur:{w.blur}px;--sc:{w.scale}">
  <div class="card {w.style}" class:ct={w.clickThrough} bind:this={card} data-tauri-drag-region role="group" aria-label="Мини-плеер" onwheel={(e) => { e.preventDefault(); player.volume(Math.min(1, Math.max(0, app.player.volume - Math.sign(e.deltaY) * 0.04))); }}>
    {#if sceneId}<canvas class="scene" bind:this={sc}></canvas>{:else if cover}<img class="bgart" src={cover} alt="" />{/if}
    {#if w.particles}<canvas class="fx" bind:this={fx}></canvas>{/if}
    {#if w.spectrum && w.style !== "spectrum"}<div class="specbg"><Spectrum bars={40} gap={2} color="currentColor" /></div>{/if}

    {#if w.style === "spectrum"}
      <div class="specbig"><Spectrum bars={36} mode="mirror" gap={3} radius={3} color="currentColor" /></div>
      <div class="meta row" data-tauri-drag-region>
        <div class="tt ellipsis" data-tauri-drag-region>{t?.title ?? "oto"}</div>
        <div class="ar ellipsis" data-tauri-drag-region>{t?.artist ?? ""}</div>
      </div>
    {:else if w.style === "lyrics"}
      {#if cover}<img class="art sm" src={cover} alt="" data-tauri-drag-region />{/if}
      <div class="lyr" data-tauri-drag-region>
        <LiveLine size={21} fallback={t ? `${t.artist} — ${t.title}` : "oto"} pop={w.lyricsStyle === "edit"} glow={w.textGlow} {hl} />
      </div>
    {:else}
      {#if cover}<img class="art" src={cover} alt="" data-tauri-drag-region />{:else}<div class="art ph" data-tauri-drag-region></div>{/if}
      <div class="meta" data-tauri-drag-region>
        <div class="tt" data-tauri-drag-region use:marquee={t?.id}><span data-tauri-drag-region>{t?.title ?? "Ничего не играет"}</span></div>
        <div class="ar ellipsis" data-tauri-drag-region>{t?.artist ?? "oto"}</div>
        <div class="prog" role="slider" tabindex="-1" aria-label="Позиция" aria-valuenow={Math.round(progress * 100)}
          onpointerdown={(e) => { const r = e.currentTarget.getBoundingClientRect(); player.seek(((e.clientX - r.left) / r.width) * app.player.durationMs); }}>
          <i style="transform:scaleX({progress})"></i>
        </div>
      </div>
    {/if}

    {#if w.style !== "spectrum" || true}
      <div class="ctl">
        <button onclick={() => player.like()} class:on={t?.liked} aria-label="В избранное"><Heart size={15} fill={t?.liked ? "currentColor" : "none"} /></button>
        <button onclick={player.prev} aria-label="Назад"><SkipBack size={16} fill="currentColor" /></button>
        <button class="pp" onclick={player.toggle} aria-label="Играть/пауза">
          {#if app.player.playing}<Pause size={16} fill="currentColor" strokeWidth={0} />{:else}<Play size={16} fill="currentColor" strokeWidth={0} />{/if}
        </button>
        <button onclick={player.next} aria-label="Вперёд"><SkipForward size={16} fill="currentColor" /></button>
        <Volume size={15} compact width={64} />
      </div>
    {/if}
    {#if w.clickThrough}<span class="ctbadge" title="Сквозь клики"><MousePointer size={11} /></span>{/if}
  </div>
</div>

<style>
  :global(html), :global(body) { background: transparent !important; }
  .wrap { position: fixed; inset: 0; padding: 26px; display: flex; }
  .card {
    --g: 0;
    position: relative;
    flex: 1;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 14px 12px 12px;
    border-radius: 22px;
    overflow: hidden;
    color: #fff;
    background: rgb(14 14 18 / var(--op));
    /* Glow stays inside the 26px transparent margin: blur ≤ 18px, spread ≤ 2px. */
    box-shadow:
      0 0 0 1px rgb(255 255 255 / 0.07) inset,
      0 6px 16px -6px rgb(0 0 0 / 0.45),
      0 0 calc(4px + var(--g) * 14px) calc(var(--g) * 2px) rgb(var(--accent-rgb, var(--accent-default)) / calc(var(--g) * 0.55));
    transition: box-shadow 0.08s linear;
    font-family: "Onest Variable", system-ui, sans-serif;
  }
  .bgart { position: absolute; inset: -30%; width: 160%; height: 160%; object-fit: cover; filter: blur(var(--blur)) saturate(1.6); opacity: calc(var(--op) * 0.55); pointer-events: none; }
  .fx { position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; }
  .scene { position: absolute; inset: 0; width: 100%; height: 100%; opacity: var(--op); pointer-events: none; }
  .specbg { position: absolute; left: 0; right: 0; bottom: 0; height: 46%; opacity: 0.16; color: #fff; pointer-events: none; }
  .art { position: relative; width: 76px; height: 76px; border-radius: 14px; object-fit: cover; flex: none; box-shadow: 0 10px 24px -10px rgb(0 0 0 / 0.7); }
  .art.sm { width: 64px; height: 64px; border-radius: 12px; }
  .art.ph { background: rgb(255 255 255 / 0.08); }
  .meta { position: relative; flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .meta.row { position: absolute; left: 16px; right: 150px; bottom: 12px; }
  .tt { font-weight: 650; font-size: 14.5px; letter-spacing: -0.01em; overflow: hidden; white-space: nowrap; mask-image: linear-gradient(90deg, #000 88%, transparent); }
  .tt span { display: inline-block; }
  .tt:global(.mq) span { animation: mq var(--mqd, 8s) ease-in-out 1.5s infinite alternate; }
  @keyframes mq { 0%, 15% { transform: translateX(0); } 85%, 100% { transform: translateX(var(--mq)); } }
  .ar { font-size: 12px; opacity: 0.65; }
  .prog { margin-top: 10px; height: 3px; border-radius: 3px; background: rgb(255 255 255 / 0.16); overflow: hidden; cursor: pointer; background-clip: content-box; padding: 5px 0; margin-bottom: -5px; box-sizing: content-box; }
  .prog i { height: 3px !important; }
  .prog i { display: block; height: 100%; background: rgb(var(--accent-rgb, var(--accent-default))); transform-origin: left; }
  .ctl { position: relative; display: flex; align-items: center; gap: 2px; flex: none; }
  .spectrum .ctl { position: absolute; right: 12px; bottom: 8px; }
  .ctl button { width: 32px; height: 32px; border-radius: 50%; display: grid; place-items: center; color: rgb(255 255 255 / 0.8); transition: background 0.15s, transform 0.2s var(--ease-spring); }
  .ctl button:hover { background: rgb(255 255 255 / 0.12); color: #fff; }
  .ctl button:active { transform: scale(0.9); }
  .ctl .pp { width: 38px; height: 38px; background: #fff; color: #111; }
  .ctl .pp:hover { background: #fff; color: #111; transform: scale(1.06); }
  .ctl .on { color: rgb(var(--accent-rgb, var(--accent-default))); }
  .lyr { position: relative; flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 4px; }
  .l1 { font-weight: 750; font-size: 21px; line-height: 1.2; letter-spacing: -0.01em; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; animation: rise 0.45s var(--ease-out); }
  .l2 { font-size: 13px; opacity: 0.5; }
  @keyframes rise { from { opacity: 0; transform: translateY(8px); filter: blur(4px); } }
  .specbig { position: absolute; inset: 14px 14px 52px; color: rgb(var(--accent-rgb, var(--accent-default))); opacity: 0.95; }
  .ctbadge { position: absolute; top: 8px; right: 10px; opacity: 0.4; }
  .ct { pointer-events: none; }
</style>
