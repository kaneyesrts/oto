<script lang="ts">
  import { SOURCE_LABEL } from "../api";
  let { source, small = false }: { source: string; small?: boolean } = $props();
</script>

<i class="sb s-{source}" class:small title={SOURCE_LABEL[source] ?? source}>{small ? (SOURCE_LABEL[source] ?? source).slice(0, 2) : (SOURCE_LABEL[source] ?? source)}</i>

<style>
  .sb { display: inline-flex; align-items: center; font-style: normal; font-size: 9.5px; font-weight: 700; letter-spacing: 0.04em; text-transform: uppercase; padding: 2px 6px; border-radius: 5px; background: var(--surface-2); color: var(--text-2); white-space: nowrap; line-height: 1.2; }
  .small { padding: 1px 4px; }
  .s-yandex { background: rgb(255 204 0 / 0.18); color: #ffcc33; }
  .s-soundcloud { background: rgb(255 85 0 / 0.18); color: #ff7a33; }
  .s-spotify { background: rgb(30 215 96 / 0.18); color: #3ee07a; }
  :root[data-theme="mist"] .sb, :root[data-theme="paper"] .sb { color: var(--text); }
</style>
