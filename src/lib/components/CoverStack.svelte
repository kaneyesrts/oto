<script lang="ts">
  // A fan of covers: shows what's inside a mix at a glance instead of an empty tile.
  import { coverUrl } from "../api";
  let { covers, size = 120, spin = false }: { covers: (string | null | undefined)[]; size?: number; spin?: boolean } = $props();
  const list = $derived(covers.map((c) => coverUrl(c)).filter(Boolean).slice(0, 5) as string[]);
  const n = $derived(list.length);
</script>

<div class="stack" class:spin style="--s:{size}px">
  {#each list as c, i (c + i)}
    {@const mid = (n - 1) / 2}
    {@const d = i - mid}
    <img
      src={c}
      alt=""
      loading="lazy"
      style="--x:{d * size * 0.34}px; --r:{d * 7}deg; --y:{Math.abs(d) * size * 0.05}px; --z:{10 - Math.abs(Math.round(d))}; --i:{i}"
    />
  {/each}
</div>

<style>
  .stack { position: relative; width: calc(var(--s) * 2.1); height: calc(var(--s) * 1.15); }
  img {
    position: absolute; left: 50%; top: 50%;
    width: var(--s); height: var(--s); margin: calc(var(--s) / -2) 0 0 calc(var(--s) / -2);
    border-radius: 12px; object-fit: cover;
    z-index: var(--z);
    transform: translate(var(--x), var(--y)) rotate(var(--r));
    box-shadow: 0 18px 34px -16px rgb(0 0 0 / 0.75), 0 0 0 1px rgb(255 255 255 / 0.06);
    transition: transform 0.5s var(--ease-spring);
  }
  :global(.hoverable:hover) .stack img { transform: translate(calc(var(--x) * 1.18), calc(var(--y) - 6px)) rotate(calc(var(--r) * 1.3)); }
  .spin img { animation: float 6s ease-in-out infinite; animation-delay: calc(var(--i) * -1.2s); }
  @keyframes float { 50% { translate: 0 -5px; } }
  :root[data-theme="paper"] img { border-radius: 0; filter: grayscale(1); box-shadow: none; outline: 1px solid var(--text); }
</style>
