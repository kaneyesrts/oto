<script lang="ts">
  // The current lyric line with per-word karaoke fill, plus the next line underneath.
  // Shared by the home stage and the widget so they sing exactly like the lyrics view.
  import { app, clock } from "../store.svelte";
  import { activeIndex, lyricTime, wordsOf } from "../lyrics";

  let { size = 30, showNext = true, fallback = "" }: { size?: number; showNext?: boolean; fallback?: string } = $props();

  const doc = $derived(app.lyrics);
  const timed = $derived(doc?.kind === "synced" || doc?.kind === "estimated");
  const tm = $derived(lyricTime(clock.pos, doc));
  const idx = $derived(timed && doc ? activeIndex(doc.lines, tm) : -1);
  const line = $derived(idx >= 0 ? doc!.lines[idx] : null);
  const words = $derived(line ? wordsOf(line) : []);
  const next = $derived(timed && doc ? doc.lines.slice(idx + 1).find((l) => l.text.trim())?.text ?? "" : "");
  const prog = (t: number, d: number) => Math.min(1, Math.max(0, (tm - t) / Math.max(80, d)));
</script>

<div class="live" style="--s:{size}px">
  {#key idx}
    <div class="cur">
      {#if line && line.text.trim()}
        {#each words as w}{#if w.text.trim()}<span class="w" style="--p:{prog(w.t, w.d)}">{w.text}</span>{:else}{w.text}{/if}{/each}
      {:else if timed}
        <span class="dots"><i></i><i></i><i></i></span>
      {:else}
        <span class="fb">{fallback}</span>
      {/if}
    </div>
  {/key}
  {#if showNext && next}<div class="next">{next}</div>{/if}
</div>

<style>
  .live { min-width: 0; }
  .cur {
    font-weight: 780;
    font-size: var(--s);
    line-height: 1.18;
    letter-spacing: -0.02em;
    animation: rise 0.5s var(--ease-out);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .w {
    background: linear-gradient(90deg, currentColor calc(var(--p) * 100% - 6%), color-mix(in srgb, currentColor 34%, transparent) calc(var(--p) * 100% + 6%));
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
  }
  .next { margin-top: 6px; font-size: calc(var(--s) * 0.55); font-weight: 600; opacity: 0.42; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .fb { opacity: 0.5; }
  .dots { display: inline-flex; gap: 8px; vertical-align: middle; }
  .dots i { width: calc(var(--s) * 0.26); height: calc(var(--s) * 0.26); border-radius: 50%; background: currentColor; opacity: 0.45; animation: breathe 1.6s ease-in-out infinite; }
  .dots i:nth-child(2) { animation-delay: 0.2s; }
  .dots i:nth-child(3) { animation-delay: 0.4s; }
  @keyframes breathe { 50% { transform: scale(1.35); opacity: 1; } }
  @keyframes rise { from { opacity: 0; transform: translateY(10px); filter: blur(6px); } }
</style>
