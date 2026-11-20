<script lang="ts">
  import { app, player, trackById, openArtist } from "../store.svelte";
  import { api, coverUrl, fmtDuration, plural, type Stats } from "../api";
  import { glass } from "../glass";

  let range = $state("month");
  let s = $state<Stats | null>(null);
  $effect(() => { const r = range; api.stats(r).then((x) => { if (r === range) s = x; }); });

  const ranges: [string, string][] = [["week", "Неделя"], ["month", "Месяц"], ["year", "Год"], ["all", "Всё время"]];
  const liquid = $derived(app.settings?.theme === "liquid");
  const days = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"];

  const maxDay = $derived(Math.max(1, ...(s?.daily.map((d) => d.ms) ?? [1])));
  const maxHeat = $derived(Math.max(1, ...(s?.heat.flat() ?? [1])));
  const skipRate = $derived(s && s.plays + s.skips > 0 ? Math.round((s.skips / (s.plays + s.skips)) * 100) : 0);
  const peak = $derived.by(() => {
    if (!s) return null;
    let best = { d: 0, h: 0, v: 0 };
    s.heat.forEach((row, d) => row.forEach((v, h) => { if (v > best.v) best = { d, h, v }; }));
    return best.v ? best : null;
  });

  let tip = $state<{ x: number; y: number; text: string } | null>(null);
  function showTip(e: MouseEvent, text: string) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    tip = { x: r.left + r.width / 2, y: r.top, text };
  }
  const fmtDay = (d: string) => new Date(d + "T12:00").toLocaleDateString("ru-RU", { day: "numeric", month: "short", weekday: "short" });
  const hours = (ms: number) => (ms / 3600000 >= 10 ? Math.round(ms / 3600000) : (ms / 3600000).toFixed(1));
</script>

