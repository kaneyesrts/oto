<script lang="ts">
  // «Поток»: endless music built from what you listen to most.
  import { onMount } from "svelte";
  import { app, station, startStation, stopStation, saveSettings, player, STATION_MODES, wantArtistImages } from "../store.svelte";
  import { api, coverUrl, type StationMode } from "../api";
  import CoverStack from "./CoverStack.svelte";
  import { Play, Pause, Settings } from "../icons";

  const cfg = $derived(app.settings!.station);
  const mode = $derived(STATION_MODES.find((m) => m.id === cfg.mode) ?? STATION_MODES[0]);
  let basis = $state<string[]>([]);
  let tuning = $state(false);

  onMount(async () => {
    basis = station.basis.length ? station.basis : await api.stationBasis().catch(() => []);
    wantArtistImages(basis);
  });

  // Covers: the latest batch when playing, else your most played tracks' covers.
  const fallbackCovers = $derived(
    basis.map((a) => app.artistImages[a]).filter(Boolean).concat(app.tracks.filter((t) => t.liked && t.cover).slice(0, 5).map((t) => t.cover!))
  );
  const covers = $derived(station.covers.length ? station.covers : fallbackCovers);

  function setMode(m: StationMode) {
    saveSettings((s) => (s.station.mode = m));
    if (station.active) { stopStation(); setTimeout(startStation, 50); }
  }
  function toggle() {
    if (station.active) player.toggle();
    else startStation();
  }
</script>

