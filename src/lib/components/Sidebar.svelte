<script lang="ts">
  import { app, libraryTracks, type View } from "../store.svelte";
  import { House, Library, Disc3, Users, Heart, Chart, Settings } from "../icons";
  import { plural, coverUrl, api } from "../api";
  import { Plus } from "../icons";
  import { albums } from "../groups";

  const items: { id: View; label: string; icon: any }[] = [
    { id: "home", label: "Главная", icon: House },
    { id: "library", label: "Треки", icon: Library },
    { id: "albums", label: "Альбомы", icon: Disc3 },
    { id: "artists", label: "Артисты", icon: Users },
    { id: "liked", label: "Избранное", icon: Heart },
    { id: "stats", label: "Статистика", icon: Chart },
  ];

  const liked = $derived(libraryTracks().filter((t) => t.liked).length);
  // Recently added albums: real navigation, and the covers bring colour into the rail.
  const recent = $derived(albums(libraryTracks()).sort((a, b) => b.addedAt - a.addedAt).slice(0, 5));

  async function newPlaylist() {
    const n = app.playlists.length + 1;
    const id = await api.playlistCreate(`Плейлист ${n}`);
    app.view = "playlist";
    app.focus = { kind: "playlist", key: String(id) };
  }

  function go(v: View) {
    app.view = v;
    app.focus = null;
    if (v !== "library") app.search = "";
  }
</script>

<nav class="side panel">
  <div class="group">
    {#each items as it}
      <button class="item" class:active={app.view === it.id} onclick={() => go(it.id)}>
        <it.icon size={17} strokeWidth={1.8} />
        <span>{it.label}</span>
        {#if it.id === "liked" && liked}<em class="num">{liked}</em>{/if}
      </button>
    {/each}
  </div>

  <div class="group">
    <div class="eyebrow row-h">Плейлисты <button class="add" onclick={newPlaylist} aria-label="Новый плейлист" title="Новый плейлист"><Plus size={14} /></button></div>
    {#each app.playlists as p (p.id)}
      <button class="item alb" class:active={app.view === "playlist" && app.focus?.key === String(p.id)} onclick={() => { app.view = "playlist"; app.focus = { kind: "playlist", key: String(p.id) }; }}>
        {#if p.covers[0]}<img class="mini" src={coverUrl(p.covers[0])} alt="" />{:else}<span class="mini ph"></span>{/if}
        <span class="at"><span class="ellipsis">{p.name}</span><span class="ellipsis faint">{p.count} {plural(p.count, "трек", "трека", "треков")}</span></span>
      </button>
    {:else}
      <button class="item small-hint faint" onclick={newPlaylist}>Создать первый плейлист</button>
    {/each}
  </div>

  {#if recent.length}
    <div class="group">
      <div class="eyebrow">Недавно добавлено</div>
      {#each recent as al (al.key)}
        <button class="item alb" class:active={app.focus?.key === al.key} onclick={() => { app.view = "albums"; app.focus = { kind: "album", key: al.key }; }}>
          {#if coverUrl(al.cover)}<img class="mini" src={coverUrl(al.cover)} alt="" />{:else}<span class="mini cover"></span>{/if}
          <span class="at"><span class="ellipsis">{al.title}</span><span class="ellipsis faint">{al.artist}</span></span>
        </button>
      {/each}
    </div>
  {/if}

  <div class="foot">
    {#if app.scan}
      <div class="scan">
        <div class="eyebrow">Сканирование</div>
        <div class="bar"><i style="width:{app.scan.total ? (app.scan.done / app.scan.total) * 100 : 0}%"></i></div>
        <div class="faint num">{app.scan.done} / {app.scan.total}</div>
      </div>
    {:else}
      <div class="faint count">{libraryTracks().length} {plural(libraryTracks().length, "трек", "трека", "треков")}</div>
    {/if}
    <button class="item" class:active={app.view === "settings"} onclick={() => go("settings")}>
      <Settings size={17} strokeWidth={1.8} />
      <span>Настройки</span>
    </button>
  </div>
</nav>

<style>
  .side {
    grid-area: side;
    display: flex;
    flex-direction: column;
    gap: 28px;
    padding: 18px 12px 14px;
    overflow: hidden auto;
    min-height: 0;
  }
  :root[data-theme="noir"] .side { border-right: 1px solid var(--line); background: var(--bg); }
  :root[data-theme="paper"] .side { border-right: 1px solid var(--line); padding-top: 26px; }
  .group { display: flex; flex-direction: column; gap: 2px; }
  .group .eyebrow { padding: 0 12px 8px; }
  .item {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    height: 38px;
    padding: 0 12px;
    border-radius: 10px;
    color: var(--text-2);
    font-weight: 500;
    text-align: left;
    transition: color 0.15s, background 0.15s;
  }
  .item span { flex: 1; min-width: 0; }
  .item em { font-style: normal; font-size: 11px; color: var(--text-3); }
  .item:hover { color: var(--text); background: var(--hover); }
  .item.active { color: var(--text); }
  :root[data-theme="noir"] .item.active { color: var(--accent); background: linear-gradient(90deg, var(--accent-soft), transparent 80%); }
  :root[data-theme="noir"] .item.active::before {
    content: ""; position: absolute; left: -12px; top: 8px; bottom: 8px; width: 3px; border-radius: 0 3px 3px 0; background: var(--accent);
  }
  :root[data-theme="mist"] .item.active { background: rgb(255 255 255 / 0.6); box-shadow: 0 6px 20px -10px rgb(160 60 100 / 0.4); }
  :root[data-theme="liquid"] .item.active { background: rgb(255 255 255 / 0.12); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.18); }
  :root[data-theme="paper"] .item { border-radius: 0; font-size: 15px; height: 36px; }
  :root[data-theme="paper"] .item :global(svg) { display: none; }
  :root[data-theme="paper"] .item.active { color: var(--accent); font-style: italic; font-family: var(--font-display); font-size: 18px; }
  .row-h { display: flex; align-items: center; justify-content: space-between; padding-right: 6px !important; }
  .add { width: 22px; height: 22px; border-radius: 6px; display: grid; place-items: center; color: var(--text-3); }
  .add:hover { background: var(--hover); color: var(--text); }
  .mini.ph { background: linear-gradient(135deg, rgb(var(--accent-rgb, var(--accent-default)) / 0.55), var(--surface-2)); }
  .small-hint { font-size: 12.5px; height: 32px; }
  .item.alb { height: 46px; gap: 10px; padding: 0 8px; }
  .mini { width: 30px; height: 30px; border-radius: 7px; object-fit: cover; flex: none; }
  .at { display: flex; flex-direction: column; min-width: 0; font-size: 12.5px; line-height: 1.25; }
  .at span:last-child { font-size: 11px; font-weight: 400; }
  :root[data-theme="paper"] .mini { display: none; }
  :root[data-theme="aura"] .item.active { background: rgb(255 255 255 / 0.1); box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.08); }
  :root[data-theme="aura"] .item:hover { background: rgb(255 255 255 / 0.06); }
  .foot { margin-top: auto; display: flex; flex-direction: column; gap: 8px; }
  .count { padding: 0 12px; font-size: 12px; }
  .scan { padding: 0 12px; display: grid; gap: 6px; font-size: 11px; }
  .bar { height: 3px; border-radius: 3px; background: var(--line); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--accent); transition: width 0.3s; }
</style>
