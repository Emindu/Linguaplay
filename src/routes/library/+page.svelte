<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { player, extractYoutubeVideoId } from "$lib/player.svelte";
  import { loadRecents, removeRecent, restoreRecent, baseName, type RecentItem } from "$lib/recents";
  import AppHeader from "$lib/AppHeader.svelte";
  import { formatBytes } from "$lib/format";

  let items = $state<RecentItem[]>([]);
  let wordsCount = $state(0);

  function refresh() {
    items = loadRecents();
  }

  function formatRelative(ts: number): string {
    const diffMs = Date.now() - ts;
    const minute = 60_000;
    const hour = 60 * minute;
    const day = 24 * hour;
    if (diffMs < minute) return "Just now";
    if (diffMs < hour) {
      const m = Math.floor(diffMs / minute);
      return m === 1 ? "A minute ago" : `${m} minutes ago`;
    }
    if (diffMs < day) {
      const h = Math.floor(diffMs / hour);
      return h === 1 ? "An hour ago" : `${h} hours ago`;
    }
    const days = Math.floor(diffMs / day);
    if (days === 1) return "Yesterday";
    if (days < 7) return `${days} days ago`;
    if (days < 14) return "Last week";
    return new Date(ts).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
  }

  function progressPercent(item: RecentItem): number {
    return item.durationMs > 0 ? Math.min(100, (item.positionMs / item.durationMs) * 100) : 0;
  }

  function progressLabel(item: RecentItem): string {
    const pct = progressPercent(item);
    if (pct >= 98) return "Finished";
    if (item.positionMs < 5000) return "Not started";
    return `${Math.max(1, Math.round(item.positionMs / 60_000))} min in`;
  }

  function formatDuration(ms: number): string {
    const total = Math.floor(ms / 1000);
    const h = Math.floor(total / 3600);
    const m = Math.floor((total % 3600) / 60);
    const sec = String(total % 60).padStart(2, "0");
    return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
  }

  /** YouTube serves a stable thumbnail per video ID with no API call needed;
   * local files have no equivalent without decoding a frame server-side, so
   * those still fall back to a generic folder icon. */
  function youtubeThumbUrl(item: RecentItem): string | null {
    if (item.kind !== "youtube") return null;
    const id = extractYoutubeVideoId(item.path);
    return id ? `https://i.ytimg.com/vi/${id}/mqdefault.jpg` : null;
  }

  function openItem(item: RecentItem) {
    // Fire-and-forget: the player is a persistent singleton, so the load
    // keeps running after we navigate away from this page.
    player.resumeRecent(item);
    goto("/");
  }

  let undo = $state<{ item: RecentItem; index: number } | null>(null);
  let undoTimer: ReturnType<typeof setTimeout> | undefined;

  function deleteItem(event: MouseEvent, item: RecentItem) {
    event.stopPropagation();
    event.preventDefault();
    const index = items.findIndex((r) => r.id === item.id);
    removeRecent(item.id);
    refresh();
    undo = { item, index: Math.max(0, index) };
    clearTimeout(undoTimer);
    undoTimer = setTimeout(() => (undo = null), 8000);
  }

  function undoDelete() {
    if (!undo) return;
    restoreRecent(undo.item, undo.index);
    undo = null;
    clearTimeout(undoTimer);
    refresh();
  }

  // ---- Downloaded YouTube videos (the on-disk cache) ----

  type DownloadedVideo = {
    video_id: string;
    title: string;
    path: string;
    size_bytes: number;
    downloaded_at_ms: number;
    quality_height: number | null;
  };
  type DownloadsSummary = { videos: DownloadedVideo[]; total_bytes: number; leftover_bytes: number };

  // Mirrors DOWNLOAD_CACHE_CAP_BYTES in src-tauri/src/youtube.rs.
  const DOWNLOAD_CAP_BYTES = 3 * 1024 * 1024 * 1024;

  let downloads = $state<DownloadsSummary>({ videos: [], total_bytes: 0, leftover_bytes: 0 });
  let confirmingClear = $state(false);
  let downloadsError = $state("");
  let busyId = $state<string | null>(null);

  async function refreshDownloads() {
    try {
      downloads = await invoke<DownloadsSummary>("list_youtube_downloads");
    } catch (err) {
      downloadsError = String(err);
    }
  }

  /** A downloaded video plays as a local file, so if it's the one on screen
   * the player has to let go of it before the file can be deleted. */
  function releaseIfLoaded(paths: string[]) {
    if (player.currentFile && paths.includes(player.currentFile)) player.closeVideo();
  }

  async function deleteDownload(video: DownloadedVideo) {
    downloadsError = "";
    busyId = video.video_id;
    releaseIfLoaded([video.path]);
    try {
      await invoke("delete_youtube_download", { videoId: video.video_id });
    } catch (err) {
      downloadsError = String(err);
    } finally {
      busyId = null;
      await refreshDownloads();
    }
  }

  async function clearDownloads() {
    downloadsError = "";
    confirmingClear = false;
    busyId = "all";
    releaseIfLoaded(downloads.videos.map((v) => v.path));
    try {
      await invoke("clear_youtube_downloads");
    } catch (err) {
      downloadsError = String(err);
    } finally {
      busyId = null;
      await refreshDownloads();
    }
  }

  onMount(() => {
    refresh();
    refreshDownloads();
    invoke<unknown[]>("list_vocabulary")
      .then((rows) => (wordsCount = rows.length))
      .catch(() => {});
  });
