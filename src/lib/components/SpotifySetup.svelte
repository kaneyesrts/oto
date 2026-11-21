<script lang="ts">
  // Step-by-step: registering your own Spotify app, so oto gets its own API rate limit
  // instead of the shared one every librespot client fights over.
  import { fade, fly } from "svelte/transition";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { app, refreshSources } from "../store.svelte";
  import { api } from "../api";
  import { X, Copy } from "../icons";

  let { onclose }: { onclose: () => void } = $props();

  const DASHBOARD = "https://developer.spotify.com/dashboard";
  const REDIRECT = "http://127.0.0.1:8898/login";
  const sp = $derived(app.sources.find((s) => s.id === "spotify"));

  let clientId = $state(app.sources.find((s) => s.id === "spotify")?.clientId ?? "");
  let error = $state("");
  let saved = $state(false);
  let copied = $state("");

  async function open(url: string) {
    try {
      await openUrl(url);
    } catch {
      window.open(url, "_blank");
    }
  }
  async function copy(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = what;
      setTimeout(() => (copied = ""), 1600);
    } catch {
      /* clipboard unavailable: the text is selectable anyway */
    }
  }
  async function save() {
    error = "";
    saved = false;
    const id = clientId.trim();
    if (id && !/^[0-9a-f]{32}$/i.test(id)) {
      error = "Client ID — это 32 символа из цифр и букв a–f. Скопируй его со страницы Settings → Basic Information.";
      return;
    }
    try {
      await api.sourceSetSpotifyClient(clientId.trim());
      await refreshSources();
      saved = true;
    } catch (e) {
      error = String(e);
    }
  }
  // Mounted on <body>: inside the settings page a fixed overlay would sit under the side panels.
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy: () => node.remove() };
  }
  function key(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={key} />

<div class="portal" use:portal>
<div class="scrim" transition:fade={{ duration: 160 }} onclick={onclose} role="presentation"></div>
<div class="sheet" role="dialog" aria-modal="true" aria-label="Подключение Spotify" transition:fly={{ y: 16, duration: 240 }}>
  <header>
    <div>
      <h2>Подключение Spotify</h2>
      <p class="muted">Своё приложение Spotify даёт oto собственный лимит запросов. Общий ключ делят тысячи программ, поэтому через него Spotify часто отвечает «Too Many Requests».</p>
    </div>
    <button class="close" onclick={onclose} aria-label="Закрыть"><X size={18} /></button>
  </header>

  <div class="body">
    <div class="need">
      <b>Что понадобится</b>
      <span>Аккаунт Spotify <b>Premium</b>: без него Spotify не даёт создать приложение с Web API, а librespot не играет музыку. Займёт 3–5 минут.</span>
    </div>

    <ol class="steps">
      <li>
        <span class="n">1</span>
        <div class="st">
          <h3>Открой панель разработчика Spotify</h3>
          <p>Войди тем же аккаунтом, которым слушаешь музыку. Если попросят принять условия для разработчиков, прими.</p>
          <button class="mini-btn" onclick={() => open(DASHBOARD)}>Открыть developer.spotify.com</button>
        </div>
      </li>
      <li>
        <span class="n">2</span>
        <div class="st">
          <h3>Создай приложение</h3>
          <p>Нажми <b>Create app</b> и заполни форму:</p>
          <dl>
            <dt>App name</dt><dd><code>oto</code> или любое другое</dd>
            <dt>App description</dt><dd>что угодно, например <code>Personal music player</code></dd>
            <dt>Redirect URIs</dt>
            <dd>
              <span class="copyrow"><code class="big">{REDIRECT}</code><button class="icon-btn mini" onclick={() => copy(REDIRECT, "uri")} aria-label="Скопировать"><Copy size={14} /></button>{#if copied === "uri"}<span class="ok">скопировано</span>{/if}</span>
              вставь ровно так и нажми <b>Add</b>. Именно <code>127.0.0.1</code>, не <code>localhost</code>, и с <code>/login</code> в конце.
            </dd>
            <dt>Which API/SDKs</dt><dd>отметь <b>Web API</b></dd>
          </dl>
          <p>Поставь галочку согласия с условиями и нажми <b>Save</b>.</p>
        </div>
      </li>
      <li>
        <span class="n">3</span>
        <div class="st">
          <h3>Добавь себя в пользователи</h3>
          <p>В приложении открой <b>Settings → User Management</b>, впиши своё имя и почту аккаунта Spotify, нажми <b>Add user</b>. Новое приложение работает в режиме разработки и пускает только тех, кто в этом списке (до 25 человек), поэтому без этого шага вход не пройдёт.</p>
        </div>
      </li>
      <li>
        <span class="n">4</span>
        <div class="st">
          <h3>Вставь Client ID</h3>
          <p>На вкладке <b>Settings → Basic Information</b> скопируй <b>Client ID</b> и вставь сюда. <b>Client Secret</b> не нужен, никому его не давай.</p>
          <div class="idrow">
            <input class="idin num" bind:value={clientId} placeholder="например 1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d" spellcheck="false" onkeydown={(e) => e.key === "Enter" && save()} />
            <button class="mini-btn" onclick={save}>Сохранить</button>
          </div>
          {#if error}<p class="err">{error}</p>{/if}
          {#if saved}<p class="ok">{clientId.trim() ? "Сохранено. Теперь войди в Spotify — следующий шаг." : "Вернулись к общему ключу."}</p>{/if}
        </div>
      </li>
      <li>
        <span class="n">5</span>
        <div class="st">
          <h3>Войди в Spotify</h3>
          <p>Откроется браузер со страницей Spotify: разреши доступ, и oto подключится. После смены Client ID входить нужно заново.</p>
          {#if sp?.connected}
            <p class="ok">Подключено{sp.account ? `: ${sp.account}` : ""}.</p>
          {:else}
            <button class="mini-btn" onclick={() => api.sourceLogin("spotify")}>Войти в Spotify</button>
          {/if}
        </div>
      </li>
    </ol>

    <details class="faq">
      <summary>Если что-то не получилось</summary>
      <dl>
        <dt>Галочка Web API серая и не нажимается</dt>
        <dd>У аккаунта нет Premium, или Spotify временно приостановил создание новых приложений. Наведи мышку на галочку или кнопку: Spotify обычно пишет причину. Если приостановлено, попробуй позже.</dd>
        <dt>«INVALID_CLIENT: Invalid redirect URI»</dt>
        <dd>Redirect URI в приложении не совпадает с <code>{REDIRECT}</code>. Скопируй его кнопкой выше и проверь, что нет пробелов и лишнего слэша.</dd>
        <dt>«User not registered in the Developer Dashboard»</dt>
        <dd>Ты не добавил себя в User Management (шаг 3), или указал другую почту, не ту, что у аккаунта Spotify.</dd>
        <dt>«Spotify временно ограничил запросы»</dt>
        <dd>Даже своё приложение иногда упирается в лимит, например при импорте большой библиотеки. Подожди указанное время и повтори: oto продолжит с того же места.</dd>
        <dt>Хочу вернуть общий ключ</dt>
        <dd>Очисти поле на шаге 4 и нажми «Сохранить», потом войди заново.</dd>
      </dl>
    </details>
  </div>
</div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 80; background: rgb(0 0 0 / 0.45); backdrop-filter: blur(4px); }
  .sheet {
    position: fixed; z-index: 81; left: 50%; top: 50%; translate: -50% -50%;
    width: min(620px, calc(100vw - 32px)); max-height: calc(100vh - 48px);
    display: flex; flex-direction: column;
    border-radius: 22px; background: #141318; color: var(--text);
    box-shadow: 0 0 0 1px var(--line), 0 40px 100px -30px rgb(0 0 0 / 0.8);
  }
  header { display: flex; gap: 16px; align-items: flex-start; padding: 22px 22px 10px 26px; }
  h2 { margin: 0 0 6px; font-family: var(--font-display); font-size: 22px; letter-spacing: -0.02em; }
  header p { margin: 0; font-size: 13px; line-height: 1.45; }
  .close { flex: none; width: 34px; height: 34px; border-radius: 50%; display: grid; place-items: center; color: var(--text-2); }
  .close:hover { background: var(--hover); color: var(--text); }
  .body { overflow-y: auto; padding: 6px 26px 24px; display: flex; flex-direction: column; gap: 18px; }
  .need { display: flex; flex-direction: column; gap: 4px; padding: 12px 14px; border-radius: 14px; background: var(--surface); font-size: 13px; line-height: 1.45; }
  .steps { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; }
  .steps li { display: grid; grid-template-columns: 30px 1fr; gap: 12px; padding: 12px 0; border-top: 1px solid color-mix(in srgb, var(--line) 60%, transparent); }
  .steps li:first-child { border-top: 0; }
  .n { width: 28px; height: 28px; border-radius: 50%; display: grid; place-items: center; font-weight: 700; font-size: 13px; background: var(--text); color: var(--bg); }
  .st { display: flex; flex-direction: column; gap: 8px; min-width: 0; font-size: 13px; line-height: 1.5; }
  h3 { margin: 3px 0 0; font-size: 15px; font-weight: 650; }
  .st p { margin: 0; color: var(--text-2); }
  .st p b, dd b { color: var(--text); }
  dl { margin: 0; display: grid; grid-template-columns: 130px 1fr; gap: 6px 12px; }
  dt { color: var(--text-3, var(--text-2)); font-size: 12.5px; padding-top: 1px; }
  dd { margin: 0; color: var(--text-2); }
  code { font-family: ui-monospace, "Cascadia Code", Consolas, monospace; font-size: 12px; padding: 1px 6px; border-radius: 6px; background: var(--surface-2); color: var(--text); }
  code.big { font-size: 12.5px; padding: 4px 8px; }
  .copyrow { display: flex; align-items: center; gap: 6px; margin-bottom: 4px; flex-wrap: wrap; }
  .icon-btn.mini { width: 28px; height: 28px; }
  .idrow { display: flex; gap: 8px; }
  .idin { flex: 1; min-width: 0; height: 32px; padding: 0 10px; border-radius: 9px; border: 1px solid var(--line); background: var(--surface-2); color: var(--text); font-size: 12.5px; }
  .idin:focus { outline: none; border-color: var(--accent); }
  .mini-btn { align-self: flex-start; flex: none; height: 30px; padding: 0 14px; border-radius: 999px; font-size: 12.5px; font-weight: 600; background: var(--text); color: var(--bg); white-space: nowrap; }
  .ok { color: var(--accent); font-size: 12.5px; }
  .err { color: #ff7a7a !important; font-size: 12.5px; }
  .faq { border-radius: 14px; background: var(--surface); padding: 12px 14px; font-size: 13px; }
  .faq summary { cursor: pointer; font-weight: 600; }
  .faq dl { margin-top: 12px; grid-template-columns: 1fr; gap: 2px 0; }
  .faq dt { color: var(--text); font-weight: 600; margin-top: 8px; }
  .faq dd { line-height: 1.45; }
  :global(:root[data-theme="mist"]) .sheet { background: #fff7fa; }
  :global(:root[data-theme="paper"]) .sheet { background: #f7f4ee; border-radius: 0; }
  @media (max-width: 560px) { dl { grid-template-columns: 1fr; } }
</style>
