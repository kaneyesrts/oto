<script lang="ts">
  import { app, player, saveSettings } from "../lib/store.svelte";
  import { api, coverUrl } from "../lib/api";
  import Seek from "../lib/components/Seek.svelte";
  import Spectrum from "../lib/components/Spectrum.svelte";
  import { Play, Pause, SkipBack, SkipForward, Heart, AppWindow, Layers, Shuffle, Repeat, Repeat1 } from "../lib/icons";

  const t = $derived(app.player.track);
  const cover = $derived(coverUrl(t?.cover));
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && api.hideFlyout()} />

<div class="wrap">
  <div class="card">
    {#if cover}<img class="bgart" src={cover} alt="" />{/if}
    <div class="art">
      {#if cover}{#key cover}<img class="cover" src={cover} alt="" />{/key}{:else}<div class="cover"></div>{/if}
      <div class="spec"><Spectrum bars={28} gap={3} radius={2} color="#fff" /></div>
    </div>
    <div class="meta">
      <div class="tt ellipsis">{t?.title ?? "Ничего не играет"}</div>
      <div class="ar ellipsis">{t?.artist ?? "Откройте oto, чтобы выбрать музыку"}</div>
    </div>
    <Seek />
    <div class="ctl">
      <button class="sm" class:on={app.player.shuffle} onclick={player.shuffle} aria-label="Перемешать"><Shuffle size={15} /></button>
      <button onclick={player.prev} aria-label="Назад"><SkipBack size={19} fill="currentColor" /></button>
      <button class="pp" onclick={player.toggle} aria-label="Играть/пауза">
        {#if app.player.playing}<Pause size={20} fill="currentColor" strokeWidth={0} />{:else}<Play size={20} fill="currentColor" strokeWidth={0} />{/if}
      </button>
      <button onclick={player.next} aria-label="Вперёд"><SkipForward size={19} fill="currentColor" /></button>
      <button class="sm" class:on={app.player.repeat !== "off"} onclick={player.repeat} aria-label="Повтор">
        {#if app.player.repeat === "one"}<Repeat1 size={15} />{:else}<Repeat size={15} />{/if}
      </button>
    </div>
    <div class="foot">
      <button class:on={t?.liked} onclick={() => player.like()}><Heart size={14} fill={t?.liked ? "currentColor" : "none"} /> Избранное</button>
      <button class:on={app.settings?.widget.enabled} onclick={() => saveSettings((s) => (s.widget.enabled = !s.widget.enabled))}><Layers size={14} /> Виджет</button>
      <button onclick={() => { api.showMain(); api.hideFlyout(); }}><AppWindow size={14} /> Открыть</button>
    </div>
  </div>
</div>

<style>
  :global(html), :global(body) { background: transparent !important; }
  .wrap { position: fixed; inset: 0; padding: 8px; display: flex; }
  .card {
    position: relative; flex: 1; overflow: hidden;
    display: flex; flex-direction: column; gap: 14px;
    padding: 16px 18px 14px;
    border-radius: 20px;
    background: rgb(18 18 22 / 0.94);
    color: #fff;
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.09);
    animation: pop 0.32s var(--ease-spring);
    font-family: "Onest Variable", system-ui, sans-serif;
  }
  @keyframes pop { from { opacity: 0; transform: translateY(10px) scale(0.97); } }
  .bgart { position: absolute; inset: -30%; width: 160%; height: 160%; object-fit: cover; filter: blur(50px) saturate(1.6); opacity: 0.45; pointer-events: none; }
  .art { position: relative; }
  .art .cover { width: 100%; height: 210px; border-radius: 14px; object-fit: cover; box-shadow: 0 20px 40px -20px rgb(0 0 0 / 0.8); }
  .spec { position: absolute; left: 12px; right: 12px; bottom: 10px; height: 34px; opacity: 0.85; mix-blend-mode: overlay; }
  .meta { position: relative; }
  .tt { font-weight: 700; font-size: 16px; letter-spacing: -0.01em; }
  .ar { font-size: 12.5px; opacity: 0.65; margin-top: 2px; }
  .card :global(.seek) { position: relative; }
  .card :global(.seek .t) { color: rgb(255 255 255 / 0.5); }
  .card :global(.rail) { background: rgb(255 255 255 / 0.16) !important; }
  .card :global(.fill) { background: #fff !important; }
  .ctl { position: relative; display: flex; align-items: center; justify-content: center; gap: 10px; margin-top: -4px; }
  .ctl button { width: 40px; height: 40px; border-radius: 50%; display: grid; place-items: center; color: rgb(255 255 255 / 0.85); transition: background 0.15s, transform 0.2s var(--ease-spring); }
  .ctl button.sm { width: 32px; height: 32px; opacity: 0.6; }
  .ctl button.on { color: rgb(var(--accent-rgb, var(--accent-default))); opacity: 1; }
  .ctl button:hover { background: rgb(255 255 255 / 0.1); }
  .ctl .pp { width: 50px; height: 50px; background: #fff; color: #111; }
  .ctl .pp:hover { background: #fff; transform: scale(1.06); }
  .foot { position: relative; display: grid; grid-template-columns: repeat(3, 1fr); gap: 6px; }
  .foot button { display: flex; align-items: center; justify-content: center; gap: 6px; height: 32px; border-radius: 10px; font-size: 12px; background: rgb(255 255 255 / 0.07); color: rgb(255 255 255 / 0.8); }
  .foot button:hover { background: rgb(255 255 255 / 0.13); color: #fff; }
  .foot button.on { color: rgb(var(--accent-rgb, var(--accent-default))); }
</style>
