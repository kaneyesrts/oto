<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { app, saveSettings, refreshSources, requestWordSync } from "../store.svelte";
  import { api, type Hotkeys, type Settings, type ThemeId, type WordSyncStatus } from "../api";
  import { FolderPlus, Refresh, X } from "../icons";
  import Equalizer from "../components/Equalizer.svelte";
  import SpotifySetup from "../components/SpotifySetup.svelte";

  const s = $derived(app.settings!);

  const themes: { id: ThemeId; name: string; note: string; sw: string[] }[] = [
    { id: "aura", name: "Aura", note: "Всё приложение окрашивается под трек", sw: ["#3a2340", "#7a4a5a", "#ffffff"] },
    { id: "noir", name: "Noir", note: "Тёмная, плотная, акцент из обложки", sw: ["#0c0c0e", "#1e1e23", "#ff703d"] },
    { id: "mist", name: "Mist", note: "Розовая дымка и матовое стекло", sw: ["#f6cfda", "#ffffff", "#d64a80"] },
    { id: "liquid", name: "Liquid", note: "Стекло с преломлением поверх обложки", sw: ["#0a0d18", "#4b6bd6", "#ffffff"] },
    { id: "paper", name: "Paper", note: "Швейцарская типографика, ничего лишнего", sw: ["#f2efe8", "#111111", "#de3420"] },
  ];

  const set = (f: (x: Settings) => void) => saveSettings(f);

  // Spotify: optional own app (its own rate limit instead of librespot's shared one).
  let spGuide = $state(false);

  // Word-exact lyrics: needs a one-time speech model download.
  let ws = $state<WordSyncStatus | null>(null);
  const loadWs = () => api.wordsyncStatus().then((x) => (ws = x)).catch(() => (ws = null));
  loadWs();
  $effect(() => { if (app.wordsyncModel?.done) loadWs(); });
  const dl = $derived(app.wordsyncModel && !app.wordsyncModel.done && !app.wordsyncModel.error ? app.wordsyncModel : null);
  async function enableWordSync() {
    await set((x) => (x.lyrics.wordSync = true));
    if (ws && !ws.modelReady && !ws.downloading) api.wordsyncDownloadModel();
    else requestWordSync();
  }
  refreshSources();

  async function addFolder() {
    const dir = await open({ directory: true, multiple: false, title: "Папка с музыкой" });
    if (typeof dir === "string") api.addFolder(dir);
  }

  const hk: [keyof Hotkeys, string][] = [
    ["playPause", "Играть / пауза"], ["next", "Следующий трек"], ["prev", "Предыдущий трек"],
    ["volumeUp", "Громче"], ["volumeDown", "Тише"], ["like", "В избранное"],
    ["toggleWidget", "Показать / скрыть виджет"], ["toggleClickThrough", "Виджет: сквозь клики"],
    ["seekForward", "Вперёд на 10 с"], ["seekBack", "Назад на 10 с"],
  ];
  let capturing = $state<keyof Hotkeys | null>(null);
  function capture(e: KeyboardEvent, key: keyof Hotkeys) {
    e.preventDefault();
    if (e.key === "Escape") { capturing = null; return; }
    if (e.key === "Backspace" || e.key === "Delete") { set((x) => (x.hotkeys[key] = "")); capturing = null; return; }
    if (["Control", "Alt", "Shift", "Meta"].includes(e.key)) return;
    const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
    if (!mods.length && !/^F\d+$/.test(e.code) && !e.code.startsWith("Media")) return; // bare keys would steal typing
    const code = e.code.replace(/^Key/, "").replace(/^Digit/, "").replace(/^Arrow/, "");
    set((x) => (x.hotkeys[key] = [...mods, code].join("+")));
    capturing = null;
  }
</script>