</script>

{#snippet iconFolder()}
  <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M4 6.5a1 1 0 0 1 1-1h4.2l1.6 2h8.2a1 1 0 0 1 1 1V17a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6.5z" />
  </svg>
{/snippet}

{#snippet iconPlay()}
  <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor" aria-hidden="true"><path d="M8 5v14l11-7z" /></svg>
{/snippet}

{#snippet iconTrash()}
  <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13" />
  </svg>
{/snippet}

{#snippet iconClock()}
  <svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <circle cx="12" cy="12" r="9" />
    <path d="M12 7v5l3.5 2" />
  </svg>
{/snippet}

<main class="page">
  <AppHeader wordsCount={wordsCount}>
    {#snippet menuExtras()}
      <div class="menu-divider"></div>
      <a class="menu-item" role="menuitem" href="/">Back to the player</a>
    {/snippet}
  </AppHeader>

  <div class="library-body">
    <div class="section-head">
      <h1>Library</h1>
      <span class="section-meta">{items.length} {items.length === 1 ? "video" : "videos"}</span>
    </div>

    {#if items.length === 0}
      <div class="empty-state">
        <div class="empty-mark">{@render iconClock()}</div>
        <h3>No videos yet</h3>
        <p>Videos and YouTube links you open will wait for you here, so you can keep watching where you left off.</p>
        <a class="nav-pill" href="/">Pick a video to begin</a>
      </div>
    {:else}
      <div class="recents-row">
        {#each items as item (item.id)}
          <div class="lib-card">
            <button class="lib-main" type="button" onclick={() => openItem(item)}>
              <span class="recent-thumb">
                {#if youtubeThumbUrl(item)}
                  <img src={youtubeThumbUrl(item)} alt="" loading="lazy" onerror={(e) => e.currentTarget.remove()} />
                {/if}
                {#if item.kind === "youtube"}{@render iconPlay()}{:else}{@render iconFolder()}{/if}
                {#if item.durationMs > 0}<span class="dur">{formatDuration(item.durationMs)}</span>{/if}
              </span>
              <span class="recent-body">
                <span class="recent-title">{item.title || baseName(item.path)}</span>
                <span class="recent-meta">{formatRelative(item.lastOpenedAt)} · {item.kind === "youtube" ? "YouTube" : "On this computer"}</span>
                <span class="recent-progress">
                  <span class="bar"><span style="width: {progressPercent(item)}%"></span></span>
                  <span class="label">{progressLabel(item)}</span>
                </span>
              </span>
            </button>
            <button class="icon-btn thumb-remove" onclick={(e) => deleteItem(e, item)} aria-label="Remove from library" title="Remove from library">
              {@render iconTrash()}
            </button>
          </div>
        {/each}
      </div>
    {/if}

    <section class="downloads" aria-labelledby="downloads-title">
      <div class="section-head">
        <h2 id="downloads-title">Downloaded videos</h2>
        <span class="section-meta">
          {formatBytes(downloads.total_bytes)} used · {downloads.videos.length}
          {downloads.videos.length === 1 ? "video" : "videos"}
        </span>
      </div>

      <p class="downloads-note">
        YouTube videos that can't be streamed are saved on this computer so they play in full. Deleting one frees
        the space; it is downloaded again the next time you open it, and your Library entry and progress stay.
        Oldest downloads are removed automatically past {formatBytes(DOWNLOAD_CAP_BYTES)}.
      </p>

      <div class="usage" role="img" aria-label="{formatBytes(downloads.total_bytes)} of {formatBytes(DOWNLOAD_CAP_BYTES)} limit used">
        <span style="width: {Math.min(100, (downloads.total_bytes / DOWNLOAD_CAP_BYTES) * 100)}%"></span>
      </div>

      {#if downloadsError}
        <p class="downloads-error" role="alert">{downloadsError}</p>
      {/if}

      {#if downloads.videos.length === 0 && downloads.total_bytes === 0}
        <p class="downloads-empty">No downloaded videos.</p>
      {:else}
        <ul class="download-list">
          {#each downloads.videos as video (video.video_id)}
            <li class="download-row">
              <div class="download-info">
                <span class="download-title">{video.title}</span>
                <span class="download-meta">
                  {video.quality_height ? `${video.quality_height}p · ` : ""}{formatBytes(video.size_bytes)} · {formatRelative(video.downloaded_at_ms)}
                </span>
              </div>
              <button
                class="btn download-delete"
                type="button"
                disabled={busyId !== null}
                onclick={() => deleteDownload(video)}
                aria-label="Delete download: {video.title}"
              >
                {@render iconTrash()} {busyId === video.video_id ? "Deleting…" : "Delete"}
              </button>
            </li>
          {/each}
        </ul>

        {#if downloads.leftover_bytes > 0}
          <p class="downloads-note">
            Includes {formatBytes(downloads.leftover_bytes)} of leftovers from interrupted downloads; “Delete all” removes those too.
          </p>
        {/if}

        <div class="downloads-actions">
          {#if confirmingClear}
            <span class="confirm-text">Delete all {formatBytes(downloads.total_bytes)} of downloads?</span>
            <button class="btn" type="button" onclick={() => (confirmingClear = false)}>Cancel</button>
            <button class="btn btn-danger" type="button" onclick={clearDownloads}>Yes, delete all</button>
          {:else}
            <button class="btn" type="button" disabled={busyId !== null} onclick={() => (confirmingClear = true)}>
              {busyId === "all" ? "Deleting…" : "Delete all downloads"}
            </button>
          {/if}
        </div>
      {/if}
    </section>
  </div>

  {#if undo}
    <div class="undo-notice" role="status">
      <span>Removed “{baseName(undo.item.title || undo.item.path)}” from your library.</span>
      <button class="link-btn" onclick={undoDelete}>Undo</button>
    </div>
  {/if}
</main>

<style>
  .page {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
  }

  .library-body {
    width: 100%;
    max-width: 880px;
    margin: 0 auto;
    padding: var(--space-8) var(--space-8) var(--space-10);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .section-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-4);
  }

  .section-head h1 {
    margin: 0;
    font: italic 500 34px/40px var(--font-serif);
    letter-spacing: -0.02em;
  }

  .section-meta {
    color: var(--color-text-muted);
    font: 400 14px/20px var(--font-ui);
  }

  .recents-row {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  /* One card per row: the card owns the border/background, the main button
     fills it (no double padding), and the remove button sits beside it in
     normal flow instead of floating over the title. */
  .lib-card {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding-right: var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    transition: border-color var(--dur-fast) var(--ease-out), box-shadow var(--dur-fast) var(--ease-out);
  }

  .lib-card:hover,
  .lib-card:focus-within {
    border-color: var(--color-border-strong);
    box-shadow: 0 1px 3px rgba(70, 45, 20, 0.06), 0 10px 24px rgba(70, 45, 20, 0.08);
  }

  .lib-main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: stretch;
    gap: var(--space-4);
    padding: var(--space-4);
    background: transparent;
    border: 0;
    border-radius: var(--radius-md);
    color: var(--color-text);
    text-align: left;
    font-family: inherit;
    cursor: pointer;
  }

  .lib-main :global(.recent-thumb) {
    width: 176px;
  }

  .lib-main :global(.recent-title) {
    font-size: 16px;
  }

  .thumb-remove {
    flex: none;
    width: 36px;
    height: 36px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    border: none;
    background: transparent;
    color: var(--color-text-muted);
    cursor: pointer;
    opacity: 0.7;
    transition: opacity var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out);
  }

  .lib-card:hover .thumb-remove,
  .thumb-remove:focus-visible {
    opacity: 1;
  }

  .thumb-remove:hover {
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
    color: var(--color-danger);
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text-muted);
    padding: var(--space-10) var(--space-6);
    text-align: center;
    background: var(--color-surface);
    border: 2px dashed var(--color-border-strong);
    border-radius: var(--radius-lg);
  }

  .empty-mark {
    width: 56px;
    height: 56px;
    display: grid;
    place-items: center;
    margin-bottom: var(--space-2);
    border-radius: 50%;
    background: var(--color-surface-hover);
    color: var(--color-text-muted);
  }

  .empty-state h3 {
    margin: 0;
    color: var(--color-text);
    font: italic 500 24px/30px var(--font-serif);
  }

  .empty-state p {
    margin: 0 0 var(--space-3);
    max-width: 38ch;
  }

  /* ---- downloaded videos (disk cache) ---- */

  .downloads {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin-top: var(--space-8);
    padding-top: var(--space-6);
    border-top: 1px solid var(--color-border);
  }

  .downloads h2 {
    margin: 0;
    font: italic 500 26px/32px var(--font-serif);
    letter-spacing: -0.01em;
  }

  .downloads-note {
    margin: 0;
    max-width: 68ch;
    color: var(--color-text-muted);
    font: 400 13px/20px var(--font-ui);
  }

  .downloads-empty {
    margin: 0;
    color: var(--color-text-muted);
    font: 400 14px/20px var(--font-ui);
  }

  .downloads-error {
    margin: 0;
    color: var(--color-danger);
    font: 500 13px/20px var(--font-ui);
  }

  .usage {
    height: 6px;
    border-radius: var(--radius-pill);
    background: var(--color-surface-hover);
    overflow: hidden;
  }

  .usage span {
    display: block;
    height: 100%;
    min-width: 2px;
    background: var(--color-accent);
  }

  .download-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
  }

  .download-row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-3) var(--space-4);
  }

  .download-row + .download-row {
    border-top: 1px solid var(--color-border);
  }

  .download-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .download-title {
    font: 600 14px/20px var(--font-ui);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .download-meta {
    color: var(--color-text-muted);
    font: 400 12px/16px var(--font-mono);
  }

  .download-delete {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-danger);
  }

  .downloads-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .confirm-text {
    margin-right: auto;
    font: 500 14px/20px var(--font-ui);
  }

  .btn-danger {
    background: var(--color-danger);
    border-color: var(--color-danger);
    color: #fff;
  }

  .undo-notice {
    position: fixed;
    left: 50%;
    bottom: var(--space-6);
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
    font-size: 0.9rem;
    max-width: min(32rem, calc(100vw - 2rem));
  }

  .undo-notice .link-btn {
    border: 0;
    background: none;
    padding: 0;
    font: 600 0.9rem var(--font-ui);
    color: var(--color-accent);
    cursor: pointer;
  }

  @media (max-width: 640px) {
    .library-body {
      padding: var(--space-6) var(--space-4) var(--space-10);
    }

    .lib-main {
      padding: var(--space-3);
      gap: var(--space-3);
    }

    .lib-main :global(.recent-thumb) {
      width: 112px;
    }
  }
</style>
