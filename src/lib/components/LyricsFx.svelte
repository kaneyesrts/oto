<script lang="ts">
  // Effects panel inside the lyrics view: every change applies live behind the panel.
  import { fly } from "svelte/transition";
  import { app, saveSettings } from "../store.svelte";
  import type { LyricsSettings, SceneId, Settings } from "../api";
  import { SCENE_LIST } from "../gl/scenes";
  import { X } from "../icons";

  let { onclose }: { onclose: () => void } = $props();
  const s = $derived(app.settings!);
  const L = $derived(s.lyrics);
  const set = (f: (x: Settings) => void) => saveSettings(f);

  type Preset = { id: string; name: string; scene: SceneId; particles: boolean; l: Partial<LyricsSettings> };
  const presets: Preset[] = [
    { id: "classic", name: "Классика", scene: "aura", particles: true, l: { style: "classic", align: "left", glow: 0.25, beat: 0.35, color: "white", hlSung: false, hlChorus: false, hlFinale: false, depthBlur: true } },
    { id: "edit", name: "Эдит", scene: "smoke", particles: false, l: { style: "edit", align: "center", glow: 0.65, beat: 0.85, color: "cover", hlSung: false, hlChorus: true, hlFinale: true } },
    { id: "space", name: "Космос", scene: "space", particles: false, l: { style: "classic", align: "center", glow: 0.45, beat: 0.45, color: "white", hlSung: true, hlChorus: false, hlFinale: true, depthBlur: true } },
    { id: "concert", name: "Концерт", scene: "depth", particles: true, l: { style: "edit", align: "center", glow: 0.8, beat: 1, color: "cover", hlSung: false, hlChorus: true, hlFinale: true } },
    { id: "aurora", name: "Сияние", scene: "aurora", particles: true, l: { style: "classic", align: "left", glow: 0.4, beat: 0.3, color: "cover", hlSung: true, hlChorus: true, hlFinale: false, depthBlur: true } },
    { id: "minimal", name: "Минимал", scene: "void", particles: false, l: { style: "classic", align: "center", glow: 0.15, beat: 0.1, color: "white", hlSung: false, hlChorus: false, hlFinale: false, depthBlur: true } },
  ];
  const current = $derived(
    presets.find((p) => p.scene === s.visual.scene && p.particles === s.visual.particles && Object.entries(p.l).every(([k, v]) => (L as any)[k] === v))?.id,
  );
  function apply(p: Preset) {
    set((x) => {
      x.visual.scene = p.scene;
      x.visual.particles = p.particles;
      Object.assign(x.lyrics, p.l);
    });
  }
  // Swatches hint at each scene's look without rendering six WebGL contexts.
  const swatch: Record<SceneId, string> = {
    aura: "radial-gradient(120% 90% at 20% 20%, #7a4a6a, transparent 60%), radial-gradient(100% 80% at 90% 90%, #304a7a, transparent 60%), #1a1420",
    space: "radial-gradient(1px 1px at 20% 30%, #fff, transparent), radial-gradient(1.5px 1.5px at 70% 60%, #fff, transparent), radial-gradient(1px 1px at 45% 80%, #fff, transparent), radial-gradient(1px 1px at 85% 20%, #fff, transparent), radial-gradient(80% 60% at 60% 40%, rgb(70 50 120 / 0.6), transparent 70%), #04040a",
    silk: "radial-gradient(120% 40% at 30% 60%, rgb(220 140 200 / 0.45), transparent 60%), radial-gradient(100% 30% at 70% 40%, rgb(140 150 255 / 0.35), transparent 60%), #08070c",
    smoke: "radial-gradient(90% 60% at 50% 110%, rgb(255 150 110 / 0.45), transparent 70%), radial-gradient(60% 50% at 30% 40%, rgb(120 110 130 / 0.25), transparent 70%), #070608",
    aurora: "radial-gradient(90% 40% at 40% 70%, rgb(80 230 170 / 0.4), transparent 70%), radial-gradient(70% 35% at 70% 55%, rgb(140 90 240 / 0.35), transparent 70%), linear-gradient(#020208, #0a1028)",
    depth: "radial-gradient(circle at 25% 35%, rgb(255 170 120 / 0.35) 0 9%, transparent 12%), radial-gradient(circle at 70% 62%, rgb(255 110 170 / 0.28) 0 14%, transparent 18%), radial-gradient(circle at 55% 25%, rgb(140 170 255 / 0.25) 0 6%, transparent 9%), #0c080e",
    void: "radial-gradient(60% 50% at 50% 55%, rgb(120 90 160 / 0.35), transparent 70%), #030304",
  };