{#snippet toggle(on: boolean, flip: () => void, label: string, hint = "")}
  <button class="opt" onclick={flip}>
    <span><span class="ol">{label}</span>{#if hint}<span class="oh faint">{hint}</span>{/if}</span>
    <span class="sw" class:on></span>
  </button>
{/snippet}

{#snippet slider(value: number, min: number, max: number, step: number, change: (v: number) => void, label: string, fmt: (v: number) => string)}
  <label class="opt">
    <span class="ol">{label}</span>
    <span class="sl"><input type="range" {min} {max} {step} {value} oninput={(e) => change(+e.currentTarget.value)} /><span class="num faint">{fmt(value)}</span></span>
  </label>
{/snippet}

{#snippet choice(value: string, options: [string, string][], change: (v: any) => void, label: string)}
  <div class="opt">
    <span class="ol">{label}</span>
    <span class="seg">{#each options as [v, l]}<button class:on={value === v} onclick={() => change(v)}>{l}</button>{/each}</span>
  </div>
{/snippet}

<div class="settings">
  <h1>Настройки</h1>

  <section>
    <h2>Тема</h2>
    <div class="themes">
      {#each themes as t}
        <button class="theme" class:on={s.theme === t.id} onclick={() => set((x) => (x.theme = t.id))}>
          <span class="prev" style="background:{t.sw[0]}">
            <i style="background:{t.sw[1]}"></i><i style="background:{t.sw[1]}"></i><b style="background:{t.sw[2]}"></b>
          </span>
          <span class="tn">{t.name}</span>
          <span class="faint tnote">{t.note}</span>
        </button>
      {/each}
    </div>
    <div class="opts">
      {@render toggle(s.accentFromCover, () => set((x) => (x.accentFromCover = !x.accentFromCover)), "Цвет из обложки", "Акцент интерфейса подстраивается под текущий трек")}
      {@render toggle(s.reducedMotion, () => set((x) => (x.reducedMotion = !x.reducedMotion)), "Спокойный режим", "Минимум анимаций")}
      {@render toggle(s.visual.particles, () => set((x) => (x.visual.particles = !x.visual.particles)), "Частицы в режиме лирики")}
      {@render slider(s.visual.intensity, 0, 1.5, 0.05, (v) => set((x) => (x.visual.intensity = v)), "Реакция на музыку", (v) => `${Math.round(v * 100)}%`)}
      {@render choice(s.visual.quality, [["high", "Высокое"], ["low", "Экономное"]], (v) => set((x) => (x.visual.quality = v)), "Качество эффектов")}
    </div>
  </section>

  <section>
    <h2>Библиотека</h2>
    <div class="folders">
      {#each s.folders as f}
        <div class="folder"><span class="ellipsis">{f}</span><button class="icon-btn" onclick={() => api.removeFolder(f)} aria-label="Убрать папку"><X size={15} /></button></div>
      {:else}
        <p class="muted">Папки не добавлены.</p>
      {/each}
    </div>
    <div class="row">
      <button class="pill solid" onclick={addFolder}><FolderPlus size={15} /> Добавить папку</button>
      <button class="pill ghost" onclick={() => api.rescan()}><Refresh size={15} /> Пересканировать</button>
    </div>
  </section>

  <section>
    <h2>Звук</h2>
    <div class="opts">
      {@render choice(s.audio.streamQuality, [["lossless", "Без потерь"], ["high", "Высокое"], ["normal", "Экономное"]], (v) => set((x) => (x.audio.streamQuality = v)), "Качество стриминга")}
      {@render toggle(s.audio.gapless, () => set((x) => (x.audio.gapless = !x.audio.gapless)), "Без пауз между треками", "Альбомы с непрерывным звуком играют слитно (для файлов)")}
      {@render slider(s.audio.crossfadeMs / 1000, 0, 12, 0.5, (v) => set((x) => (x.audio.crossfadeMs = Math.round(v * 1000))), "Кроссфейд", (v) => (v ? `${v} с` : "выкл"))}
      {@render toggle(s.audio.eqEnabled, () => set((x) => (x.audio.eqEnabled = !x.audio.eqEnabled)), "Эквалайзер", "Двойной клик по полосе сбрасывает её")}
    </div>
    <div class="eqwrap"><Equalizer /></div>
  </section>

  <section>
    <h2>Лирика</h2>
    <div class="opts">
      {@render toggle(s.lyrics.online, () => set((x) => (x.lyrics.online = !x.lyrics.online)), "Искать в интернете", "LRCLIB, если в файле и рядом с ним текста нет")}
      {@render toggle(s.lyrics.autoAlign, () => set((x) => (x.lyrics.autoAlign = !x.lyrics.autoAlign)), "Подгонять под трек", "Сдвиг по вокалу, если текст снят с другой версии песни")}
      {#if ws?.available}
        <div class="opt">
          <span>
            <span class="ol">Слова точно по голосу</span>
            <span class="oh faint">Плеер сам слушает песню и ставит каждое слово ровно туда, где его поют. Работает на этом компьютере, в фоне; текст без таймкодов тоже становится синхронным.</span>
            {#if dl}
              <span class="dl"><i style="transform:scaleX({dl.total ? dl.downloaded / dl.total : 0})"></i></span>
              <span class="oh faint num">Скачиваем модель распознавания: {Math.round(dl.downloaded / 1e6)} из {Math.round((dl.total || ws.modelSizeMb * 1e6) / 1e6)} МБ</span>
            {:else if ws.modelReady}
              <button class="link faint" onclick={() => { set((x) => (x.lyrics.wordSync = false)); api.wordsyncDeleteModel().then(loadWs); }}>Удалить модель ({ws.modelSizeMb} МБ)</button>
            {/if}
          </span>
          {#if !ws.modelReady && !dl}
            <button class="mini-btn" onclick={enableWordSync}>Включить · {ws.modelSizeMb} МБ</button>
          {:else}
            <button class="swbtn" onclick={() => (s.lyrics.wordSync ? set((x) => (x.lyrics.wordSync = false)) : enableWordSync())} aria-label="Слова точно по голосу"><span class="sw" class:on={s.lyrics.wordSync}></span></button>
          {/if}
        </div>
      {/if}
      {@render toggle(s.lyrics.depthBlur, () => set((x) => (x.lyrics.depthBlur = !x.lyrics.depthBlur)), "Глубина резкости", "Дальние строки размываются")}
      {@render slider(s.lyrics.fontScale, 0.7, 1.4, 0.05, (v) => set((x) => (x.lyrics.fontScale = v)), "Размер текста", (v) => `${Math.round(v * 100)}%`)}
      {@render choice(s.lyrics.style, [["classic", "Список"], ["edit", "Эдит"]], (v) => set((x) => (x.lyrics.style = v)), "Подача текста")}
      <p class="faint fxhint">Пространство, свечение, подсветку припева и финала удобнее настраивать прямо в режиме лирики: кнопка ✦ справа вверху, всё меняется на лету.</p>
    </div>
  </section>

  <section>
    <h2>Виджет</h2>
    <div class="opts">
      {@render toggle(s.widget.enabled, () => set((x) => (x.widget.enabled = !x.widget.enabled)), "Показывать виджет")}
      {@render choice(s.widget.layer, [["overlay", "Поверх окон"], ["desktop", "Под окнами"]], (v) => set((x) => (x.widget.layer = v)), "Слой")}
      {@render choice(s.widget.style, [["compact", "Компакт"], ["lyrics", "Строка лирики"], ["spectrum", "Спектр"]], (v) => set((x) => (x.widget.style = v)), "Вид")}
      {@render choice(s.widget.scene, [["cover", "Обложка"], ["aura", "Аура"], ["space", "Космос"], ["silk", "Шёлк"], ["depth", "Огни"], ["smoke", "Дым"]], (v) => set((x) => (x.widget.scene = v)), "Фон")}
      {@render choice(s.widget.lyricsStyle, [["line", "Плавно"], ["edit", "Эдит"]], (v) => set((x) => (x.widget.lyricsStyle = v)), "Строка лирики")}
      {@render choice(s.widget.color, [["white", "Белый"], ["cover", "Из обложки"]], (v) => set((x) => (x.widget.color = v)), "Цвет текста")}
      {@render slider(s.widget.textGlow, 0, 1, 0.05, (v) => set((x) => (x.widget.textGlow = v)), "Свечение текста", (v) => `${Math.round(v * 100)}%`)}
      {@render slider(s.widget.opacity, 0.2, 1, 0.02, (v) => set((x) => (x.widget.opacity = v)), "Непрозрачность", (v) => `${Math.round(v * 100)}%`)}
      {@render slider(s.widget.blur, 0, 40, 1, (v) => set((x) => (x.widget.blur = v)), "Размытие обложки", (v) => `${v}px`)}
      {@render slider(s.widget.scale, 0.75, 1.5, 0.05, (v) => set((x) => (x.widget.scale = v)), "Масштаб", (v) => `${Math.round(v * 100)}%`)}
      {@render toggle(s.widget.clickThrough, () => set((x) => (x.widget.clickThrough = !x.widget.clickThrough)), "Сквозь клики", `Мышь проходит сквозь виджет. Вернуть: ${s.hotkeys.toggleClickThrough || "меню трея"}`)}
      {@render toggle(s.widget.glow, () => set((x) => (x.widget.glow = !x.widget.glow)), "Свечение в такт")}
      {@render toggle(s.widget.spectrum, () => set((x) => (x.widget.spectrum = !x.widget.spectrum)), "Спектр")}
      {@render toggle(s.widget.particles, () => set((x) => (x.widget.particles = !x.widget.particles)), "Частицы")}
    </div>
  </section>

  <section>
    <h2>Горячие клавиши</h2>
    <p class="muted hint">Работают глобально, даже когда oto свёрнут. Нажмите на поле и затем сочетание; Backspace очищает.</p>
    <div class="opts">
      {#each hk as [k, label]}
        <div class="opt">
          <span class="ol">{label}</span>
          <button class="key num" class:cap={capturing === k} onclick={() => (capturing = k)} onkeydown={(e) => capturing === k && capture(e, k)} onblur={() => (capturing = null)}>
            {capturing === k ? "нажмите сочетание…" : s.hotkeys[k] || "—"}
          </button>
        </div>
      {/each}
    </div>
  </section>

  <section>
    <h2>Discord</h2>
    <div class="opts">
      {@render toggle(s.discord.enabled, () => set((x) => (x.discord.enabled = !x.discord.enabled)), "Показывать, что играет", "«Слушает» в профиле Discord: обложка, трек, исполнитель и полоса прогресса")}
      {@render toggle(s.discord.hidePaused, () => set((x) => (x.discord.hidePaused = !x.discord.hidePaused)), "Скрывать на паузе", "Иначе статус остаётся с пометкой «на паузе»")}
      {@render toggle(s.discord.buttons, () => set((x) => (x.discord.buttons = !x.discord.buttons)), "Кнопки", "«Слушать в Яндекс Музыке» и «oto на GitHub». Другие люди их видят, а в своём профиле Discord их не показывает")}
    </div>
  </section>

  <section>
    <h2>Система</h2>
    <div class="opts">
      {@render toggle(s.closeToTray, () => set((x) => (x.closeToTray = !x.closeToTray)), "Сворачивать в трей при закрытии", "Музыка продолжит играть, управление из трея и виджета")}
    </div>
  </section>

  <section>
    <h2>Источники</h2>
    <p class="muted hint">Музыка из сервисов играет через тот же движок: визуализация, лирика, эквалайзер и статистика работают одинаково. Одна и та же песня из разных источников показывается один раз, а при недоступности источника играет другая копия.</p>
    <div class="sources">
      <div class="src"><b>Локальные файлы</b><span class="ok">{s.folders.length} {s.folders.length === 1 ? "папка" : "папки"}</span></div>
      {#each app.sources as src (src.id)}
        <div class="src">
          <b>{src.name}</b>
          {#if src.connected}
            <span class="ok">{src.account ? `Вход: ${src.account}` : "Подключено"}</span>
            <div class="srow">
              <button class="mini-btn" onclick={() => api.sourceImportLiked(src.id)}>Импорт избранного</button>
              <button class="mini-btn ghost" onclick={() => api.sourceLogout(src.id)}>Выйти</button>
            </div>
          {:else}
            <span class="faint">{src.worksAnonymously ? "Поиск и публичные треки без входа" : "Нужен вход"}</span>
            <div class="srow"><button class="mini-btn" onclick={() => api.sourceLogin(src.id)}>Войти</button></div>
          {/if}
          {#if src.id === "spotify"}
            <button class="link faint tiny" onclick={() => (spGuide = true)}>{src.clientId ? "Своё приложение подключено · настроить" : "Как подключить Spotify (свой ключ)"}</button>
          {/if}
        </div>
      {/each}
    </div>
    <p class="faint note">API Яндекс Музыки и SoundCloud неофициальные и могут меняться. Токены хранятся только на этом компьютере.</p>
  </section>
</div>

{#if spGuide}<SpotifySetup onclose={() => (spGuide = false)} />{/if}

<style>
  .settings { height: 100%; overflow-y: auto; padding: 28px 32px 60px; max-width: 920px; }
  h1 { margin: 0 0 18px; font-family: var(--font-display); font-size: clamp(28px, 3vw, 40px); font-weight: 700; letter-spacing: -0.03em; }
  :root[data-theme="paper"] h1 { font-style: italic; font-size: 56px; }
  section { padding: 22px 0; border-top: 1px solid var(--line); }
  h2 { margin: 0 0 14px; font-size: 13px; letter-spacing: 0.1em; text-transform: uppercase; color: var(--text-2); font-weight: 650; }
  .hint { margin: -6px 0 10px; font-size: 13px; }
  .themes { display: grid; grid-template-columns: repeat(5, 1fr); gap: 12px; margin-bottom: 16px; }
  .theme { display: flex; flex-direction: column; gap: 6px; text-align: left; padding: 10px; border-radius: 16px; border: 1px solid var(--line); transition: border-color 0.2s, transform 0.25s var(--ease-spring); }
  .theme:hover { transform: translateY(-2px); }
  .theme.on { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .prev { position: relative; height: 74px; border-radius: 10px; display: flex; gap: 5px; padding: 8px; overflow: hidden; }
  .prev i { flex: 1; border-radius: 5px; opacity: 0.55; }
  .prev b { position: absolute; right: 10px; bottom: 10px; width: 18px; height: 18px; border-radius: 50%; }
  .tn { font-weight: 650; margin-top: 2px; }
  .tnote { font-size: 11.5px; line-height: 1.3; }
  .opts { display: flex; flex-direction: column; }
  .opt { display: flex; align-items: center; justify-content: space-between; gap: 20px; min-height: 48px; padding: 6px 0; text-align: left; width: 100%; }
  .opt + .opt { border-top: 1px solid color-mix(in srgb, var(--line) 60%, transparent); }
  .ol { display: block; font-weight: 550; }
  .oh { display: block; font-size: 12px; margin-top: 2px; }
  .sw { flex: none; width: 40px; height: 24px; border-radius: 999px; background: color-mix(in srgb, var(--text) 18%, transparent); position: relative; transition: background 0.2s; }
  .sw::after { content: ""; position: absolute; top: 3px; left: 3px; width: 18px; height: 18px; border-radius: 50%; background: #fff; box-shadow: 0 1px 3px rgb(0 0 0 / 0.3); transition: transform 0.3s var(--ease-spring); }
  .sw.on { background: var(--accent); }
  .sw.on::after { transform: translateX(16px); }
  :root[data-theme="paper"] .sw, :root[data-theme="paper"] .sw::after { border-radius: 0; }
  .sl { display: flex; align-items: center; gap: 12px; }
  .sl span { width: 44px; text-align: right; font-size: 12px; }
  input[type="range"] { -webkit-appearance: none; appearance: none; width: 200px; height: 4px; border-radius: 4px; background: color-mix(in srgb, var(--text) 15%, transparent); outline: none; }
  input[type="range"]::-webkit-slider-thumb { -webkit-appearance: none; width: 16px; height: 16px; border-radius: 50%; background: var(--accent); cursor: pointer; box-shadow: 0 0 0 4px var(--accent-soft); }
  .seg { display: flex; padding: 3px; border-radius: 999px; background: var(--surface); flex: none; }
  .seg button { height: 28px; padding: 0 12px; border-radius: 999px; font-size: 12.5px; color: var(--text-2); }
  .seg button.on { background: var(--surface-2); color: var(--text); }
  :root[data-theme="paper"] .seg { border-radius: 0; border: 1px solid var(--line); padding: 0; background: none; }
  :root[data-theme="paper"] .seg button { border-radius: 0; }
  :root[data-theme="paper"] .seg button.on { background: var(--text); color: var(--bg); }
  .folders { display: flex; flex-direction: column; gap: 4px; margin-bottom: 12px; }
  .folder { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 6px 6px 6px 14px; border-radius: 12px; background: var(--surface); font-size: 13px; }
  .row { display: flex; gap: 10px; }
  .key { min-width: 170px; height: 32px; padding: 0 12px; border-radius: 8px; background: var(--surface); border: 1px solid var(--line); font-size: 12.5px; }
  .key.cap { border-color: var(--accent); color: var(--accent); }
  .sources { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 10px; }
  .src { display: flex; flex-direction: column; gap: 2px; padding: 12px 14px; border-radius: 14px; background: var(--surface); font-size: 12.5px; }
  .src b { font-size: 14px; }
  .ok { color: var(--accent); }
  .eqwrap { margin-top: 14px; }
  .srow { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; }
  .mini-btn { flex: none; height: 28px; padding: 0 12px; border-radius: 999px; font-size: 12px; font-weight: 600; background: var(--text); color: var(--bg); }
  .mini-btn.ghost { background: transparent; color: var(--text-2); border: 1px solid var(--line); }
  :root[data-theme="paper"] .mini-btn { border-radius: 0; }
  .note { font-size: 12px; margin-top: 12px; }
  .tiny { font-size: 11.5px; line-height: 1.35; }
  .fxhint { font-size: 12px; margin: 8px 0 0; }
  .dl { display: block; height: 4px; border-radius: 4px; margin: 8px 0 4px; max-width: 320px; background: color-mix(in srgb, var(--text) 12%, transparent); overflow: hidden; }
  .dl i { display: block; height: 100%; background: var(--accent); transform-origin: left; transition: transform 0.3s; }
  .link { font-size: 12px; margin-top: 4px; text-decoration: underline; text-underline-offset: 2px; }
  .swbtn { flex: none; }
</style>
