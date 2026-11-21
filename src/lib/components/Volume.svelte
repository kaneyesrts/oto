<script lang="ts">
  // Volume for places outside the player bar (lyrics view, widget): icon toggles mute,
  // the slider drags, the mouse wheel nudges. `compact` keeps only the icon until hovered.
  import { app, player } from "../store.svelte";
  import { Volume2, Volume1, VolumeX } from "../icons";

  let { size = 18, compact = false, width = 96 }: { size?: number; compact?: boolean; width?: number } = $props();

  let el: HTMLDivElement | undefined = $state();
  let open = $state(false);
  let closeTimer = 0;
  let lastLoud = 0;
  const v = $derived(app.player.volume);

  const set = (x: number) => player.volume(Math.min(1, Math.max(0, Math.round(x * 100) / 100)));
  function drag(e: PointerEvent) {
    if (!el || (e.type === "pointermove" && !(e.buttons & 1))) return;
    if (e.type === "pointerdown") (e.currentTarget as Element).setPointerCapture(e.pointerId);
    const r = el.getBoundingClientRect();
    set((e.clientX - r.left) / r.width);
  }
  function wheel(e: WheelEvent) {
    e.preventDefault();
    e.stopPropagation();
    set(v - Math.sign(e.deltaY) * 0.04);
  }
  function toggle() {
    if (v > 0) {
      lastLoud = v;
      set(0);
    } else set(lastLoud || 0.6);
  }
  function key(e: KeyboardEvent) {
    if (e.key === "ArrowRight" || e.key === "ArrowUp") set(v + 0.05);
    else if (e.key === "ArrowLeft" || e.key === "ArrowDown") set(v - 0.05);
    else return;
    e.preventDefault();
  }
</script>

<div
  class="vol"
  class:compact
  class:open
  style="--w:{width}px"
  role="group"
  aria-label="Громкость"
  onwheel={wheel}
  onpointerenter={() => { clearTimeout(closeTimer); open = true; }}
  onpointerleave={() => { closeTimer = window.setTimeout(() => (open = false), 700); }}
>
  <button class="vbtn" onclick={toggle} aria-label={v === 0 ? "Включить звук" : "Выключить звук"} title="Громкость {Math.round(v * 100)}% · колесо мыши">
    {#if v === 0}<VolumeX {size} />{:else if v < 0.5}<Volume1 {size} />{:else}<Volume2 {size} />{/if}
  </button>
  <div class="track" bind:this={el} onpointerdown={drag} onpointermove={drag} onkeydown={key} role="slider" tabindex="0" aria-label="Громкость" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(v * 100)}>
    <div class="rail"><div class="fill" style="width:{v * 100}%"></div><div class="knob" style="left:{v * 100}%"></div></div>
  </div>
</div>

<style>
  .vol { display: flex; align-items: center; gap: 2px; color: inherit; flex: none; }
  .vbtn { flex: none; width: 34px; height: 34px; border-radius: 50%; display: grid; place-items: center; color: inherit; opacity: 0.75; transition: opacity 0.15s, background 0.15s; }
  .vbtn:hover { opacity: 1; background: rgb(127 127 127 / 0.15); }
  .track { width: var(--w); height: 26px; display: flex; align-items: center; cursor: pointer; touch-action: none; outline: none; }
  .rail { position: relative; width: 100%; height: 4px; border-radius: 4px; background: rgb(127 127 127 / 0.32); }
  .fill { height: 100%; border-radius: 4px; background: currentColor; }
  .knob { position: absolute; top: 50%; width: 12px; height: 12px; border-radius: 50%; background: currentColor; transform: translate(-50%, -50%) scale(0); transition: transform 0.15s; }
  .vol:hover .knob, .track:focus-visible .knob { transform: translate(-50%, -50%) scale(1); }
  .compact .track { width: 0; opacity: 0; pointer-events: none; transition: width 0.25s var(--ease-out, ease-out), opacity 0.2s; }
  .compact.open .track { width: var(--w); opacity: 1; pointer-events: auto; }
</style>