</script>

{#snippet toggle(on: boolean, flip: () => void, label: string, hint = "")}
  <button class="opt" onclick={flip}>
    <span><span class="ol">{label}</span>{#if hint}<span class="oh">{hint}</span>{/if}</span>
    <span class="sw" class:on></span>
  </button>
{/snippet}
{#snippet slider(value: number, min: number, max: number, step: number, change: (v: number) => void, label: string, fmt: (v: number) => string)}
  <label class="opt col">
    <span class="row"><span class="ol">{label}</span><span class="num v">{fmt(value)}</span></span>
    <input type="range" {min} {max} {step} {value} oninput={(e) => change(+e.currentTarget.value)} />
  </label>
{/snippet}
{#snippet choice(value: string, options: [string, string][], change: (v: any) => void, label: string)}
  <div class="opt">
    <span class="ol">{label}</span>
    <span class="seg">{#each options as [v, l]}<button class:on={value === v} onclick={() => change(v)}>{l}</button>{/each}</span>
  </div>
{/snippet}

<div class="fx" transition:fly={{ x: 40, duration: 260 }} role="dialog" aria-label="Эффекты лирики">
  <header>
    <h2>Эффекты</h2>
    <button class="close" onclick={onclose} aria-label="Закрыть"><X size={17} /></button>
  </header>
  <div class="body">
    <section>
      <h3>Пресеты</h3>
      <div class="presets">
        {#each presets as p}
          <button class="preset" class:on={current === p.id} onclick={() => apply(p)}>
            <span class="pv" style="background:{swatch[p.scene]}"></span>{p.name}
          </button>
        {/each}
      </div>
    </section>

    <section>
      <h3>Пространство</h3>
      <div class="scenes">
        {#each SCENE_LIST as sc}
          <button class="scene" class:on={s.visual.scene === sc.id} onclick={() => set((x) => (x.visual.scene = sc.id))} title={sc.note}>
            <span class="sv" style="background:{swatch[sc.id]}"></span>
            <span class="sn">{sc.name}</span>
          </button>
        {/each}
      </div>
      <p class="note">{SCENE_LIST.find((x) => x.id === s.visual.scene)?.note}</p>
      {@render slider(s.visual.intensity, 0, 1.5, 0.05, (v) => set((x) => (x.visual.intensity = v)), "Реакция на музыку", (v) => `${Math.round(v * 100)}%`)}
      {@render slider(s.visual.sceneSpeed, 0.2, 2, 0.05, (v) => set((x) => (x.visual.sceneSpeed = v)), "Скорость", (v) => `${v.toFixed(1)}×`)}
      {@render toggle(s.visual.particles, () => set((x) => (x.visual.particles = !x.visual.particles)), "Частицы", "Пылинки, которые разлетаются от баса")}
    </section>

    <section>
      <h3>Текст</h3>
      {@render choice(L.style, [["classic", "Список"], ["edit", "Эдит"]], (v) => set((x) => (x.lyrics.style = v)), "Подача")}
      {@render choice(L.align, [["left", "Слева"], ["center", "По центру"]], (v) => set((x) => (x.lyrics.align = v)), "Выравнивание")}
      {@render choice(L.color, [["white", "Белый"], ["cover", "Из обложки"]], (v) => set((x) => (x.lyrics.color = v)), "Цвет")}
      {@render slider(L.glow, 0, 1, 0.05, (v) => set((x) => (x.lyrics.glow = v)), "Свечение", (v) => `${Math.round(v * 100)}%`)}
      {@render slider(L.beat, 0, 1, 0.05, (v) => set((x) => (x.lyrics.beat = v)), "Удар в такт", (v) => (v ? `${Math.round(v * 100)}%` : "выкл"))}
      {@render slider(L.fontScale, 0.7, 1.5, 0.05, (v) => set((x) => (x.lyrics.fontScale = v)), "Размер", (v) => `${Math.round(v * 100)}%`)}
      {@render toggle(L.depthBlur, () => set((x) => (x.lyrics.depthBlur = !x.lyrics.depthBlur)), "Глубина резкости", "Дальние строки размываются")}
    </section>

    <section>
      <h3>Подсветка</h3>
      {@render toggle(L.hlSung, () => set((x) => (x.lyrics.hlSung = !x.lyrics.hlSung)), "Спетые строки", "Только что спетое остаётся гореть и медленно гаснет")}
      {@render toggle(L.hlChorus, () => set((x) => (x.lyrics.hlChorus = !x.lyrics.hlChorus)), "Припев", "Повторяющиеся строки светятся цветом обложки")}
      {@render toggle(L.hlFinale, () => set((x) => (x.lyrics.hlFinale = !x.lyrics.hlFinale)), "Финал", "Последние строки песни разгораются, сцена становится ярче")}
    </section>
  </div>
</div>

<style>
  .fx {
    position: absolute; z-index: 6; top: 60px; right: 16px; bottom: 16px; width: 340px;
    display: flex; flex-direction: column;
    border-radius: 22px; overflow: hidden;
    background: rgb(12 12 16 / 0.62); backdrop-filter: blur(28px) saturate(1.4);
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.08), 0 30px 80px -30px rgb(0 0 0 / 0.8);
    color: #fff; font-family: var(--font-ui, inherit);
  }
  header { display: flex; align-items: center; justify-content: space-between; padding: 16px 16px 8px 20px; }
  h2 { margin: 0; font-family: var(--font-display); font-size: 19px; font-weight: 700; }
  .close { width: 32px; height: 32px; border-radius: 50%; display: grid; place-items: center; opacity: 0.7; }
  .close:hover { opacity: 1; background: rgb(255 255 255 / 0.1); }
  .body { flex: 1; overflow-y: auto; padding: 0 20px 20px; scrollbar-width: thin; }
  section { padding: 12px 0 8px; border-top: 1px solid rgb(255 255 255 / 0.08); }
  section:first-child { border-top: 0; }
  h3 { margin: 0 0 10px; font-size: 11px; letter-spacing: 0.12em; text-transform: uppercase; font-weight: 650; opacity: 0.55; }
  .presets { display: grid; grid-template-columns: repeat(3, 1fr); gap: 6px; }
  .preset { display: flex; flex-direction: column; align-items: stretch; gap: 6px; padding: 6px 6px 8px; border-radius: 12px; font-size: 12px; font-weight: 600; white-space: nowrap; background: rgb(255 255 255 / 0.05); transition: background 0.15s, box-shadow 0.15s; }
  .preset:hover { background: rgb(255 255 255 / 0.1); }
  .preset.on { box-shadow: inset 0 0 0 1.5px #fff; }
  .pv, .sv { display: block; height: 34px; border-radius: 8px; }
  .scenes { display: grid; grid-template-columns: repeat(4, 1fr); gap: 6px; }
  .scene { display: flex; flex-direction: column; gap: 5px; padding: 4px; border-radius: 12px; transition: background 0.15s, box-shadow 0.15s; }
  .scene:hover { background: rgb(255 255 255 / 0.08); }
  .scene.on { box-shadow: inset 0 0 0 1.5px #fff; }
  .sv { height: 40px; }
  .sn { font-size: 11.5px; font-weight: 600; white-space: nowrap; }
  .note { margin: 8px 0 4px; font-size: 12px; opacity: 0.55; min-height: 16px; }
  .opt { display: flex; align-items: center; justify-content: space-between; gap: 12px; width: 100%; min-height: 42px; padding: 4px 0; text-align: left; }
  .opt.col { flex-direction: column; align-items: stretch; gap: 6px; padding: 8px 0 6px; }
  .row { display: flex; justify-content: space-between; }
  .ol { display: block; font-size: 13.5px; font-weight: 550; }
  .oh { display: block; font-size: 11.5px; opacity: 0.5; margin-top: 2px; white-space: normal; }
  .v { font-size: 12px; opacity: 0.55; }
  .sw { flex: none; width: 36px; height: 22px; border-radius: 999px; background: rgb(255 255 255 / 0.18); position: relative; transition: background 0.2s; }
  .sw::after { content: ""; position: absolute; top: 3px; left: 3px; width: 16px; height: 16px; border-radius: 50%; background: #fff; transition: transform 0.3s var(--ease-spring); }
  .sw.on { background: rgb(var(--accent-rgb, var(--accent-default))); }
  .sw.on::after { transform: translateX(14px); }
  .seg { display: flex; padding: 3px; border-radius: 999px; background: rgb(255 255 255 / 0.08); flex: none; }
  .seg button { height: 26px; padding: 0 11px; border-radius: 999px; font-size: 12px; white-space: nowrap; opacity: 0.65; }
  .seg button.on { background: rgb(255 255 255 / 0.16); opacity: 1; }
  input[type="range"] { -webkit-appearance: none; appearance: none; width: 100%; height: 4px; border-radius: 4px; background: rgb(255 255 255 / 0.16); outline: none; }
  input[type="range"]::-webkit-slider-thumb { -webkit-appearance: none; width: 15px; height: 15px; border-radius: 50%; background: #fff; cursor: pointer; }
</style>