<div class="stats">
  <header>
    <div>
      <div class="eyebrow">Слушательская статистика</div>
      <h1>{s ? `${hours(s.rangeMs)} ч музыки` : "…"}</h1>
    </div>
    <div class="seg">
      {#each ranges as [k, l]}<button class:on={range === k} onclick={() => (range = k)}>{l}</button>{/each}
    </div>
  </header>

  {#if s}
    <section class="kpis">
      <div class="kpi panel" use:glass={{ radius: 24, enabled: liquid }}><b class="num">{s.plays}</b><span>{plural(s.plays, "прослушивание", "прослушивания", "прослушиваний")}</span></div>
      <div class="kpi panel" use:glass={{ radius: 24, enabled: liquid }}><b class="num">{s.uniqueTracks}</b><span>разных треков</span></div>
      <div class="kpi panel" use:glass={{ radius: 24, enabled: liquid }}><b class="num">{s.streakDays}</b><span>{plural(s.streakDays, "день", "дня", "дней")} подряд</span></div>
      <div class="kpi panel" use:glass={{ radius: 24, enabled: liquid }}><b class="num">{skipRate}%</b><span>треков пропущено</span></div>
    </section>

    <section class="two">
      <div class="box panel" use:glass={{ radius: 28, enabled: liquid }}>
        <div class="bh"><h3>По дням</h3><span class="faint">часы прослушивания</span></div>
        <div class="bars" role="img" aria-label="Время прослушивания по дням">
          {#each s.daily as d}
            <div class="bcol" role="presentation" onmouseenter={(e) => showTip(e, `${fmtDay(d.day)}: ${fmtDuration(d.ms)}`)} onmouseleave={() => (tip = null)}>
              <div class="b" style="height:{Math.max(d.ms ? 3 : 0, (d.ms / maxDay) * 100)}%"></div>
            </div>
          {/each}
        </div>
        <div class="axis faint num"><span>{fmtDay(s.daily[0]?.day ?? "")}</span><span>сегодня</span></div>
      </div>

      <div class="box panel" use:glass={{ radius: 28, enabled: liquid }}>
        <div class="bh"><h3>Когда вы слушаете</h3>{#if peak}<span class="faint">пик: {days[peak.d]}, {peak.h}:00</span>{/if}</div>
        <div class="heat">
          {#each s.heat as row, d}
            <span class="hl faint">{days[d]}</span>
            {#each row as v, h}
              <div class="cell" role="presentation" style="--v:{v / maxHeat}" onmouseenter={(e) => showTip(e, `${days[d]}, ${h}:00–${h + 1}:00 · ${fmtDuration(v)}`)} onmouseleave={() => (tip = null)}></div>
            {/each}
          {/each}
          <span></span>
          {#each Array(24) as _, h}<span class="hx faint num">{h % 6 === 0 ? h : ""}</span>{/each}
        </div>
      </div>
    </section>

    <section class="two">
      <div class="box panel" use:glass={{ radius: 28, enabled: liquid }}>
        <div class="bh"><h3>Артисты</h3></div>
        {#if s.topArtists.length}
          <ol class="artists">
            {#each s.topArtists.slice(0, 5) as a, i}
              <li>
                <span class="rk">{i + 1}</span>
                {#if coverUrl(a.cover)}<img class="cover" src={coverUrl(a.cover)} alt="" />{:else}<span class="cover"></span>{/if}
                <button class="an" onclick={() => openArtist(a.artist)}>
                  <span class="ellipsis">{a.artist}</span>
                  <span class="faint">{fmtDuration(a.ms)} · {a.plays} {plural(a.plays, "раз", "раза", "раз")}</span>
                </button>
              </li>
            {/each}
          </ol>
        {:else}<p class="muted">Пока пусто. Статистика копится с каждым прослушиванием дольше 30 секунд.</p>{/if}
        {#if s.rising}
          <div class="rising"><span class="eyebrow">Открытие недели</span><b>{s.rising.artist}</b><span class="faint">{fmtDuration(s.rising.ms)} за 7 дней</span></div>
        {/if}
      </div>

      <div class="box panel" use:glass={{ radius: 28, enabled: liquid }}>
        <div class="bh"><h3>Треки</h3></div>
        {#if s.topTracks.length}
          <div class="tracks">
            {#each s.topTracks as tt, i}
              {@const tr = trackById(tt.track.id) ?? tt.track}
              <button class="tr" onclick={() => player.playList(s!.topTracks.map((x) => x.track.id), i)}>
                <span class="num faint">{String(i + 1).padStart(2, "0")}</span>
                {#if coverUrl(tr.cover)}<img class="cover" src={coverUrl(tr.cover)} alt="" />{:else}<span class="cover"></span>{/if}
                <span class="tx"><span class="ellipsis">{tr.title}</span><span class="ellipsis faint">{tr.artist}</span></span>
                <span class="num muted">{tt.plays}×</span>
              </button>
            {/each}
          </div>
        {:else}<p class="muted">Здесь появятся самые частые треки.</p>{/if}
      </div>
    </section>
  {/if}
</div>

{#if tip}<div class="tip num" style="left:{tip.x}px;top:{tip.y}px">{tip.text}</div>{/if}

<style>
  .stats { height: 100%; overflow-y: auto; padding: 28px 32px 40px; display: flex; flex-direction: column; gap: 18px; }
  header { display: flex; align-items: flex-end; justify-content: space-between; gap: 20px; margin-bottom: 6px; }
  h1 { margin: 6px 0 0; font-family: var(--font-display); font-size: clamp(28px, 3vw, 44px); font-weight: 700; letter-spacing: -0.03em; }
  :root[data-theme="noir"] h1 { font-weight: 500; }
  :root[data-theme="paper"] h1 { font-style: italic; font-size: clamp(40px, 5vw, 72px); }
  h3 { margin: 0; font-family: var(--font-display); font-size: 16px; font-weight: 650; }
  .seg { display: flex; padding: 3px; border-radius: 999px; background: var(--surface); }
  .seg button { height: 30px; padding: 0 14px; border-radius: 999px; font-size: 12.5px; color: var(--text-2); }
  .seg button.on { background: var(--surface-2); color: var(--text); }
  :root[data-theme="paper"] .seg { border-radius: 0; background: none; border: 1px solid var(--line); padding: 0; }
  :root[data-theme="paper"] .seg button { border-radius: 0; }
  :root[data-theme="paper"] .seg button.on { background: var(--text); color: var(--bg); }

  .panel { background: var(--surface); border-radius: var(--radius-xl); }
  :root[data-theme="noir"] .panel { border-radius: 18px; background: var(--surface); }
  :root[data-theme="paper"] .panel { border-top: 1px solid var(--line); background: none; padding-left: 0 !important; padding-right: 0 !important; }
  .kpis { display: grid; grid-template-columns: repeat(4, 1fr); gap: 14px; }
  .kpi { padding: 18px 20px; display: flex; flex-direction: column; gap: 2px; }
  .kpi b { font-family: var(--font-display); font-size: 30px; font-weight: 650; letter-spacing: -0.02em; }
  .kpi span { color: var(--text-2); font-size: 12.5px; }
  .two { display: grid; grid-template-columns: 1fr 1fr; gap: 14px; }
  .box { padding: 20px 22px; display: flex; flex-direction: column; gap: 14px; min-width: 0; }
  .bh { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; font-size: 12px; }

  .bars { height: 150px; display: flex; align-items: flex-end; gap: 2px; border-bottom: 1px solid var(--line); }
  .bcol { flex: 1; height: 100%; display: flex; align-items: flex-end; cursor: default; }
  .b { width: 100%; background: var(--accent); border-radius: 4px 4px 0 0; opacity: 0.85; transition: opacity 0.15s; }
  .bcol:hover .b { opacity: 1; }
  :root[data-theme="paper"] .b { border-radius: 0; background: var(--text); }
  .axis { display: flex; justify-content: space-between; font-size: 11px; }

  .heat { display: grid; grid-template-columns: 22px repeat(24, 1fr); gap: 2px; align-items: center; }
  .cell { aspect-ratio: 1; border-radius: 3px; background: color-mix(in srgb, var(--accent) calc(var(--v) * 100%), var(--surface-2)); min-height: 8px; }
  .cell:hover { outline: 2px solid var(--text); outline-offset: -1px; }
  :root[data-theme="paper"] .cell { border-radius: 0; background: color-mix(in srgb, var(--text) calc(var(--v) * 100%), var(--surface-2)); }
  .hl, .hx { font-size: 10px; }

  .artists { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .artists li { display: grid; grid-template-columns: 44px 48px 1fr; align-items: center; gap: 12px; }
  .rk { font-family: var(--font-display); font-size: 40px; font-weight: 800; line-height: 1; color: transparent; -webkit-text-stroke: 1px color-mix(in srgb, var(--text) 35%, transparent); text-align: center; }
  .artists .cover { width: 48px; height: 48px; border-radius: 50%; }
  .an { display: flex; flex-direction: column; text-align: left; min-width: 0; font-weight: 600; }
  .an span:last-child { font-size: 12px; font-weight: 400; }
  .rising { display: flex; flex-direction: column; gap: 2px; padding: 14px 16px; border-radius: 16px; background: var(--accent-soft); }
  .rising b { font-size: 17px; }
  .tracks { display: flex; flex-direction: column; gap: 2px; }
  .tr { display: grid; grid-template-columns: 24px 40px 1fr auto; gap: 12px; align-items: center; padding: 6px 8px; border-radius: 10px; text-align: left; font-size: 13px; }
  .tr:hover { background: var(--hover); }
  .tr .cover { width: 40px; height: 40px; border-radius: 7px; }
  .tx { display: flex; flex-direction: column; min-width: 0; font-weight: 550; }
  .tx span:last-child { font-size: 12px; font-weight: 400; }
  .tip { position: fixed; z-index: 100; transform: translate(-50%, calc(-100% - 8px)); padding: 5px 9px; border-radius: 8px; background: var(--surface-2); color: var(--text); font-size: 12px; pointer-events: none; white-space: nowrap; box-shadow: 0 8px 20px -8px rgb(0 0 0 / 0.4); }
</style>