<section class="card hoverable" class:on={station.active}>
  <div class="glow" style={covers[0] ? `background-image:url("${coverUrl(covers[0])}")` : ""}></div>
  <div class="left">
    <div class="eyebrow">Поток{station.active ? " · играет" : ""}</div>
    <h2>{mode.name}</h2>
    <p class="muted">{mode.note}</p>
    {#if basis.length && cfg.mode !== "random"}
      <div class="basis">
        <span class="avatars">
          {#each basis.slice(0, 4) as a}
            {#if app.artistImages[a]}<img src={coverUrl(app.artistImages[a])} alt="" title={a} />{:else}<i title={a}>{a.slice(0, 1)}</i>{/if}
          {/each}
        </span>
        <span class="faint ellipsis">на основе: {basis.slice(0, 3).join(", ")}</span>
      </div>
    {/if}
    <div class="modes">
      {#each STATION_MODES as m}
        <button class:on={cfg.mode === m.id} onclick={() => setMode(m.id)}>{m.name}</button>
      {/each}
      <button class="tune" class:on={tuning} onclick={() => (tuning = !tuning)} aria-label="Настроить"><Settings size={14} /></button>
    </div>
    {#if tuning}
      <div class="tuning">
        {#if cfg.mode === "style"}
          <label>
            <span>Знакомое</span>
            <input type="range" min="0" max="1" step="0.05" value={cfg.novelty} onchange={(e) => saveSettings((s) => (s.station.novelty = +e.currentTarget.value))} />
            <span>Новое</span>
          </label>
        {/if}
        <label class="chk">
          <input type="checkbox" checked={cfg.excludeRecent} onchange={(e) => saveSettings((s) => (s.station.excludeRecent = e.currentTarget.checked))} />
          <span>Не повторять то, что слушали в последние 3 дня</span>
        </label>
        <p class="faint">Пропуск трека в потоке убирает его исполнителя до конца сессии.</p>
      </div>
    {/if}
  </div>
  <div class="right">
    <CoverStack {covers} size={118} spin={station.active && app.player.playing} />
    <button class="play" onclick={toggle} aria-label={station.active && app.player.playing ? "Пауза" : "Запустить поток"} disabled={station.loading}>
      {#if station.loading}<span class="spinner"></span>
      {:else if station.active && app.player.playing}<Pause size={24} fill="currentColor" strokeWidth={0} />
      {:else}<Play size={24} fill="currentColor" strokeWidth={0} />{/if}
    </button>
  </div>
</section>

<style>
  .card {
    position: relative; overflow: hidden; flex: none; display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 24px; align-items: center;
    padding: 26px 28px; border-radius: var(--radius-xl); background: var(--surface);
    transition: box-shadow 0.4s;
  }
  :root[data-theme="aura"] .card { background: rgb(255 255 255 / 0.06); backdrop-filter: blur(30px) saturate(1.3); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.08); }
  :root[data-theme="paper"] .card { border: 1px solid var(--line); background: none; }
  .card.on { box-shadow: 0 0 0 1px rgb(var(--accent-rgb, var(--accent-default)) / 0.4), 0 30px 60px -30px rgb(var(--accent-rgb, var(--accent-default)) / 0.5); }
  .glow { position: absolute; inset: -30%; background-size: cover; background-position: center; filter: blur(60px) saturate(1.5); opacity: 0.35; pointer-events: none; }
  :root[data-theme="paper"] .glow { display: none; }
  .left, .right { position: relative; min-width: 0; }
  h2 { margin: 6px 0 4px; font-family: var(--font-display); font-size: clamp(26px, 2.6vw, 38px); font-weight: 800; letter-spacing: -0.03em; line-height: 1; }
  :root[data-theme="paper"] h2 { font-style: italic; }
  p { margin: 0; font-size: 13px; }
  .basis { display: flex; align-items: center; gap: 10px; margin-top: 12px; font-size: 12.5px; min-width: 0; }
  .avatars { display: flex; flex: none; }
  .avatars img, .avatars i { width: 26px; height: 26px; border-radius: 50%; object-fit: cover; margin-left: -7px; box-shadow: 0 0 0 2px var(--bg); display: grid; place-items: center; font-style: normal; font-size: 11px; font-weight: 700; background: var(--surface-2); }
  .avatars :first-child { margin-left: 0; }
  .modes { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 16px; }
  .modes button { height: 30px; padding: 0 13px; border-radius: 999px; font-size: 12.5px; font-weight: 600; background: rgb(127 127 127 / 0.14); color: var(--text-2); transition: background 0.15s, color 0.15s; }
  .modes button:hover { color: var(--text); }
  .modes button.on { background: var(--text); color: var(--bg); }
  .modes .tune { width: 30px; padding: 0; display: grid; place-items: center; }
  :root[data-theme="paper"] .modes button { border-radius: 0; border: 1px solid var(--line); background: none; }
  :root[data-theme="paper"] .modes button.on { background: var(--text); color: var(--bg); }
  .tuning { margin-top: 14px; display: flex; flex-direction: column; gap: 10px; font-size: 12.5px; max-width: 420px; }
  .tuning label { display: flex; align-items: center; gap: 10px; }
  .tuning input[type="range"] { flex: 1; accent-color: var(--accent); }
  .chk input { accent-color: var(--accent); }
  .tuning p { font-size: 11.5px; }
  .right { display: flex; align-items: center; justify-content: center; }
  .play {
    position: absolute; right: 50%; bottom: -8px; translate: 50% 0; z-index: 20;
    width: 58px; height: 58px; border-radius: 50%; display: grid; place-items: center;
    background: var(--text); color: var(--bg); box-shadow: 0 14px 30px -12px rgb(0 0 0 / 0.7);
    transition: transform 0.25s var(--ease-spring);
  }
  .play:hover { transform: scale(1.07); }
  :root[data-theme="aura"] .play { background: #fff; color: #0b0b0e; }
  :root[data-theme="mist"] .play { background: var(--accent); color: #fff; }
  :root[data-theme="paper"] .play { border-radius: 0; }
  .spinner { width: 20px; height: 20px; border-radius: 50%; border: 2.5px solid currentColor; border-right-color: transparent; animation: rot 0.8s linear infinite; }
  @keyframes rot { to { transform: rotate(360deg); } }
  @media (max-width: 1100px) { .card { grid-template-columns: 1fr; } .right { justify-content: flex-start; } }
</style>
