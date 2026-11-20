<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { app, libraryTracks, player, trackById } from "../store.svelte";
  import { api, coverUrl, fmtDuration, plural, type Stats } from "../api";
  import { albums } from "../groups";
  import { glass } from "../glass";
  import { Play, FolderPlus, Shuffle, Flame, Clock, TrendingUp } from "../icons";
  import NowStage from "../components/NowStage.svelte";
  import StationCard from "../components/StationCard.svelte";
  import CoverStack from "../components/CoverStack.svelte";

  let stats = $state<Stats | null>(null);
  onMount(async () => { stats = await api.stats("week"); });

  const now = new Date();
  const greet = (() => {
    const h = now.getHours();
    return h < 5 ? "Доброй ночи" : h < 12 ? "Доброе утро" : h < 18 ? "Добрый день" : "Добрый вечер";
  })();
  const dateLine = now.toLocaleDateString("ru-RU", { weekday: "long", day: "numeric", month: "long" });

  const recent = $derived(albums(libraryTracks()).sort((a, b) => b.addedAt - a.addedAt).slice(0, 10));
  const liquid = $derived(app.settings?.theme === "liquid");
  const resume = $derived(app.player.track);
  const upNext = $derived(
    app.player.queue.slice(app.player.index + 1, app.player.index + 7)
      .map((id, k) => ({ t: trackById(id)!, i: app.player.index + 1 + k }))
      .filter((u) => u.t)
  );

  // Mixes are built once per visit so the covers on the card are exactly what plays.
  function weightedMix(boost: Set<string>, extra = 0.8) {
    const pool = libraryTracks().map((t) => ({ t, w: Math.random() + (boost.has(t.id) ? extra : 0) }));
    pool.sort((a, b) => b.w - a.w);
    return pool.slice(0, 60).map((p) => p.t);
  }
  let dayMix = $state<import("../api").Track[]>([]);
  let randomMix = $state<import("../api").Track[]>([]);
  $effect(() => {
    if (!libraryTracks().length || dayMix.length) return;
    const fav = new Set([...libraryTracks().filter((t) => t.liked).map((t) => t.id), ...(stats?.topTracks.map((x) => x.track.id) ?? [])]);
    dayMix = weightedMix(fav);
    randomMix = weightedMix(new Set(), 0);
  });
  const coversOf = (l: import("../api").Track[]) => [...new Set(l.map((t) => t.cover).filter(Boolean))].slice(0, 5);

  async function addFolder() {
    const dir = await open({ directory: true, multiple: false, title: "Папка с музыкой" });
    if (typeof dir === "string") api.addFolder(dir);
  }
</script>

