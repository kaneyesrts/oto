<script lang="ts">
  import { untrack } from "svelte";
  // Theme backdrops. Noir and Paper are flat; Mist draws layered ridges tinted by the cover;
  // Liquid shows the cover itself, huge and blurred, under a flowing gradient — something
  // with detail is needed behind the glass, or refraction has nothing to bend.
  import { app, isDark } from "../store.svelte";
  import { coverUrl, type RGB } from "../api";
  import { mesh } from "../gl/mesh";
  import { particles } from "../gl/particles";
  import { shaderPalette, rgbToHsl, hslToRgb, accentFrom } from "../color";

  let canvas: HTMLCanvasElement | undefined = $state();
  let fx: HTMLCanvasElement | undefined = $state();
  const theme = $derived(app.settings?.theme ?? "noir");
  const cover = $derived(coverUrl(app.player.track?.cover));
  const pal = $derived(app.player.track?.palette ?? []);

  // Mist ridges: cover hue, but always pastel so the frosted cards stay readable.
  const ridge = (i: number): string => {
    const base: RGB = pal[0] ?? [236, 140, 170];
    const [h, s] = rgbToHsl(base);
    const hue = pal.length ? h : 340;
    const c = hslToRgb(hue + i * 6, Math.min(0.55, Math.max(0.3, s)), 0.84 - i * 0.075);
    return `rgb(${c[0]} ${c[1]} ${c[2]})`;
  };

  // Effects must depend only on the canvas, theme and on/off flags. Everything the renderers
  // read per frame goes through getters inside `untrack`; otherwise every player update
  // (once a second while playing) would tear the WebGL context down and rebuild it.
  $effect(() => {
    if (canvas && (theme === "liquid" || theme === "aura")) {
      const c = canvas;
      return untrack(() => mesh(c, {
        palette: () => shaderPalette(app.player.track?.palette, [[38, 34, 48], [70, 44, 58], [30, 40, 60], [60, 36, 44]]),
        dark: () => isDark(),
        intensity: () => (app.settings?.visual.intensity ?? 1) * (theme === "aura" ? 0.85 : 0.7),
        scale: 0.35,
      }));
    }
  });

  // A sparse, slow drift of particles: present in the room, never competing with the UI.
  $effect(() => {
    if (fx && theme === "aura" && app.settings?.visual.particles) {
      const c = fx;
      const count = app.settings.visual.quality === "low" ? 40 : 90;
      return untrack(() => particles(c, {
        count,
        enabled: () => app.player.playing && !app.lyricsOpen,
        intensity: () => (app.settings?.visual.intensity ?? 1) * 0.55,
        color: () => {
          const a = accentFrom(app.player.track?.palette ?? [], true) ?? [255, 210, 190];
          return [a[0] / 255 * 0.6 + 0.4, a[1] / 255 * 0.6 + 0.4, a[2] / 255 * 0.6 + 0.4];
        },
        focus: () => [0.3, 0.35],
      }));
    }
  });
</script>

<div class="bg" data-t={theme}>
  {#if theme === "aura"}
    <canvas bind:this={canvas} class="fill"></canvas>
    {#if cover}{#key cover}<img class="fill art aura-art" src={cover} alt="" />{/key}{/if}
    <canvas bind:this={fx} class="fill"></canvas>
    <div class="fill aura-shade"></div>
  {:else if theme === "liquid"}
    <canvas bind:this={canvas} class="fill"></canvas>
    {#if cover}
      {#key cover}<img class="fill art" src={cover} alt="" />{/key}
    {/if}
    <div class="fill shade"></div>
  {:else if theme === "mist"}
    <div class="fill sky"></div>
    <svg class="fill" viewBox="0 0 1600 1000" preserveAspectRatio="xMidYMax slice" aria-hidden="true">
      <path fill={ridge(0)} d="M0 520 L120 470 L260 500 L380 380 L470 420 L600 300 L720 360 L820 330 L960 410 L1080 340 L1200 380 L1330 300 L1460 360 L1600 330 V1000 H0Z" opacity="0.75" />
      <path fill={ridge(1)} d="M0 620 L160 560 L300 600 L430 520 L560 580 L700 500 L860 570 L990 520 L1120 590 L1280 520 L1420 580 L1600 540 V1000 H0Z" opacity="0.85" />
      <path fill={ridge(2)} d="M0 720 L140 690 L280 730 L420 660 L600 720 L760 680 L900 740 L1060 690 L1220 740 L1380 700 L1600 740 V1000 H0Z" />
      <path fill={ridge(3)} d="M0 840 L200 800 L380 850 L560 810 L760 860 L960 820 L1160 870 L1380 830 L1600 860 V1000 H0Z" />
      <!-- treeline -->
      <g fill={ridge(4)} opacity="0.9">
        {#each Array(26) as _, i}
          {@const x = 1080 + i * 22 + ((i * 37) % 13)}
          {@const hgt = 70 + ((i * 53) % 90)}
          <path d="M{x} 1000 L{x} {1000 - hgt} L{x - 14} {1000 - hgt * 0.45} L{x - 5} {1000 - hgt * 0.5} L{x - 16} {1000 - hgt * 0.1} L{x + 16} {1000 - hgt * 0.1} L{x + 5} {1000 - hgt * 0.5} L{x + 14} {1000 - hgt * 0.45}Z" />
        {/each}
      </g>
    </svg>
    <div class="fill haze"></div>
  {/if}
</div>

<style>
  .bg { position: fixed; inset: 0; z-index: 0; pointer-events: none; background: var(--bg); transition: background 0.8s; }
  .fill { position: absolute; inset: 0; width: 100%; height: 100%; }
  .art {
    object-fit: cover;
    filter: blur(70px) saturate(1.5);
    transform: scale(1.35);
    opacity: 0.55;
    mix-blend-mode: screen;
    animation: fade 1.2s var(--ease-out);
  }
  .aura-art { opacity: 0.22; filter: blur(100px) saturate(1.5) brightness(0.45); }
  .aura-shade { background: linear-gradient(180deg, rgb(0 0 0 / 0.18), rgb(0 0 0 / 0.05) 40%, rgb(0 0 0 / 0.35)); }
  .shade { background: radial-gradient(120% 80% at 50% 0%, transparent, rgb(0 0 0 / 0.35)); }
  .sky {
    background:
      radial-gradient(60% 50% at 75% 18%, rgb(255 255 255 / 0.65), transparent 70%),
      linear-gradient(180deg, color-mix(in srgb, rgb(var(--amb-2, var(--amb-2-def))) 35%, #fbe3ea), #f6cfda 55%, #eeb6c7);
    transition: background 1s;
  }
  .haze { background: linear-gradient(180deg, rgb(255 255 255 / 0.05), rgb(255 230 238 / 0.25)); }
  svg path { transition: fill 1.2s ease; }
  @keyframes fade { from { opacity: 0; } }
</style>
