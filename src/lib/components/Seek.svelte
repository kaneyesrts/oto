<script lang="ts">
  import { app, clock, player } from "../store.svelte";
  import { fmtTime } from "../api";

  let { showTimes = true, thin = false } = $props();

  let el: HTMLDivElement;
  let drag = $state<number | null>(null);
  let hover = $state<number | null>(null);
  const dur = $derived(app.player.durationMs || 1);
  const frac = $derived(drag ?? Math.min(1, clock.pos / dur));

  const at = (e: PointerEvent) => {
    const r = el.getBoundingClientRect();
    return Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
  };
  function down(e: PointerEvent) {
    if (!app.player.track) return;
    el.setPointerCapture(e.pointerId);
    drag = at(e);
  }
  function move(e: PointerEvent) {
    hover = at(e);
    if (drag !== null) drag = at(e);
  }
  function up() {
    if (drag !== null) player.seek(drag * dur);
    drag = null;
  }
</script>

<div class="seek" class:thin>
  {#if showTimes}<span class="t num">{fmtTime(drag !== null ? drag * dur : clock.pos)}</span>{/if}
  <div
    class="track"
    class:dragging={drag !== null}
    bind:this={el}
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointerleave={() => (hover = null)}
    onwheel={(e) => { e.preventDefault(); player.seekBy(e.deltaY > 0 ? -5000 : 5000); }}
    role="slider"
    tabindex="0"
    aria-label="Позиция"
    aria-valuenow={Math.round(frac * 100)}
  >
    <div class="rail">
      <div class="fill" style="transform:scaleX({frac})"></div>
    </div>
    <div class="knob" style="left:{frac * 100}%"></div>
    {#if hover !== null && drag === null}
      <div class="tip num" style="left:{hover * 100}%">{fmtTime(hover * dur)}</div>
    {/if}
  </div>
  {#if showTimes}<span class="t num right">{fmtTime(app.player.durationMs)}</span>{/if}
</div>

<style>
  .seek { display: flex; align-items: center; gap: 12px; width: 100%; }
  .t { font-size: 11px; color: var(--text-3); min-width: 34px; }
  .right { text-align: right; }
  .track { position: relative; flex: 1; height: 22px; display: flex; align-items: center; cursor: pointer; touch-action: none; }
  .rail { position: relative; width: 100%; height: 4px; border-radius: 4px; background: color-mix(in srgb, var(--text) 12%, transparent); overflow: hidden; transition: height 0.2s var(--ease-spring); }
  .thin .rail { height: 3px; }
  .track:hover .rail, .dragging .rail { height: 6px; }
  .fill { position: absolute; inset: 0; background: var(--accent); transform-origin: left; border-radius: inherit; }
  .knob {
    position: absolute; top: 50%; width: 12px; height: 12px; margin: -6px 0 0 -6px; border-radius: 50%;
    background: #fff; box-shadow: 0 0 0 4px var(--accent-soft), 0 2px 6px rgb(0 0 0 / 0.3);
    transform: scale(0); transition: transform 0.2s var(--ease-spring);
  }
  .track:hover .knob, .dragging .knob { transform: scale(1); }
  .tip {
    position: absolute; bottom: 20px; transform: translateX(-50%); padding: 3px 7px; border-radius: 6px;
    font-size: 11px; background: var(--surface-2); color: var(--text); pointer-events: none; white-space: nowrap;
  }
  :root[data-theme="paper"] .rail { border-radius: 0; height: 2px; background: color-mix(in srgb, var(--text) 20%, transparent); }
  :root[data-theme="paper"] .fill { background: var(--text); }
  :root[data-theme="paper"] .knob { border-radius: 0; width: 3px; height: 14px; margin: -7px 0 0 -1px; background: var(--accent); box-shadow: none; transform: none; }
</style>