<div class="home">
  {#if resume}
    <NowStage />
  {:else}
    <header class="hello">
      <div class="eyebrow">{dateLine}</div>
      <h1>{greet}</h1>
    </header>
  {/if}

  {#if !libraryTracks().length}
    <section class="onboard panel" use:glass={{ radius: 30, enabled: liquid }}>
      <div>
        <h2>Начнём с вашей музыки</h2>
        <p class="muted">Укажите папку: oto прочитает теги, обложки и тексты (.lrc и встроенные), а недостающие тексты найдёт в LRCLIB. Файлы никуда не загружаются.</p>
      </div>
      <button class="pill solid" onclick={addFolder}><FolderPlus size={16} /> Добавить папку</button>
    </section>
  {:else}
    <StationCard />

    <section class="hero" class:compact={!!resume}>
      {#if upNext.length}
        <div class="upnext">
          <div class="sec-h"><h3>Далее</h3><button class="faint" onclick={() => (app.queueOpen = true)}>Вся очередь</button></div>
          <div class="un-row">
            {#each upNext as u (u.i)}
              <button class="un" onclick={() => player.jumpTo(u.i)}>
                {#if coverUrl(u.t.cover)}<img class="cover" src={coverUrl(u.t.cover)} alt="" loading="lazy" />{:else}<span class="cover"></span>{/if}
                <span class="ut"><span class="ellipsis">{u.t.title}</span><span class="ellipsis faint">{u.t.artist}</span></span>
              </button>
            {/each}
          </div>
        </div>
      {:else}
        <button class="card mixcard hoverable panel" onclick={() => player.playList(randomMix.map((t) => t.id), 0)} use:glass={{ radius: 30, enabled: liquid }}>
          {#if randomMix[0]?.cover}<div class="tint" style="background-image:url('{coverUrl(randomMix[0].cover)}')"></div>{/if}
          <div class="txt">
            <div class="eyebrow">Случайный микс</div>
            <div class="big">Всё вперемешку</div>
            <div class="muted">60 из {libraryTracks().length} {plural(libraryTracks().length, "трека", "треков", "треков")} библиотеки</div>
          </div>
          <div class="stackwrap"><CoverStack covers={coversOf(randomMix)} size={92} /></div>
        </button>
      {/if}
      <button class="card mixcard hoverable panel" onclick={() => player.playList(dayMix.map((t) => t.id), 0)} use:glass={{ radius: 30, enabled: liquid }}>
        {#if dayMix[0]?.cover}<div class="tint" style="background-image:url('{coverUrl(dayMix[0].cover)}')"></div>{/if}
        <div class="txt">
          <div class="eyebrow">Микс дня</div>
          <div class="big">Любимое и новое</div>
          <div class="muted">60 треков: избранное, частое и немного забытого</div>
        </div>
        <div class="stackwrap"><CoverStack covers={coversOf(dayMix)} size={92} /></div>
      </button>
    </section>

    {#if stats}
      <section class="strip">
        <div class="stat panel" use:glass={{ radius: 24, enabled: liquid }}>
          <Clock size={16} /><div><b class="num">{fmtDuration(stats.todayMs)}</b><span class="faint">сегодня</span></div>
        </div>
        <div class="stat panel" use:glass={{ radius: 24, enabled: liquid }}>
          <Flame size={16} /><div><b class="num">{stats.streakDays} {plural(stats.streakDays, "день", "дня", "дней")}</b><span class="faint">подряд с музыкой</span></div>
        </div>
        <div class="stat panel" use:glass={{ radius: 24, enabled: liquid }}>
          <TrendingUp size={16} /><div><b class="ellipsis">{stats.topArtists[0]?.artist ?? "—"}</b><span class="faint">артист недели</span></div>
        </div>
      </section>
    {/if}

    <section>
      <div class="sec-h"><h3>Недавно добавлено</h3><button class="faint" onclick={() => (app.view = "albums")}>Все альбомы</button></div>
      <div class="row-scroll">
        {#each recent as a (a.key)}
          <div class="alb">
            <button class="alb-art" onclick={() => { app.view = "albums"; app.focus = { kind: "album", key: a.key }; }}>
              {#if coverUrl(a.cover)}<img class="cover" src={coverUrl(a.cover)} alt="" loading="lazy" />{:else}<div class="cover"></div>{/if}
              <span class="fab" role="button" tabindex="-1" onclick={(e) => { e.stopPropagation(); player.playList(a.tracks.map((t) => t.id), 0); }} onkeydown={() => {}}><Play size={18} fill="currentColor" strokeWidth={0} /></span>
            </button>
            <div class="ellipsis an">{a.title}</div>
            <div class="ellipsis faint aa">{a.artist}</div>
          </div>
        {/each}
      </div>
    </section>

    {#if stats?.topTracks.length}
      <section>
        <div class="sec-h"><h3>На повторе этой недели</h3></div>
        <div class="top">
          {#each stats.topTracks.slice(0, 6) as tt, i}
            {@const tr = trackById(tt.track.id) ?? tt.track}
            <button class="tt" ondblclick={() => player.playList(stats!.topTracks.map((x) => x.track.id), i)} onclick={() => player.playList(stats!.topTracks.map((x) => x.track.id), i)}>
              <span class="rank num">{i + 1}</span>
              {#if coverUrl(tr.cover)}<img class="cover" src={coverUrl(tr.cover)} alt="" />{:else}<span class="cover"></span>{/if}
              <span class="ttx"><span class="ellipsis">{tr.title}</span><span class="ellipsis faint">{tr.artist}</span></span>
              <span class="faint num">{tt.plays}×</span>
            </button>
          {/each}
        </div>
      </section>
    {/if}
  {/if}
</div>

<style>
  .home { height: 100%; overflow-y: auto; padding: 28px clamp(24px, 3vw, 44px) 40px; display: flex; flex-direction: column; gap: 34px; }
  .hello h1 { margin: 6px 0 0; font-family: var(--font-display); font-size: clamp(30px, 3.4vw, 46px); font-weight: 700; letter-spacing: -0.03em; line-height: 1; }
  :root[data-theme="noir"] .hello h1 { font-weight: 500; letter-spacing: -0.01em; }
  :root[data-theme="paper"] .hello h1 { font-size: clamp(48px, 6vw, 92px); font-style: italic; font-weight: 800; border-bottom: 1px solid var(--line); padding-bottom: 18px; }
  .hello .eyebrow::first-letter { text-transform: uppercase; }

  .onboard { display: flex; align-items: center; justify-content: space-between; gap: 32px; padding: 32px; background: var(--surface); }
  .onboard h2 { margin: 0 0 8px; font-family: var(--font-display); font-size: 24px; }
  .onboard p { margin: 0; max-width: 560px; }

  .hero { display: grid; grid-template-columns: repeat(auto-fit, minmax(340px, 1fr)); gap: 18px; align-items: stretch; }
  .hero.compact .card { min-height: 0; }
  .mixcard { align-items: center; min-height: 168px; }
  .mixcard .txt { align-self: flex-end; }
  .tint { position: absolute; inset: -30%; background-size: cover; background-position: center; filter: blur(55px) saturate(1.5); opacity: 0.32; pointer-events: none; }
  :root[data-theme="paper"] .tint { display: none; }
  .stackwrap { position: relative; flex: none; margin-right: -6px; }
  .upnext { min-width: 0; display: flex; flex-direction: column; }
  .un-row { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 4px 14px; }
  .un { display: grid; grid-template-columns: 44px 1fr; gap: 12px; align-items: center; padding: 6px; border-radius: 12px; text-align: left; min-width: 0; transition: background 0.15s; }
  .un:hover { background: var(--hover); }
  .un .cover { width: 44px; height: 44px; border-radius: 8px; }
  .ut { display: flex; flex-direction: column; min-width: 0; font-weight: 600; font-size: 13.5px; }
  .ut span:last-child { font-weight: 400; font-size: 12px; }
  :root[data-theme="aura"] .card { background: rgb(255 255 255 / 0.06); backdrop-filter: blur(30px) saturate(1.3); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.08); }
  :root[data-theme="aura"] .stat { background: rgb(255 255 255 / 0.06); backdrop-filter: blur(30px); }
  .card {
    position: relative; overflow: hidden; min-height: 196px; padding: 24px; text-align: left;
    display: flex; align-items: flex-end; justify-content: space-between; gap: 18px;
    background: var(--surface); border-radius: var(--radius-xl);
    transition: transform 0.35s var(--ease-spring);
  }
  .card:hover { transform: translateY(-2px); }
  :root[data-theme="noir"] .card { border-radius: 18px; background: var(--surface); }
  :root[data-theme="paper"] .card { border: 1px solid var(--line); background: transparent; }
  .txt { position: relative; min-width: 0; }
  .big { font-family: var(--font-display); font-size: 26px; font-weight: 700; letter-spacing: -0.02em; margin: 8px 0 4px; line-height: 1.1; }
  :root[data-theme="paper"] .big { font-style: italic; font-size: 32px; }

  .strip { display: grid; grid-template-columns: repeat(3, 1fr); gap: 14px; }
  .stat { display: flex; align-items: center; gap: 14px; padding: 14px 18px; color: var(--accent); background: var(--surface); border-radius: var(--radius-lg); }
  :root[data-theme="paper"] .stat { border-top: 1px solid var(--line); background: none; padding-left: 0; }
  .stat div { display: flex; flex-direction: column; min-width: 0; color: var(--text); }
  .stat b { font-weight: 650; font-size: 15px; }
  .stat span { font-size: 12px; }

  .sec-h { display: flex; align-items: baseline; justify-content: space-between; margin-bottom: 14px; }
  h3 { margin: 0; font-family: var(--font-display); font-size: 18px; font-weight: 650; letter-spacing: -0.01em; }
  :root[data-theme="paper"] h3 { font-size: 24px; font-style: italic; }
  .sec-h button { font-size: 12px; }
  .sec-h button:hover { color: var(--text); }
  .row-scroll { display: grid; grid-auto-flow: column; grid-auto-columns: 168px; gap: 18px; overflow-x: auto; padding-bottom: 8px; }
  .alb-art { position: relative; display: block; width: 100%; }
  .alb-art .cover { width: 100%; border-radius: 14px; transition: transform 0.35s var(--ease-spring), box-shadow 0.3s; }
  .alb-art:hover .cover { transform: translateY(-3px); box-shadow: 0 18px 36px -18px rgb(0 0 0 / 0.6); }
  :root[data-theme="paper"] .alb-art .cover { border-radius: 0; filter: grayscale(1); }
  :root[data-theme="paper"] .alb-art:hover .cover { filter: none; }
  .fab {
    position: absolute; right: 10px; bottom: 10px; width: 42px; height: 42px; border-radius: 50%;
    display: grid; place-items: center; background: var(--accent); color: #fff;
    opacity: 0; transform: translateY(6px); transition: opacity 0.2s, transform 0.3s var(--ease-spring);
    box-shadow: 0 10px 20px -8px var(--accent);
  }
  .alb-art:hover .fab { opacity: 1; transform: none; }
  .an { margin-top: 10px; font-weight: 600; font-size: 13.5px; }
  .aa { font-size: 12px; }

  .top { display: grid; grid-template-columns: repeat(2, 1fr); gap: 4px 18px; }
  .tt { display: grid; grid-template-columns: 24px 44px 1fr auto; gap: 12px; align-items: center; padding: 8px; border-radius: 12px; text-align: left; }
  .tt:hover { background: var(--hover); }
  .rank { font-family: var(--font-display); font-size: 20px; color: var(--text-3); text-align: center; }
  .tt .cover { width: 44px; height: 44px; border-radius: 8px; }
  .ttx { display: flex; flex-direction: column; min-width: 0; font-weight: 550; }
  .ttx span:last-child { font-size: 12px; font-weight: 400; }
</style>
