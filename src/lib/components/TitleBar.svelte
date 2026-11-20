<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { app } from "../store.svelte";
  import { Search, Minus, Square, X } from "../icons";

  const win = getCurrentWindow();
  let input: HTMLInputElement;

  function onKey(e: KeyboardEvent) {
    if ((e.ctrlKey && e.key.toLowerCase() === "f") || (e.key === "/" && document.activeElement?.tagName !== "INPUT")) {
      e.preventDefault();
      input?.focus();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<header class="title" data-tauri-drag-region>
  <div class="brand" data-tauri-drag-region>
    <span class="mark">oto</span>
  </div>
  <label class="search">
    <Search size={15} />
    <input
      bind:this={input}
      bind:value={app.search}
      placeholder="Поиск по трекам, артистам, альбомам"
      onfocus={() => { if (app.view !== "library") app.view = "library"; }}
      onkeydown={(e) => { if (e.key === "Escape") { app.search = ""; input.blur(); } }}
    />
    <kbd>Ctrl F</kbd>
  </label>
  <div class="spacer" data-tauri-drag-region></div>
  <div class="win">
    <button aria-label="Свернуть" onclick={() => win.minimize()}><Minus size={15} /></button>
    <button aria-label="Развернуть" onclick={() => win.toggleMaximize()}><Square size={12} /></button>
    <button aria-label="Закрыть" class="close" onclick={() => win.close()}><X size={16} /></button>
  </div>
</header>

<style>
  .title {
    grid-area: title;
    display: flex;
    align-items: center;
    gap: 16px;
    height: var(--title-h);
    padding-left: 20px;
    position: relative;
    z-index: 5;
  }
  .brand { width: calc(var(--sidebar-w) - 36px); display: flex; align-items: center; }
  .mark {
    font-family: var(--font-display);
    font-weight: 700;
    font-size: 19px;
    letter-spacing: -0.02em;
  }
  :root[data-theme="noir"] .mark { letter-spacing: 0.04em; font-weight: 600; }
  :root[data-theme="paper"] .mark { font-style: italic; font-size: 24px; font-weight: 800; }
  .search {
    display: flex;
    align-items: center;
    gap: 10px;
    width: min(460px, 40vw);
    height: 34px;
    padding: 0 12px;
    border-radius: 999px;
    background: var(--surface);
    color: var(--text-3);
    border: 1px solid transparent;
    transition: border-color 0.2s, background 0.2s;
  }
  .search:focus-within { border-color: color-mix(in srgb, var(--accent) 60%, transparent); color: var(--text-2); }
  :root[data-theme="mist"] .search { background: rgb(255 255 255 / 0.55); backdrop-filter: blur(20px); }
  :root[data-theme="liquid"] .search { background: rgb(255 255 255 / 0.08); }
  :root[data-theme="paper"] .search { border-radius: 0; background: transparent; border: 0; border-bottom: 1px solid var(--text); }
  input { flex: 1; min-width: 0; background: none; border: 0; outline: none; color: var(--text); font-size: 13px; }
  input::placeholder { color: var(--text-3); }
  kbd { font-family: var(--font-num); font-size: 10px; color: var(--text-3); border: 1px solid var(--line); border-radius: 5px; padding: 1px 5px; }
  .spacer { flex: 1; align-self: stretch; }
  .win { display: flex; align-self: stretch; }
  .win button { width: 46px; display: grid; place-items: center; color: var(--text-2); transition: background 0.12s, color 0.12s; }
  .win button:hover { background: var(--hover); color: var(--text); }
  .win .close:hover { background: #e5484d; color: #fff; }
</style>
