<script lang="ts">
  // Right column: what's playing, a live lyrics peek and what's next.
  import { app, clock, player, trackById } from "../store.svelte";
  import { coverUrl, fmtTime } from "../api";
  import { activeIndex, lyricTime } from "../lyrics";
  import { glass } from "../glass";
  import { MicVocal } from "../icons";

  const t = $derived(app.player.track);
  const doc = $derived(app.lyrics);
  const timed = $derived(doc?.kind === "synced" || doc?.kind === "estimated");
  const idx = $derived(timed && doc ? activeIndex(doc.lines, lyricTime(clock.pos, doc)) : -1);
  const peek = $derived.by(() => {
    if (!doc?.lines.length || !timed) return [];
    const from = Math.max(0, idx - 1);
    return doc.lines.slice(from, from + 4).map((l, k) => ({ text: l.text || "•••", on: from + k === idx }));
  });
  const upcoming = $derived(
    app.player.queue.slice(app.player.index + 1, app.player.index + 7).map((id, k) => ({ t: trackById(id), i: app.player.index + 1 + k }))
  );
  const liquid = $derived(app.settings?.theme === "liquid");
</script>

<aside class="aside">
  <div class="card panel" use:glass={{ radius: 28, enabled: liquid }}>
    {#if t}
      <button class="art" onclick={() => (app.lyricsOpen = true)}>
        {#if coverUrl(t.cover)}{#key t.cover}<img class="cover" src={coverUrl(t.cover)} alt="" />{/key}{:else}<div class="cover"></div>{/if}
      </button>
      <div class="ti">{t.title}</div>
      <div class="ar muted">{t.artist}</div>
      {#if t.album}<div class="al faint">{t.album}{t.year ? ` · ${t.year}` : ""}</div>{/if}
    {:else}
      <div class="art"><div class="cover"></div></div>
      <div class="ti faint">Тишина</div>
    {/if}
  </div>

  {#if peek.length}
    <button class="peek panel" onclick={() => (app.lyricsOpen = true)} use:glass={{ radius: 24, enabled: liquid }}>
      <div class="eyebrow"><MicVocal size={12} /> Текст</div>
      {#each peek as l}<p class:on={l.on}>{l.text}</p>{/each}
    </button>
  {/if}

  {#if upcoming.length}
    <div class="next panel" use:glass={{ radius: 24, enabled: liquid }}>
      <div class="eyebrow">Далее</div>
      {#each upcoming as u (u.i)}
        {#if u.t}
          <button class="q" ondblclick={() => player.jumpTo(u.i)} onclick={() => player.jumpTo(u.i)}>
            {#if coverUrl(u.t.cover)}<img class="cover" src={coverUrl(u.t.cover)} alt="" />{:else}<span class="cover"></span>{/if}
            <span class="qt"><span class="ellipsis">{u.t.title}</span><span class="ellipsis faint">{u.t.artist}</span></span>
            <span class="num faint">{fmtTime(u.t.durationMs)}</span>
          </button>
        {/if}
      {/each}
    </div>
  {/if}
</aside>

<style>
  .aside { grid-area: aside; display: flex; flex-direction: column; gap: var(--gap); min-height: 0; overflow: hidden auto; }
  :root[data-theme="noir"] .aside { border-left: 1px solid var(--line); background: var(--bg); padding: 20px 18px; gap: 26px; }
  :root[data-theme="paper"] .aside { display: none; }
  .card { padding: 18px; }
  :root[data-theme="noir"] .card, :root[data-theme="noir"] .peek, :root[data-theme="noir"] .next { padding: 0; background: none; }
  .art { display: block; width: 100%; }
  .art .cover { width: 100%; border-radius: 16px; box-shadow: 0 24px 50px -24px rgb(0 0 0 / 0.55); }
  .ti { margin-top: 16px; font-family: var(--font-display); font-size: 19px; font-weight: 650; line-height: 1.2; letter-spacing: -0.01em; }
  :root[data-theme="noir"] .ti { font-size: 17px; font-weight: 500; }
  .ar { margin-top: 4px; }
  .al { margin-top: 2px; font-size: 12px; }
  .peek { text-align: left; padding: 16px 18px; display: flex; flex-direction: column; gap: 6px; }
  .peek .eyebrow { display: flex; align-items: center; gap: 6px; margin-bottom: 4px; }
  .peek p { margin: 0; font-weight: 650; font-size: 15px; line-height: 1.3; color: var(--text-3); transition: color 0.3s; }
  .peek p.on { color: var(--text); }
  .next { padding: 16px 12px; display: flex; flex-direction: column; gap: 2px; }
  .next .eyebrow { padding: 0 6px 8px; }
  .q { display: grid; grid-template-columns: 36px 1fr auto; gap: 10px; align-items: center; padding: 6px; border-radius: 10px; text-align: left; font-size: 13px; }
  .q:hover { background: var(--hover); }
  .q .cover { width: 36px; height: 36px; border-radius: 6px; }
  .qt { display: flex; flex-direction: column; min-width: 0; }
  .qt span:last-child { font-size: 11.5px; }
  .q .num { font-size: 11px; }
</style>
