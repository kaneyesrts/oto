<script lang="ts">
  import { app, saveSettings } from "../store.svelte";

  const FREQS = ["31", "62", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];
  const PRESETS: Record<string, { name: string; g: number[] }> = {
    flat: { name: "Ровно", g: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0] },
    bass: { name: "Бас", g: [6, 5, 4, 2, 0, 0, 0, 0, 0, 0] },
    vocal: { name: "Вокал", g: [-2, -2, -1, 0, 2, 4, 4, 3, 1, 0] },
    rock: { name: "Рок", g: [4, 3, 2, 0, -1, -1, 1, 3, 4, 4] },
    electronic: { name: "Электроника", g: [5, 4, 1, 0, -2, 1, 0, 2, 4, 5] },
    acoustic: { name: "Акустика", g: [3, 2, 1, 1, 2, 2, 3, 3, 2, 1] },
    treble: { name: "Верха", g: [0, 0, 0, 0, 0, 1, 2, 4, 5, 6] },
    night: { name: "Тихо, ночью", g: [4, 3, 1, 0, -1, -1, 0, 1, 3, 4] },
  };

  const a = $derived(app.settings!.audio);
  // Local copy while dragging so the curve follows the finger; saved with a small debounce.
  let gains = $state<number[]>([]);
  let preamp = $state(0);
  $effect(() => { gains = [...a.eqGains]; preamp = a.eqPreamp; });

  let t = 0;
  function commit(preset?: string) {
    clearTimeout(t);
    const g = [...gains], p = preamp;
    t = window.setTimeout(() => saveSettings((s) => {
      s.audio.eqGains = g;
      s.audio.eqPreamp = p;
      s.audio.eqPreset = preset ?? "custom";
      s.audio.eqEnabled = true;
    }), 120);
  }
  function preset(id: string) {
    gains = [...PRESETS[id].g];
    preamp = Math.min(0, -Math.max(0, ...gains) * 0.5); // headroom for boosts
    commit(id);
  }

  // Smooth response curve through the slider points (Catmull-Rom → cubic Bézier).
  const W = 560, H = 140, PAD = 14;
  const x = (i: number) => ((i + 0.5) * W) / 10; // centre of each slider column
  const y = (g: number) => H / 2 - (g / 12) * (H / 2 - 12);
  const curve = $derived.by(() => {
    const p = gains.map((g, i) => [x(i), y(g)]);
    if (p.length < 2) return "";
    let d = `M ${p[0][0]} ${p[0][1]}`;
    for (let i = 0; i < p.length - 1; i++) {
      const p0 = p[i - 1] ?? p[i], p1 = p[i], p2 = p[i + 1], p3 = p[i + 2] ?? p2;
      const c1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
      const c2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
      d += ` C ${c1[0]} ${c1[1]}, ${c2[0]} ${c2[1]}, ${p2[0]} ${p2[1]}`;
    }
    return d;
  });

  function drag(e: PointerEvent, i: number) {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const r = el.getBoundingClientRect();
    const set = (ev: PointerEvent) => {
      const k = 1 - Math.min(1, Math.max(0, (ev.clientY - r.top) / r.height));
      gains[i] = Math.round((k * 24 - 12) * 2) / 2;
      commit();
    };
    set(e);
    el.onpointermove = (ev) => ev.buttons && set(ev);
    el.onpointerup = () => { el.onpointermove = null; };
  }
</script>

<div class="eq" class:off={!a.eqEnabled}>
  <div class="presets">
    {#each Object.entries(PRESETS) as [id, p]}
      <button class:on={a.eqPreset === id && a.eqEnabled} onclick={() => preset(id)}>{p.name}</button>
    {/each}
  </div>
  <div class="board">
    <svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" aria-hidden="true">
      <line x1={PAD} x2={W - PAD} y1={H / 2} y2={H / 2} class="zero" />
      <path d="{curve} L {x(9)} {H} L {x(0)} {H} Z" class="area" />
      <path d={curve} class="line" />
    </svg>
    <div class="cols">
      {#each gains as g, i}
        <div class="col">
          <span class="db num">{g > 0 ? "+" : ""}{g}</span>
          <div class="track" role="slider" tabindex="0" aria-label="{FREQS[i]} Гц" aria-valuenow={g}
            onpointerdown={(e) => drag(e, i)}
            ondblclick={() => { gains[i] = 0; commit(); }}
            onkeydown={(e) => { if (e.key === "ArrowUp") { gains[i] = Math.min(12, g + 0.5); commit(); } if (e.key === "ArrowDown") { gains[i] = Math.max(-12, g - 0.5); commit(); } }}>
            <i class="knob" style="top:{(1 - (g + 12) / 24) * 100}%"></i>
          </div>
          <span class="f num faint">{FREQS[i]}</span>
        </div>
      {/each}
    </div>
  </div>
  <label class="pre">
    <span>Предусиление</span>
    <input type="range" min="-12" max="6" step="0.5" bind:value={preamp} oninput={() => commit()} />
    <span class="num faint">{preamp > 0 ? "+" : ""}{preamp} дБ</span>
  </label>
</div>

<style>
  .eq { display: flex; flex-direction: column; gap: 14px; transition: opacity 0.2s; }
  .eq.off .board { opacity: 0.45; }
  .presets { display: flex; flex-wrap: wrap; gap: 6px; }
  .presets button { height: 30px; padding: 0 12px; border-radius: 999px; font-size: 12.5px; background: var(--surface); color: var(--text-2); }
  .presets button:hover { color: var(--text); }
  .presets button.on { background: var(--accent); color: #fff; }
  :root[data-theme="paper"] .presets button { border-radius: 0; border: 1px solid var(--line); background: none; }
  :root[data-theme="paper"] .presets button.on { background: var(--text); color: var(--bg); }
  .board { position: relative; height: 200px; border-radius: 18px; background: var(--surface); padding: 18px 0 8px; }
  :root[data-theme="paper"] .board { border-radius: 0; border: 1px solid var(--line); background: none; }
  svg { position: absolute; left: 0; right: 0; top: 30px; width: 100%; height: 140px; pointer-events: none; }
  .zero { stroke: var(--line); stroke-dasharray: 3 4; }
  .line { fill: none; stroke: var(--accent); stroke-width: 2.5; vector-effect: non-scaling-stroke; }
  .area { fill: var(--accent); opacity: 0.12; }
  .cols { position: relative; display: grid; grid-template-columns: repeat(10, 1fr); height: 100%; padding: 0 2px; }
  .col { display: flex; flex-direction: column; align-items: center; gap: 4px; }
  .db { font-size: 10.5px; color: var(--text-2); height: 14px; }
  .track { position: relative; flex: 1; width: 28px; cursor: ns-resize; touch-action: none; }
  .track::before { content: ""; position: absolute; left: 50%; top: 0; bottom: 0; width: 2px; translate: -1px 0; background: color-mix(in srgb, var(--text) 10%, transparent); border-radius: 2px; }
  .knob { position: absolute; left: 50%; width: 14px; height: 14px; margin: -7px 0 0 -7px; border-radius: 50%; background: #fff; box-shadow: 0 0 0 4px var(--accent-soft), 0 2px 6px rgb(0 0 0 / 0.3); transition: top 0.08s; }
  :root[data-theme="paper"] .knob { border-radius: 0; background: var(--accent); box-shadow: none; }
  .f { font-size: 10.5px; }
  .pre { display: flex; align-items: center; gap: 14px; font-size: 13px; }
  .pre input { flex: 1; max-width: 260px; accent-color: var(--accent); }
</style>
