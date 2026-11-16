<script lang="ts">
  import { onDestroy, onMount, untrack } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import {
    player,
    QUALITY_OPTIONS,
    ASPECT_OPTIONS,
    PLAYBACK_RATE_OPTIONS,
    DOWNLOAD_QUALITY_OPTIONS,
    LLM_CATALOG,
    trackLabel,
    type CatalogModel,
    type DownloadProgress,
    type YoutubeDownloadProgress,
    type WordPopup,
  } from "$lib/player.svelte";
  import { formatBytes, formatEta } from "$lib/format";
  import AppHeader from "$lib/AppHeader.svelte";
  import SubtitleStylePanel from "$lib/SubtitleStylePanel.svelte";
  import { subtitleStyleVars, positionClass } from "$lib/subtitleStyle";
  import { invoke } from "@tauri-apps/api/core";
  import { baseName, loadRecents, type RecentItem } from "$lib/recents";
  import type { SavedWord } from "$lib/player.svelte";

  let subtitleMenuOpen = $state(false);
  let qualityMenuOpen = $state(false);
  let aspectMenuOpen = $state(false);
  let fontMenuOpen = $state(false);
  let rateMenuOpen = $state(false);
  let aiSetupOpen = $state(false);
  let shortcutsHelpOpen = $state(false);
  let isDraggingFileOver = $state(false);
  let youtubeUrlInput = $state("");
  let wordsCount = $state(0);
  let recents = $state<RecentItem[]>([]);
  let railTab = $state<"transcript" | "saved">("transcript");
  let follow = $state(true);
  let railBodyEl: HTMLDivElement | undefined = $state();
  let lastVolume = 100;
  let isFullscreen = $state(false);
  let videoStageEl: HTMLDivElement;
  let canvasEl: HTMLCanvasElement | undefined = $state();
  let playerCardEl: HTMLDivElement;

  // Fullscreen is plain browser Fullscreen API on the player card (video
  // stage + control bar) rather than a Tauri window-level fullscreen: it
  // keeps the app's own title bar/OS chrome intact and needs no extra
  // plugin. The word popup only makes sense here — in windowed mode the
  // definition lives in the sidebar instead (see wordPanel below), since
  // there's no "outside the video" space in fullscreen for a sidebar to
  // occupy.
  function toggleFullscreen() {
    if (!player.currentFile) return;
    if (document.fullscreenElement) {
      document.exitFullscreen();
    } else {
      playerCardEl.requestFullscreen();
    }
  }

  function onFullscreenChange() {
    isFullscreen = document.fullscreenElement === playerCardEl;
  }

  // Auto-hide controls during playback — fades the (now overlaid, see
  // .control-bar CSS) control bar out after a few seconds of no mouse/
  // keyboard activity, like every mainstream video player, and brings it
  // back on movement, a keypress, or pausing. Never hides while a menu is
  // open or nothing is playing.
  let controlsVisible = $state(true);
  let hideControlsTimer: ReturnType<typeof setTimeout> | null = null;

  function menusOpen(): boolean {
    return (
      subtitleMenuOpen || qualityMenuOpen || aspectMenuOpen || fontMenuOpen || rateMenuOpen || aiSetupOpen || shortcutsHelpOpen
    );
  }

  function scheduleHideControls() {
    if (hideControlsTimer) clearTimeout(hideControlsTimer);
    hideControlsTimer = setTimeout(() => {
      if (player.currentFile && player.isPlaying && !menusOpen()) {
        controlsVisible = false;
      }
    }, 2500);
  }

  function wakeControls() {
    controlsVisible = true;
    scheduleHideControls();
  }

  function onPlayerCardMouseLeave() {
    if (hideControlsTimer) clearTimeout(hideControlsTimer);
    if (player.currentFile && player.isPlaying && !menusOpen()) {
      controlsVisible = false;
    }
  }

  $effect(() => {
    if (player.isPlaying) {
      scheduleHideControls();
    } else {
      controlsVisible = true;
      if (hideControlsTimer) clearTimeout(hideControlsTimer);
    }
  });

  function fmtBytes(n: number): string {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${(n / 1024 / 1024).toFixed(1)} MB`;
  }

  function progressPct(p: DownloadProgress): number {
    return p.estimateBytes > 0 ? Math.min(100, (p.receivedBytes / p.estimateBytes) * 100) : 0;
  }

  function isModelInstalled(model: CatalogModel): boolean {
    return player.llmStatus?.models.some((m) => m.name === model.id) ?? false;
  }

  function openAiSetup() {
    aiSetupOpen = true;
    player.refreshLlmStatus();
  }

  function submitYoutubeUrl(event: SubmitEvent) {
    event.preventDefault();
    if (!youtubeUrlInput.trim()) return;
    player.loadYoutubeUrl(youtubeUrlInput);
    youtubeUrlInput = "";
  }

  const adjustedPositionMs = $derived(player.positionMs - player.subtitleOffsetMs);
  const activeCue = $derived(
    player.subtitleCues.find((c) => adjustedPositionMs >= c.start_ms && adjustedPositionMs <= c.end_ms) ?? null,
  );
  const activeWords = $derived(activeCue ? activeCue.text.split(/\s+/).filter(Boolean) : []);
  const progressPercent = $derived(player.durationMs > 0 ? (player.positionMs / player.durationMs) * 100 : 0);
  // Tick marks on the seek bar for where each subtitle line starts, so line
  // density/spacing is visible without scrubbing blindly.
  const cueMarkerPercents = $derived(
    player.durationMs > 0
      ? player.subtitleCues.map((c) => ((c.start_ms + player.subtitleOffsetMs) / player.durationMs) * 100)
      : [],
  );

  /** Index of the last line that has started (binary search; cues are sorted). */
  function lastStartedIdx(adjustedMs: number): number {
    const cues = player.subtitleCues;
    let lo = 0;
    let hi = cues.length - 1;
    let found = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (cues[mid].start_ms <= adjustedMs) {
        found = mid;
        lo = mid + 1;
      } else {
        hi = mid - 1;
      }
    }
    return found;
  }

  // anchorIdx: the line the transcript is "at" (last started). nowIdx: that
  // same line, but only while it is actually being spoken.
  const anchorIdx = $derived(lastStartedIdx(adjustedPositionMs));
  const nowIdx = $derived(
    anchorIdx >= 0 && adjustedPositionMs <= player.subtitleCues[anchorIdx].end_ms ? anchorIdx : -1,
  );
  const cueProgress = $derived.by(() => {
    if (nowIdx < 0) return 0;
    const cue = player.subtitleCues[nowIdx];
    const span = Math.max(1, cue.end_ms - cue.start_ms);
    return Math.max(0, Math.min(100, ((adjustedPositionMs - cue.start_ms) / span) * 100));
  });
  const seekLine = $derived.by(() => {
    if (nowIdx < 0 || player.durationMs <= 0) return null;
    const cue = player.subtitleCues[nowIdx];
    return {
      left: ((cue.start_ms + player.subtitleOffsetMs) / player.durationMs) * 100,
      width: Math.max(0.4, ((cue.end_ms - cue.start_ms) / player.durationMs) * 100),
    };
  });

  const isYoutube = $derived(player.mediaKind === "youtube" || player.mediaKind === "youtube-embed");
  const nowTitle = $derived(
    // A downloaded YouTube video plays as a local file (named after its video
    // id) but carries its real title in `youtubeTitle`.
    player.youtubeTitle ?? (isYoutube ? "YouTube video" : baseName(player.currentFile ?? "")),
  );
  const nowMeta = $derived.by(() => {
    const parts: string[] = [isYoutube ? "YouTube" : "On this computer"];
    if (player.mediaKind === "youtube-embed") {
      parts.push("YouTube's own player");
    } else {
      const label = player.activeSubtitleLabel;
      parts.push(
        label === "Off"
          ? "No subtitles"
          : label === "External file"
            ? "Subtitles from a file"
            : `${label} subtitles`,
      );
      if (player.durationMs > 0) {
        if (player.positionMs < 5000) {
          parts.push("Not started");
        } else {
          const minIn = Math.floor(player.positionMs / 60_000);
          const minLeft = Math.max(0, Math.round((player.durationMs - player.positionMs) / 60_000));
          parts.push(`${minIn} min in, about ${minLeft} min left`);
        }
      }
    }
    return parts.join(" · ");
  });
  const subtitleChipText = $derived(
    player.activeSubtitleLabel === "Off" ? "Off" : player.activeSubtitleLabel.split(/[\s(]/)[0].slice(0, 8),
  );
  const loadingMessage = $derived(
    player.subtitleLoading
      ? "Reading the subtitles — only the first time for this video."
      : player.qualityLoading
        ? "Switching quality…"
        : player.aspectLoading
          ? "Adjusting the picture…"
          : player.youtubeLoading
            ? "Finding your YouTube video — this takes a few seconds."
            : player.youtubeDownloading
              ? "This one has to download first — hang tight."
              : "",
  );

  function downloadPhaseLabel(p: YoutubeDownloadProgress): string {
    if (p.phase === "merging") return "Combining audio and video…";
    const what = p.phase === "audio" ? "audio" : "video";
    return p.stages > 1 ? `Downloading ${what} (step ${p.stage} of ${p.stages})` : `Downloading ${what}`;
  }

  // Words saved from whatever is loaded: refresh when the video changes.
  $effect(() => {
    void player.currentFile;
    player.refreshSavedWords();
  });

  // Follow: keep the current line centred in the transcript.
  $effect(() => {
    const idx = anchorIdx;
    if (!follow || idx < 0 || railTab !== "transcript" || !railBodyEl) return;
    const el = railBodyEl.querySelector<HTMLElement>(`[data-cue="${idx}"]`);
    if (!el) return;
    railBodyEl.scrollTo({ top: el.offsetTop - railBodyEl.clientHeight / 2 + el.offsetHeight / 2, behavior: "smooth" });
  });

  function formatTime(ms: number): string {
    const totalSeconds = Math.max(0, Math.floor(ms / 1000));
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;
    return `${minutes}:${seconds.toString().padStart(2, "0")}`;
  }

  function formatDuration(ms: number): string {
    const total = Math.floor(ms / 1000);
    const h = Math.floor(total / 3600);
    const m = Math.floor((total % 3600) / 60);
    const sec = String(total % 60).padStart(2, "0");
    return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
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

  function recentProgressPercent(item: RecentItem): number {
    return item.durationMs > 0 ? Math.min(100, (item.positionMs / item.durationMs) * 100) : 0;
  }

  function recentProgressLabel(item: RecentItem): string {
    const pct = recentProgressPercent(item);
    if (pct >= 98) return "Finished";
    if (item.positionMs < 5000) return "Not started";
    return `${Math.max(1, Math.round(item.positionMs / 60_000))} min in`;
  }

  // Phone header sub-line, per the phone screens: "12 min in · Line 49 of
  // 162" while playing, "Paused · Line 48 of 162" when a word is open.
  const phoneMeta = $derived.by(() => {
    const line = player.subtitleCues.length > 0 && anchorIdx >= 0 ? `Line ${anchorIdx + 1} of ${player.subtitleCues.length}` : "";
    if (player.wordPopup) return ["Paused", line].filter(Boolean).join(" · ");
    if (player.durationMs > 0 && player.positionMs >= 5000) {
      const minIn = Math.floor(player.positionMs / 60_000);
      return [`${minIn} min in`, line].filter(Boolean).join(" · ");
    }
    return line;
  });

  // Punctuation is stripped before lookup and before matching against the
  // saved list ("language." and "language" are the same word).
  function cleanWord(word: string): string {
    return word.replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, "");
  }

  const savedSet = $derived(new Set(player.savedWords.map((w) => cleanWord(w.word).toLowerCase())));

  function isSaved(word: string): boolean {
    const clean = cleanWord(word).toLowerCase();
    return clean !== "" && savedSet.has(clean);
  }

  // The open word is only inverted in the line it was tapped in, so opening
  // "the" doesn't light up every "the" in the transcript.
  function isOpenIn(word: string, lineText: string): boolean {
    const popup = player.wordPopup;
    return !!popup && popup.sentence === lineText && cleanWord(word).toLowerCase() === popup.word.toLowerCase();
  }

  function onWordClick(word: string, event: MouseEvent) {
    const clean = cleanWord(word);
    if (!clean || !activeCue) return;
    const stageRect = videoStageEl.getBoundingClientRect();
    player.onWordActivate(clean, activeCue.text, event.clientX - stageRect.left, event.clientY - stageRect.top);
  }

  function onWordKeydown(word: string, event: KeyboardEvent) {
    const clean = cleanWord(word);
    if (event.key !== "Enter" || !clean || !activeCue) return;
    const stageRect = videoStageEl.getBoundingClientRect();
    const wordRect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    player.onWordActivate(
      clean,
      activeCue.text,
      wordRect.left - stageRect.left + wordRect.width / 2,
      wordRect.top - stageRect.top,
    );
  }

  function onCueWordClick(word: string, sentence: string, event: MouseEvent) {
    event.stopPropagation();
    const clean = cleanWord(word);
    if (!clean) return;
    player.onWordActivate(clean, sentence, 0, 0);
  }

  function openSavedWord(saved: SavedWord) {
    player.onWordActivate(saved.word, saved.context ?? "", 0, 0);
  }

  function toggleMute() {
    if (player.volume > 0) {
      lastVolume = player.volume;
      player.changeVolume(-player.volume);
    } else {
      player.changeVolume(lastVolume || 60);
    }
  }

  function onVolumeInput(event: Event) {
    const value = Number((event.target as HTMLInputElement).value);
    player.changeVolume(value - player.volume);
  }

  function pauseFollow() {
    follow = false;
  }

  const SEEK_STEP_MS = 10_000;

  function handleGlobalKeydown(event: KeyboardEvent) {
    wakeControls();

    // While the definition popup is open, Space always resumes playback —
    // takes priority over the interactive-control check below, since the
    // subtitle word that was clicked to open it keeps keyboard focus (it's
    // a role="button" span) and would otherwise swallow the keypress.
    if (player.wordPopup && event.code === "Space") {
      event.preventDefault();
      player.resumeFromPopup();
      return;
    }

    if (event.key === "Escape" && shortcutsHelpOpen) {
      event.preventDefault();
      shortcutsHelpOpen = false;
      return;
    }

    const target = event.target as HTMLElement | null;
    const isInteractiveControl =
      !!target &&
      (["INPUT", "TEXTAREA", "SELECT", "BUTTON"].includes(target.tagName) ||
        target.isContentEditable ||
        target.getAttribute("role") === "button");
    if (isInteractiveControl) return;

    if (event.key === "?") {
      event.preventDefault();
      shortcutsHelpOpen = !shortcutsHelpOpen;
    } else if (event.code === "Space") {
      event.preventDefault();
      player.togglePlay();
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      player.changeVolume(10);
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      player.changeVolume(-10);
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      player.seekBy(SEEK_STEP_MS);
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      player.seekBy(-SEEK_STEP_MS);
    } else if (event.key === "f" || event.key === "F") {
      event.preventDefault();
      toggleFullscreen();
    } else if (event.key === ",") {
      event.preventDefault();
      player.jumpToPreviousCue();
    } else if (event.key === ".") {
      event.preventDefault();
      player.jumpToNextCue();
    } else if (event.key === "l" || event.key === "L") {
      event.preventDefault();
      player.toggleLoopLine();
    } else if (event.key === "r" || event.key === "R") {
      event.preventDefault();
      player.replayCurrentCue();
    } else if (event.key === "[") {
      event.preventDefault();
      cyclePlaybackRate(-1);
    } else if (event.key === "]") {
      event.preventDefault();
      cyclePlaybackRate(1);
    }
  }

  function cyclePlaybackRate(direction: 1 | -1) {
    const idx = PLAYBACK_RATE_OPTIONS.findIndex((o) => o.value === player.playbackRate);
    const nextIdx = Math.max(0, Math.min(PLAYBACK_RATE_OPTIONS.length - 1, (idx < 0 ? 2 : idx) + direction));
    player.setPlaybackRate(PLAYBACK_RATE_OPTIONS[nextIdx].value);
  }

  function onSeekInputEvent(event: Event) {
    player.onSeekInput(Number((event.target as HTMLInputElement).value));
  }

  function onSeekCommitEvent(event: Event) {
    player.onSeekCommit(Number((event.target as HTMLInputElement).value));
  }

  const LOCAL_VIDEO_EXT = /\.(mp4|mkv|avi|mov|webm)$/i;
  let unlistenDragDrop: (() => void) | null = null;

  // The canvas element isn't bound yet when onMount first runs, so register
  // it from an effect that re-runs once `canvasEl` is actually assigned.
  $effect(() => {
    const el = canvasEl;
    if (!el) return;
    untrack(() => player.registerCanvas(el));
    return () => player.unregisterCanvas(el);
  });

  onMount(() => {
    recents = loadRecents();
    invoke<unknown[]>("list_vocabulary")
      .then((rows) => (wordsCount = rows.length))
      .catch(() => {});
    document.addEventListener("fullscreenchange", onFullscreenChange);

    // Tauri's own OS-level drag-drop (not HTML5 DnD, which it disables by
    // default) — lets a video be dropped straight onto the window instead
    // of only reachable through the file-picker button.
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "drop") {
          isDraggingFileOver = false;
          const path = event.payload.paths.find((p) => LOCAL_VIDEO_EXT.test(p));
          if (path) player.openDroppedPath(path);
        } else if (event.payload.type === "leave") {
          isDraggingFileOver = false;
        } else {
          isDraggingFileOver = true;
        }
      })
      .then((unlisten) => {
        unlistenDragDrop = unlisten;
      });
  });

  onDestroy(() => {
    document.removeEventListener("fullscreenchange", onFullscreenChange);
    if (hideControlsTimer) clearTimeout(hideControlsTimer);
    unlistenDragDrop?.();
  });
</script>

{#snippet iconPlay()}
  <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor"><path d="M8 5v14l11-7z" /></svg>
{/snippet}

{#snippet iconPause()}
  <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor"
    ><path d="M6 5h4v14H6zM14 5h4v14h-4z" /></svg
  >
{/snippet}

{#snippet iconVolume()}
  <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <path d="M4 9v6h3.2L12 19V5L7.2 9H4z" fill="currentColor" stroke="none" />
    <path d="M15.5 9.2a3.5 3.5 0 0 1 0 5.6" />
    <path d="M17.8 6.8a7 7 0 0 1 0 10.4" />
  </svg>
{/snippet}

{#snippet iconCaptions()}
  <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
    <rect x="3" y="5" width="18" height="14" rx="3" />
    <path d="M7 10.2h4M7 13.8h6.5M14.5 10.2h2.5" />
  </svg>
{/snippet}

{#snippet iconQuality()}
  <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
    <rect x="4" y="13" width="3.2" height="7" rx="1" />
    <rect x="10.4" y="8.5" width="3.2" height="11.5" rx="1" />
    <rect x="16.8" y="4" width="3.2" height="16" rx="1" />
  </svg>
{/snippet}

{#snippet iconAspect()}
  <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <rect x="2.5" y="6" width="19" height="12" rx="2" />
    <path d="M7 9v6M17 9v6" />
  </svg>
{/snippet}

{#snippet iconPrevLine()}
  <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor">
    <rect x="4" y="5" width="2.2" height="14" />
    <path d="M20 6L9 12l11 6V6z" />
  </svg>
{/snippet}

{#snippet iconNextLine()}
  <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor">
    <rect x="17.8" y="5" width="2.2" height="14" />
    <path d="M4 6l11 6-11 6V6z" />
  </svg>
{/snippet}

{#snippet iconReplay()}
  <svg viewBox="0 0 24 24" width="17" height="17" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <path d="M4 12a8 8 0 1 0 2.4-5.7" />
    <path d="M4 4.2V9h4.8" />
  </svg>
{/snippet}

{#snippet iconKeyboard()}
  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <rect x="2.5" y="6" width="19" height="12" rx="2" />
    <path d="M6 10h.01M9.5 10h.01M13 10h.01M16.5 10h.01M6 13.5h12" />
  </svg>
{/snippet}

{#snippet iconSpeed()}
  <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <path d="M12 3a9 9 0 1 0 9 9" />
    <path d="M12 12l4.3-4.3" />
    <circle cx="12" cy="12" r="1.3" fill="currentColor" stroke="none" />
  </svg>
{/snippet}

{#snippet iconTextSize()}
  <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor" font-family="sans-serif">
    <text x="1" y="17" font-size="11" font-weight="700">A</text>
    <text x="11" y="19" font-size="16" font-weight="700">A</text>
  </svg>
{/snippet}

{#snippet iconClose()}
  <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"
    ><path d="M6 6l12 12M18 6L6 18" /></svg
  >
{/snippet}

{#snippet iconPlus()}
  <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"
    ><path d="M12 5v14M5 12h14" /></svg
  >
{/snippet}

{#snippet iconCheck()}
  <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"
    ><path d="M4 12l5 5L20 6" /></svg
  >
{/snippet}

{#snippet iconFolder()}
  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <path d="M4 6.5a1 1 0 0 1 1-1h4.2l1.6 2h8.2a1 1 0 0 1 1 1V17a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6.5z" />
  </svg>
{/snippet}

{#snippet iconSparkle()}
  <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor">
    <path d="M12 3l1.6 4.9L18.5 9.5l-4.9 1.6L12 16l-1.6-4.9L5.5 9.5l4.9-1.6L12 3z" />
    <path d="M19 14l.7 2.1 2.1.7-2.1.7-.7 2.1-.7-2.1-2.1-.7 2.1-.7L19 14z" />
  </svg>
{/snippet}

{#snippet iconLoop()}
  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <path d="M17 3.5l2.5 2.5L17 8.5" />
    <path d="M4.5 11V9.5a3.5 3.5 0 0 1 3.5-3.5h11.5" />
    <path d="M7 20.5L4.5 18 7 15.5" />
    <path d="M19.5 13v1.5a3.5 3.5 0 0 1-3.5 3.5H4.5" />
  </svg>
{/snippet}

{#snippet iconLink()}
  <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
    <path d="M9.5 14.5l5-5" />
    <path d="M11 6.5l1-1a3.5 3.5 0 0 1 5 5l-1 1" />
    <path d="M13 17.5l-1 1a3.5 3.5 0 0 1-5-5l1-1" />
  </svg>
{/snippet}

{#snippet iconSpeaker()}
  <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <path d="M4 9v6h3.2L12 19V5L7.2 9H4z" fill="currentColor" stroke="none" />
    <path d="M15.5 9.2a3.5 3.5 0 0 1 0 5.6" />
  </svg>
{/snippet}

{#snippet iconFullscreenEnter()}
  <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <path d="M4 9V5a1 1 0 0 1 1-1h4M15 4h4a1 1 0 0 1 1 1v4M20 15v4a1 1 0 0 1-1 1h-4M9 20H5a1 1 0 0 1-1-1v-4" />
  </svg>
{/snippet}

{#snippet iconFullscreenExit()}
  <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <path d="M9 4v4a1 1 0 0 1-1 1H4M15 4v4a1 1 0 0 1 1 1h4M20 15h-4a1 1 0 0 0-1 1v4M4 15h4a1 1 0 0 1 1 1v4" />
  </svg>
{/snippet}

{#snippet rateMenu()}
  <div class="menu menu-up" role="menu">
    <div class="menu-label">Speed</div>
    {#each PLAYBACK_RATE_OPTIONS as option (option.value)}
      <button class="menu-item" class:is-selected={player.playbackRate === option.value} onclick={() => player.setPlaybackRate(option.value, () => (rateMenuOpen = false))}>
        <span class="menu-item-check">{#if player.playbackRate === option.value}{@render iconCheck()}{/if}</span>
        {option.label}
      </button>
    {/each}
  </div>
{/snippet}

{#snippet wordPanel(popup: WordPopup)}
  <div class="word-head">
    <div>
      <h2 class="headword">{popup.word}</h2>
      {#if popup.definition && (popup.definition.phonetic || popup.definition.audio_url)}
        <p class="phonetic">
          {#if popup.definition.phonetic}{popup.definition.phonetic}{/if}
          {#if popup.definition.audio_url}
            <button class="icon-btn icon-btn-sm" onclick={() => player.pronounceWord()} title="Hear it" aria-label="Hear it">
              {@render iconSpeaker()}
            </button>
          {/if}
        </p>
      {/if}
    </div>
    <button class="icon-btn icon-btn-sm" onclick={() => player.closePopup()} aria-label="Close">{@render iconClose()}</button>
  </div>

  {#if popup.loading}
    <p class="popup-status"><span class="working"><span class="dots"><i></i><i></i><i></i></span> Looking it up…</span></p>
  {:else if popup.error}
    <p class="popup-status error">Couldn’t find that word. Check the spelling, or try another.{#if popup.error} ({popup.error}){/if}</p>
  {:else if popup.definition}
    {#if popup.definition.meanings.length > 0}
      {#each popup.definition.meanings as meaning (meaning.part_of_speech)}
        <p class="pos">{meaning.part_of_speech}</p>
        <ol class="defs">
          {#each meaning.definitions as def, i (i)}
            <li>{def}</li>
          {/each}
        </ol>
      {/each}
    {:else if popup.definition.summary}
      <p class="popup-definition">{popup.definition.summary}</p>
      <p class="popup-example">From Wikipedia</p>
    {/if}
  {/if}

  {#if player.isLlmReady()}
    <div class="ai-explain">
      <div class="ai-actions">
        {#if !popup.aiLoading && !popup.aiText && !popup.aiError}
          <button class="btn btn-outline btn-sm ai-btn" onclick={() => player.explainInContext()}>
            {@render iconSparkle()} Explain this
          </button>
        {/if}
        {#if !popup.examplesLoading && !popup.examplesText && !popup.examplesError}
          <button class="btn btn-outline btn-sm ai-btn" onclick={() => player.showWordExamples()}>
            {@render iconSparkle()} More examples
          </button>
        {/if}
      </div>

      {#if popup.aiLoading}
        <p class="popup-status"><span class="working"><span class="dots"><i></i><i></i><i></i></span> Thinking about this scene…</span></p>
      {:else if popup.aiError}
        <p class="popup-status error">{popup.aiError}</p>
        <button class="btn btn-outline btn-sm ai-btn" onclick={() => player.explainInContext()}>Try again</button>
      {:else if popup.aiText}
        <p class="ai-label">{@render iconSparkle()} In this scene</p>
        <p class="popup-definition">{popup.aiText}</p>
      {/if}

      {#if popup.examplesLoading}
        <p class="popup-status"><span class="working"><span class="dots"><i></i><i></i><i></i></span> Finding examples…</span></p>
      {:else if popup.examplesError}
        <p class="popup-status error">{popup.examplesError}</p>
        <button class="btn btn-outline btn-sm ai-btn" onclick={() => player.showWordExamples()}>Try again</button>
      {:else if popup.examplesText}
        <p class="ai-label">{@render iconSparkle()} More examples</p>
        <p class="popup-definition examples-text">{popup.examplesText}</p>
      {/if}
    </div>
  {/if}

  <div class="popup-actions">
    <button class="btn btn-outline" disabled={popup.added} onclick={() => player.addToVocabulary()}>
      {#if popup.added}
        {@render iconCheck()} Saved
      {:else}
        {@render iconPlus()} Save word
      {/if}
    </button>
    <button class="btn btn-primary" onclick={() => player.resumeFromPopup()} title="Space">
      {@render iconPlay()} Keep watching
    </button>
  </div>
{/snippet}

{#snippet statusLine()}
  {#if player.youtubeDownloading}
    {@const p = player.downloadProgress}
    <div class="download-panel" role="status" aria-live="polite">
      <div class="dl-head">
        <strong>{p ? downloadPhaseLabel(p) : "Preparing download…"}</strong>
        <span class="dl-quality">{player.downloadQuality.height ? `${player.downloadQuality.height}p` : "Auto quality"}</span>
      </div>
      <div
        class="dl-bar"
        class:indeterminate={!p || p.phase === "merging"}
        role="progressbar"
        aria-label="Download progress"
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow={p && p.phase !== "merging" ? Math.round(p.overall_percent) : undefined}
      >
        <span style={p && p.phase !== "merging" ? `width: ${p.overall_percent}%` : ""}></span>
      </div>
      <div class="dl-meta">
        {#if !p}
          <span>Contacting YouTube…</span>
        {:else if p.phase === "merging"}
          <span>Almost done — joining the two streams.</span>
        {:else}
          <span class="dl-percent">{Math.round(p.overall_percent)}%</span>
          <span>{formatBytes(p.downloaded_bytes)}{p.total_bytes ? ` of ${formatBytes(p.total_bytes)}` : ""}</span>
          {#if p.speed_bps}<span>{formatBytes(p.speed_bps)}/s</span>{/if}
          {#if p.eta_secs != null}<span>{formatEta(p.eta_secs)} left</span>{/if}
        {/if}
      </div>
    </div>
  {:else if loadingMessage}
    <span class="working" role="status"><span class="dots"><i></i><i></i><i></i></span> {loadingMessage}</span>
  {/if}
{/snippet}

<svelte:window onkeydown={handleGlobalKeydown} />

<main class="page">
  <AppHeader
    wordsCount={wordsCount}
    showLinkbar={!!player.currentFile}
    bind:linkValue={youtubeUrlInput}
    linkLoading={player.youtubeLoading}
    onLinkSubmit={(v) => player.loadYoutubeUrl(v)}
    mobileTitle={player.currentFile ? nowTitle : ""}
    mobileMeta={phoneMeta}
  >
    {#snippet menuExtras()}
      <button class="menu-item" role="menuitem" onclick={() => openAiSetup()}>
        {@render iconSparkle()} AI helper
      </button>
      <button class="menu-item" role="menuitem" onclick={() => (shortcutsHelpOpen = true)}>
        {@render iconKeyboard()} Shortcuts <span class="menu-hint">?</span>
      </button>
      <div class="menu-divider"></div>
      <button class="menu-item" role="menuitemcheckbox" aria-checked={player.youtubeEmbedMode} onclick={() => player.setYoutubeEmbedMode(!player.youtubeEmbedMode)}>
        <span class="menu-item-check">{#if player.youtubeEmbedMode}{@render iconCheck()}{/if}</span>
        Use YouTube's player for links
      </button>
      <div class="menu-divider"></div>
      <div class="menu-label">Download quality</div>
      {#each DOWNLOAD_QUALITY_OPTIONS as option (option.height)}
        <button class="menu-item" role="menuitemradio" aria-checked={player.downloadQuality.height === option.height} onclick={() => player.setDownloadQuality(option)}>
          <span class="menu-item-check">{#if player.downloadQuality.height === option.height}{@render iconCheck()}{/if}</span>
          {option.label}
        </button>
      {/each}
    {/snippet}
  </AppHeader>

  {#if !player.currentFile}
    <!-- Start screen (design: "Start — no video yet"): hero, dropzone with
         the link form, and "Pick up where you left off" recents. -->
    <div class="start">
      {@render statusLine()}
      {#if player.errorMessage}
        <div class="notice notice-error" role="alert">
          <span class="notice-icon">{@render iconClose()}</span>
          <span>{player.errorMessage}</span>
        </div>
      {/if}

      <div class="start-hero">
        <h1>What would you like to watch?</h1>
        <p>Open a video with subtitles, then tap any word you don't know. We'll explain it and keep it for you.</p>
      </div>

      <div class="dropzone" class:dragging={isDraggingFileOver}>
        {#if isDraggingFileOver}
          <div class="drop-overlay" aria-hidden="true">
            {@render iconFolder()}
            <p>Drop to open this video</p>
          </div>
        {/if}
        <span class="drop-icon">{@render iconFolder()}</span>
        <p class="drop-title">Drop a video file here</p>
        <button class="btn btn-primary btn-lg" type="button" onclick={() => player.pickFile()}>Choose a file</button>
        <div class="or-divider"><span></span>or paste a link<span></span></div>
        <form class="linkbar linkbar--hero" onsubmit={submitYoutubeUrl}>
          {@render iconLink()}
          <label class="sr-only" for="start-link">YouTube link</label>
          <input
            id="start-link"
            type="text"
            placeholder="https://youtube.com/watch?v=…"
            aria-label="YouTube link"
            bind:value={youtubeUrlInput}
            disabled={player.youtubeLoading}
          />
          <button class="btn" type="submit" disabled={player.youtubeLoading || !youtubeUrlInput.trim()}>
            {player.youtubeLoading ? "Opening…" : "Load"}
          </button>
        </form>
        <label class="check check--accent" title="Simpler, but you can't tap words for meanings.">
          <input
            type="checkbox"
            checked={player.youtubeEmbedMode}
            onchange={(e) => player.setYoutubeEmbedMode(e.currentTarget.checked)}
          />
          Use YouTube's own player
        </label>
        <label class="quality-select" title="Used when a YouTube video has to be downloaded before it can play. Playback is capped at 1080p.">
          <span>Download quality</span>
          <select
            value={player.downloadQuality.height}
            onchange={(e) =>
              player.setDownloadQuality(
                DOWNLOAD_QUALITY_OPTIONS.find((o) => o.height === Number(e.currentTarget.value)) ?? DOWNLOAD_QUALITY_OPTIONS[0],
              )}
          >
            {#each DOWNLOAD_QUALITY_OPTIONS as option (option.height)}
              <option value={option.height}>{option.label}</option>
            {/each}
          </select>
        </label>
      </div>

      {#if recents.length > 0}
        <section class="start-recents">
          <div class="section-head">
            <h2>Pick up where you left off</h2>
            <a class="section-link" href="/library">See all in Library</a>
          </div>
          <div class="recents-row">
            {#each recents.slice(0, 2) as item (item.id)}
              <button class="recent-card" type="button" onclick={() => player.resumeRecent(item)}>
                <span class="recent-thumb">
                  {#if item.kind === "youtube"}{@render iconPlay()}{:else}{@render iconFolder()}{/if}
                  {#if item.durationMs > 0}<span class="dur">{formatDuration(item.durationMs)}</span>{/if}
                </span>
                <span class="recent-body">
                  <span class="recent-title">{item.title || baseName(item.path)}</span>
                  <span class="recent-meta">{formatRelative(item.lastOpenedAt)}</span>
                  <span class="recent-progress">
                    <span class="bar"><span style="width: {recentProgressPercent(item)}%"></span></span>
                    <span class="label">{recentProgressLabel(item)}</span>
                  </span>
                </span>
              </button>
            {/each}
          </div>
        </section>
      {/if}
    </div>
  {:else}
  <div class="player-screen">
    <div class="player-main">
      {@render statusLine()}

      <div
        class="player-card"
        class:embed-layout={player.mediaKind === "youtube-embed"}
        class:cursor-hidden={isFullscreen && !controlsVisible}
        role="group"
        aria-label="Video player"
        bind:this={playerCardEl}
        onmousemove={wakeControls}
        onmouseleave={onPlayerCardMouseLeave}
      >
        {#if isDraggingFileOver}
          <div class="drop-overlay" aria-hidden="true">
            {@render iconFolder()}
            <p>Drop to open this video</p>
          </div>
        {/if}

        <div class="video-stage" bind:this={videoStageEl}>
          {#if isFullscreen}
            <span class="fs-title">{nowTitle}</span>
          {/if}
          {#if player.mediaKind === "youtube-embed" && player.youtubeEmbedVideoId}
            <iframe
              class="youtube-embed-frame"
              src="https://www.youtube-nocookie.com/embed/{player.youtubeEmbedVideoId}?rel=0{player.youtubeEmbedStartSeconds > 0
                ? `&start=${player.youtubeEmbedStartSeconds}`
                : ''}"
              title="YouTube video player"
              allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share"
              allowfullscreen
            ></iframe>
          {/if}
          <canvas bind:this={canvasEl} style={player.mediaKind === "youtube-embed" ? "display: none;" : ""}></canvas>

          {#if !isFullscreen && player.loopLine && player.mediaKind !== "youtube-embed"}
            <span class="video-chip"><i></i> Looping this line</span>
          {/if}
          {#if !isFullscreen && player.wordPopup && player.mediaKind !== "youtube-embed"}
            <span class="video-chip"><i></i> Paused on a word</span>
          {/if}

          {#if activeWords.length > 0 && player.mediaKind !== "youtube-embed"}
            <div
              class="subtitle-overlay {positionClass(player.subtitlePosition)}"
              class:controls-visible={isFullscreen && controlsVisible && player.subtitlePosition === "Bottom"}
              style={subtitleStyleVars(player)}
            >
              {#each activeWords as word, i (i)}
                <span
                  class="subtitle-word"
                  class:is-saved={isSaved(word)}
                  class:is-open={isOpenIn(word, activeCue?.text ?? "")}
                  role="button"
                  tabindex="0"
                  onclick={(e) => onWordClick(word, e)}
                  onkeydown={(e) => onWordKeydown(word, e)}
                >{word}</span>
              {/each}
            </div>
          {/if}

          {#if player.wordPopup && isFullscreen}
            <div class="word-popup" style="left: {player.wordPopup.x}px; top: {player.wordPopup.y}px;">
              {@render wordPanel(player.wordPopup)}
              <span class="popup-tail" aria-hidden="true"></span>
            </div>
          {/if}
        </div>

        {#if player.mediaKind !== "youtube-embed"}
          <div class="dock" class:hidden={isFullscreen && !controlsVisible}>
            <div class="seek-wrap">
              <input
                class="seek"
                style="--progress: {progressPercent}%"
                type="range"
                min="0"
                max={player.durationMs || 0}
                value={player.positionMs}
                disabled={!player.currentFile}
                title="Seek (Left and Right arrow keys jump 10 seconds)"
                aria-label="Seek"
                oninput={onSeekInputEvent}
                onpointerdown={() => (player.seeking = true)}
                onchange={onSeekCommitEvent}
              />
              <div class="seek-markers" aria-hidden="true">
                {#each cueMarkerPercents as pct, i (i)}
                  <span class="seek-marker" style="left: {pct}%"></span>
                {/each}
                {#if seekLine}
                  <span class="seek-line" style="left: {seekLine.left}%; width: {seekLine.width}%"></span>
                {/if}
              </div>
            </div>
            <div class="seek-times">
              <span><b>{formatTime(player.positionMs)}</b> / {formatTime(player.durationMs)}</span>
              {#if player.subtitleCues.length > 0 && anchorIdx >= 0}
                <span>Line {anchorIdx + 1} of {player.subtitleCues.length}</span>
              {/if}
            </div>

            <div class="dock-row">
              <div class="dock-left">
                <span class="volume">
                  <button class="icon-btn" onclick={toggleMute} disabled={!player.currentFile} aria-label={player.volume === 0 ? "Unmute" : "Mute"} title="Volume (Up and Down arrow keys)">
                    {@render iconVolume()}
                  </button>
                  <input
                    class="volume-track"
                    type="range"
                    min="0"
                    max="100"
                    value={player.volume}
                    style="--progress: {player.volume}%"
                    disabled={!player.currentFile}
                    aria-label="Volume"
                    oninput={onVolumeInput}
                  />
                </span>
              </div>

              <div class="transport">
                <button
                  class="line-btn"
                  onclick={() => player.jumpToPreviousCue()}
                  disabled={!player.currentFile || player.subtitleCues.length === 0}
                  title="Previous line (,)"
                >
                  {@render iconPrevLine()}<span>Back</span>
                </button>
                <button
                  class="line-btn"
                  onclick={() => player.replayCurrentCue()}
                  disabled={!player.currentFile || player.subtitleCues.length === 0}
                  title="Say it again (R)"
                >
                  {@render iconReplay()}<span>Again</span>
                </button>
                <button
                  class="icon-btn play-lg"
                  onclick={() => player.togglePlay()}
                  disabled={!player.currentFile}
                  title={player.isPlaying ? "Pause (Space)" : "Play (Space)"}
                  aria-label={player.isPlaying ? "Pause" : "Play"}
                >
                  {@render (player.isPlaying ? iconPause : iconPlay)()}
                </button>
                <button
                  class="line-btn"
                  onclick={() => player.jumpToNextCue()}
                  disabled={!player.currentFile || player.subtitleCues.length === 0}
                  title="Next line (.)"
                >
                  {@render iconNextLine()}<span>Next</span>
                </button>
                <button
                  class="line-btn line-btn-loop"
                  class:is-active={player.loopLine}
                  aria-pressed={player.loopLine}
                  onclick={() => player.toggleLoopLine()}
                  disabled={!player.currentFile || player.subtitleCues.length === 0}
                  title="Loop this line (L)"
                >
                  {@render iconLoop()}<span>Loop</span>
                </button>
                <!-- Phone transport: speed lives in the row instead of the
                     right-hand chip cluster (hidden on narrow screens). -->
                <div class="menu-trigger speed-trigger">
                  <button
                    class="line-btn speed-btn"
                    class:is-active={rateMenuOpen}
                    onclick={() => (rateMenuOpen = !rateMenuOpen)}
                    disabled={!player.currentFile}
                    title={`Speed: ${player.playbackRate}× ([ and ] to change)`}
                    aria-label="Playback speed"
                  >
                    <span class="rate">{player.playbackRate}×</span>
                    <span>Speed</span>
                  </button>
                  {#if rateMenuOpen}
                    <button class="menu-backdrop" aria-label="Close speed menu" onclick={() => (rateMenuOpen = false)}></button>
                    {@render rateMenu()}
                  {/if}
                </div>
              </div>

              <div class="dock-right">
                <div class="menu-trigger">
                  <button
                    class="chip-btn"
                    onclick={() => (rateMenuOpen = !rateMenuOpen)}
                    disabled={!player.currentFile}
                    title={`Speed: ${player.playbackRate}× ([ and ] to change)`}
                    aria-label="Playback speed"
                  >
                    {@render iconSpeed()} {player.playbackRate}×
                  </button>
                  {#if rateMenuOpen}
                    <button class="menu-backdrop" aria-label="Close speed menu" onclick={() => (rateMenuOpen = false)}></button>
                    {@render rateMenu()}
                  {/if}
                </div>

                <div class="menu-trigger">
                  <button
                    class="chip-btn"
                    disabled={!player.currentFile || player.subtitleLoading}
                    onclick={() => (subtitleMenuOpen = !subtitleMenuOpen)}
                    title={player.subtitleLoading ? "Reading the subtitles…" : `Subtitles: ${player.activeSubtitleLabel}`}
                    aria-label="Subtitles"
                  >
                    {@render iconCaptions()} {subtitleChipText}
                  </button>
                  {#if subtitleMenuOpen}
                    <button class="menu-backdrop" aria-label="Close subtitle menu" onclick={() => (subtitleMenuOpen = false)}></button>
                    <div class="menu menu-up" role="menu">
                      <div class="menu-label">Subtitles</div>
                      <button class="menu-item" class:is-selected={player.activeSubtitleLabel === "Off"} onclick={() => player.turnSubtitlesOff(() => (subtitleMenuOpen = false))}>
                        <span class="menu-item-check">{#if player.activeSubtitleLabel === "Off"}{@render iconCheck()}{/if}</span>
                        Off
                      </button>
                      {#each player.embeddedTracks as track (track.id)}
                        <button class="menu-item" class:is-selected={player.activeSubtitleLabel === trackLabel(track)} onclick={() => player.selectEmbeddedTrack(track, () => (subtitleMenuOpen = false))}>
                          <span class="menu-item-check">{#if player.activeSubtitleLabel === trackLabel(track)}{@render iconCheck()}{/if}</span>
                          {trackLabel(track)}
                        </button>
                      {/each}
                      <button class="menu-item" onclick={() => player.pickExternalSubtitles(() => (subtitleMenuOpen = false))}>
                        <span class="menu-item-check"></span>
                        Choose a file…
                      </button>
                      {#if player.activeSubtitleLabel !== "Off"}
                        <div class="menu-divider"></div>
                        <div class="menu-label">Timing · {player.subtitleOffsetMs >= 0 ? "+" : "−"}{Math.abs(player.subtitleOffsetMs / 1000).toFixed(2)}s</div>
                        <div class="sync-offset-row">
                          <button class="btn btn-sm btn-stage" onclick={() => player.nudgeSubtitleOffset(-100)} aria-label="Show subtitles a little earlier">Earlier</button>
                          <button class="btn btn-sm btn-stage" onclick={() => player.resetSubtitleOffset()} disabled={player.subtitleOffsetMs === 0} aria-label="Reset subtitle timing">Reset</button>
                          <button class="btn btn-sm btn-stage" onclick={() => player.nudgeSubtitleOffset(100)} aria-label="Show subtitles a little later">Later</button>
                        </div>
                      {/if}
                    </div>
                  {/if}
                </div>

                <div class="menu-trigger">
                  <button
                    class="icon-btn"
                    onclick={() => (fontMenuOpen = !fontMenuOpen)}
                    title="Subtitle text style"
                    aria-label="Subtitle text style"
                  >
                    {@render iconTextSize()}
                  </button>
                  {#if fontMenuOpen}
                    <button class="menu-backdrop" aria-label="Close text style menu" onclick={() => (fontMenuOpen = false)}></button>
                    <div class="menu menu-up" role="menu">
                      <SubtitleStylePanel />
                    </div>
                  {/if}
                </div>

                <div class="menu-trigger">
                  <button
                    class="icon-btn"
                    disabled={!player.currentFile || player.qualityLoading}
                    onclick={() => (qualityMenuOpen = !qualityMenuOpen)}
                    title={player.qualityLoading ? "Switching quality…" : `Quality: ${player.quality.label}`}
                    aria-label="Video quality"
                  >
                    {@render iconQuality()}
                  </button>
                  {#if qualityMenuOpen}
                    <button class="menu-backdrop" aria-label="Close quality menu" onclick={() => (qualityMenuOpen = false)}></button>
                    <div class="menu menu-up" role="menu">
                      <div class="menu-label">Quality</div>
                      {#each QUALITY_OPTIONS as option (option.maxDimension)}
                        <button class="menu-item" class:is-selected={player.quality.maxDimension === option.maxDimension} onclick={() => player.changeQuality(option, () => (qualityMenuOpen = false))}>
                          <span class="menu-item-check">{#if player.quality.maxDimension === option.maxDimension}{@render iconCheck()}{/if}</span>
                          {option.label}
                        </button>
                      {/each}
                    </div>
                  {/if}
                </div>

                <div class="menu-trigger">
                  <button
                    class="icon-btn"
                    disabled={!player.currentFile || player.aspectLoading}
                    onclick={() => (aspectMenuOpen = !aspectMenuOpen)}
                    title={player.aspectLoading ? "Adjusting the picture…" : `Picture shape: ${player.aspect.label}`}
                    aria-label="Picture shape"
                  >
                    {@render iconAspect()}
                  </button>
                  {#if aspectMenuOpen}
                    <button class="menu-backdrop" aria-label="Close picture shape menu" onclick={() => (aspectMenuOpen = false)}></button>
                    <div class="menu menu-up" role="menu">
                      <div class="menu-label">Picture shape</div>
                      {#each ASPECT_OPTIONS as option (option.label)}
                        <button class="menu-item" class:is-selected={player.aspect.label === option.label} onclick={() => player.changeAspect(option, () => (aspectMenuOpen = false))}>
                          <span class="menu-item-check">{#if player.aspect.label === option.label}{@render iconCheck()}{/if}</span>
                          {option.label}
                        </button>
                      {/each}
                    </div>
                  {/if}
                </div>

                <button
                  class="icon-btn"
                  onclick={toggleFullscreen}
                  disabled={!player.currentFile}
                  title="Fullscreen (F)"
                  aria-label={isFullscreen ? "Leave fullscreen" : "Go fullscreen"}
                >
                  {@render (isFullscreen ? iconFullscreenExit : iconFullscreenEnter)()}
                </button>
              </div>
            </div>
          </div>
        {/if}
      </div>

      {#if player.mediaKind === "youtube-embed"}
        <div class="notice notice-info" role="note">
          <span class="notice-icon">{@render iconCaptions()}</span>
          <span><b>YouTube's player is on.</b> Its captions are drawn inside the video, so words can't be tapped for meanings.</span>
          <button class="btn btn-sm notice-action" type="button" onclick={() => player.openWithWordLookup()}>
            Use tap-to-define player
          </button>
        </div>
      {/if}

      {#if player.errorMessage}
        <div class="notice notice-error" role="alert">
          <span class="notice-icon">{@render iconClose()}</span>
          <span>{player.errorMessage}</span>
        </div>
      {/if}

      {#if player.infoMessage}
        <div class="notice notice-info" role="status">
          <span class="notice-icon">{@render iconCheck()}</span>
          <span>{player.infoMessage}</span>
          <button class="btn btn-sm notice-action" type="button" onclick={() => (player.infoMessage = "")}>Dismiss</button>
        </div>
      {/if}

      {#if player.currentFile}
        <div class="now">
          <div class="now-text">
            <h1 class="now-title">{nowTitle}</h1>
            <p class="now-meta">{nowMeta}</p>
          </div>
          {#if player.mediaKind !== "youtube-embed"}
            <div class="now-tools">
              <label class="check" title="Stop at the end of every line so you can tap words before moving on">
                <input
                  type="checkbox"
                  checked={player.pauseAfterLine}
                  onchange={(e) => player.setPauseAfterLine((e.currentTarget as HTMLInputElement).checked)}
                />
                Pause after each line
              </label>
            </div>
          {/if}
        </div>

        {#if player.savedWords.length > 0}
          <div class="saved-strip">
            <span class="strip-label">Saved from this video</span>
            {#each player.savedWords as saved (saved.word)}
              <button class="word-chip" onclick={() => openSavedWord(saved)}>{saved.word}</button>
            {/each}
          </div>
        {/if}
      {/if}
    </div>

    {#if player.currentFile && player.mediaKind !== "youtube-embed"}
      <aside class="rail" aria-label="Transcript">
        <div class="rail-head">
          <div class="segmented" role="group" aria-label="Show">
            <button aria-pressed={railTab === "transcript"} onclick={() => (railTab = "transcript")}>Transcript</button>
            <button aria-pressed={railTab === "saved"} onclick={() => (railTab = "saved")}>Saved here · {player.savedWords.length}</button>
          </div>
          {#if railTab === "transcript"}
            <label class="check" title="Keep the current line in view">
              <input type="checkbox" bind:checked={follow} />
              Follow
            </label>
          {/if}
        </div>

        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="rail-body" bind:this={railBodyEl} onwheel={pauseFollow} ontouchmove={pauseFollow}>
          {#if railTab === "transcript"}
            {#if player.subtitleCues.length === 0}
              <p class="rail-empty">
                {player.subtitleLoading
                  ? "Reading the subtitles — only the first time for this video."
                  : "No subtitles loaded yet. Choose a subtitle track from the dock to see every line here."}
              </p>
            {:else}
              {#each player.subtitleCues as cue, i (i)}
                <div
                  class="cue"
                  class:is-now={i === nowIdx}
                  class:is-past={i < anchorIdx || (i === anchorIdx && nowIdx !== i)}
                  data-cue={i}
                  role="button"
                  tabindex="0"
                  onclick={() => player.seekTo(cue.start_ms + player.subtitleOffsetMs)}
                  onkeydown={(e) => e.key === "Enter" && player.seekTo(cue.start_ms + player.subtitleOffsetMs)}
                >
                  <span class="cue-time">{formatTime(cue.start_ms + player.subtitleOffsetMs)}</span>
                  <span class="cue-text">
                    {#each cue.text.split(/(\s+)/) as part, k (k)}{#if part.trim() === ""}{part}{:else}<span
                          class="cue-word"
                          class:marked={isSaved(part)}
                          class:is-open={isOpenIn(part, cue.text)}
                          role="button"
                          tabindex="-1"
                          onclick={(e) => onCueWordClick(part, cue.text, e)}
                          onkeydown={() => {}}
                        >{part}</span>{/if}{/each}
                  </span>
                  {#if i === nowIdx}
                    <div class="cue-progress"><span style="width: {cueProgress}%"></span></div>
                  {/if}
                </div>
              {/each}
            {/if}
          {:else if player.savedWords.length === 0}
            <p class="rail-empty">Nothing saved from this video yet. Tap a word in the subtitles or the transcript and choose "Save word".</p>
          {:else}
            {#each player.savedWords as saved (saved.word)}
              <div class="cue saved-row" role="button" tabindex="0" onclick={() => openSavedWord(saved)} onkeydown={(e) => e.key === "Enter" && openSavedWord(saved)}>
                <span class="cue-time">{saved.timestampMs != null ? formatTime(saved.timestampMs) : ""}</span>
                <span class="cue-text">
                  <strong class="saved-word">{saved.word}</strong>
                  {#if saved.context}<span class="saved-context">“{saved.context}”</span>{/if}
                </span>
              </div>
            {/each}
          {/if}

          {#if railTab === "transcript" && !follow && anchorIdx >= 0}
            <button class="btn btn-outline btn-sm back-now" onclick={() => (follow = true)}>Back to now</button>
          {/if}
        </div>

        <div class="rail-foot">
          <span><kbd class="kbd">Space</kbd>play or pause</span>
          <span><kbd class="kbd">R</kbd>again</span>
          <span><kbd class="kbd">L</kbd>loop</span>
          <span><kbd class="kbd">,</kbd><kbd class="kbd">.</kbd>lines</span>
        </div>

        {#if player.wordPopup && !isFullscreen}
          <button class="sheet-scrim" aria-label="Close word" onclick={() => player.closePopup()}></button>
          <div class="sheet" role="dialog" aria-label={player.wordPopup.word}>
            <div class="sheet-grip"></div>
            {@render wordPanel(player.wordPopup)}
          </div>
        {/if}
      </aside>
    {/if}
  </div>
  {/if}

  {#if aiSetupOpen}
    <div class="modal-backdrop" onclick={() => (aiSetupOpen = false)} role="presentation"></div>
    <div class="modal-panel" role="dialog" aria-label="Local AI setup">
      <div class="modal-header">
        <h2>Local AI setup</h2>
        <button class="modal-close" onclick={() => (aiSetupOpen = false)} aria-label="Close">{@render iconClose()}</button>
      </div>
      <p class="modal-intro">
        Runs a small language model on your own machine to explain words <em>as used in their subtitle sentence</em> —
        catches idioms and slang the dictionary lookup can't. Fully offline; nothing leaves your computer. Needs a
        one-time download below (a couple hundred MB to a few GB depending on the model you pick).
      </p>

      {#if player.llmSetupError}
        <p class="error-banner">{player.llmSetupError}</p>
      {/if}

      <section class="setup-section">
        <div class="setup-section-header">
          <h3>Engine</h3>
          <span class="status-pill" class:ok={player.llmStatus?.binary_exists}>
            {player.llmStatus?.binary_exists ? "Installed" : "Not installed (~30-50 MB)"}
          </span>
        </div>
        <button class="btn btn-primary" disabled={player.llmBinaryInstalling} onclick={() => player.installLlmBinary()}>
          {player.llmBinaryInstalling ? player.llmBinaryProgress.phase : player.llmStatus?.binary_exists ? "Re-install" : "Install"}
        </button>
        {#if player.llmBinaryInstalling}
          <div class="setup-progress">
            <div class="setup-progress-bar"><span style="width: {progressPct(player.llmBinaryProgress)}%"></span></div>
            <span class="setup-progress-label">{fmtBytes(player.llmBinaryProgress.receivedBytes)}</span>
          </div>
        {/if}
      </section>

      <section class="setup-section">
        <h3>Models</h3>
        <ul class="model-catalog">
          {#each LLM_CATALOG as model (model.id)}
            <li class="model-row">
              <div class="model-info">
                <strong>{model.label}</strong>
                <span class="model-desc">{model.desc} · ~{model.sizeMb} MB</span>
              </div>
              {#if isModelInstalled(model)}
                <span class="status-pill ok">{@render iconCheck()} Installed</span>
              {:else if player.llmModelInstalling[model.id]}
                <div class="setup-progress inline">
                  <div class="setup-progress-bar"><span style="width: {progressPct(player.llmModelInstalling[model.id])}%"></span></div>
                  <span class="setup-progress-label">{fmtBytes(player.llmModelInstalling[model.id].receivedBytes)}</span>
                </div>
              {:else}
                <button class="btn btn-outline" onclick={() => player.installLlmModel(model)}>Install</button>
              {/if}
            </li>
          {/each}
        </ul>

        <div class="custom-model-row">
          <input
            type="text"
            placeholder="Or paste a direct .gguf URL…"
            bind:value={player.llmCustomUrl}
            disabled={player.llmCustomInstalling}
          />
          <button
            class="btn btn-outline"
            disabled={player.llmCustomInstalling || !player.llmCustomUrl.trim()}
            onclick={() => player.installLlmCustomModel()}
          >
            {player.llmCustomInstalling ? "Downloading…" : "Install"}
          </button>
        </div>
        {#if player.llmCustomInstalling}
          <div class="setup-progress">
            <div class="setup-progress-bar"><span style="width: {progressPct(player.llmCustomProgress)}%"></span></div>
            <span class="setup-progress-label">{fmtBytes(player.llmCustomProgress.receivedBytes)}</span>
          </div>
        {/if}

        {#if player.llmStatus && player.llmStatus.models.length > 1}
          <label class="active-model-picker">
            Active model
            <select value={player.currentLlmModel()?.path} onchange={(e) => player.setLlmModelPath((e.target as HTMLSelectElement).value)}>
              {#each player.llmStatus.models as m (m.path)}
                <option value={m.path}>{m.name}</option>
              {/each}
            </select>
          </label>
        {/if}
      </section>
    </div>
  {/if}

  {#if shortcutsHelpOpen}
    <div class="modal-backdrop" onclick={() => (shortcutsHelpOpen = false)} role="presentation"></div>
    <div class="modal-panel" role="dialog" aria-label="Keyboard shortcuts">
      <div class="modal-header">
        <h2>Keyboard shortcuts</h2>
        <button class="modal-close" onclick={() => (shortcutsHelpOpen = false)} aria-label="Close">{@render iconClose()}</button>
      </div>
      <ul class="shortcuts-list">
        <li><kbd>Space</kbd><span>Play / pause</span></li>
        <li><kbd>←</kbd> <kbd>→</kbd><span>Seek ±10s</span></li>
        <li><kbd>↑</kbd> <kbd>↓</kbd><span>Volume ±10</span></li>
        <li><kbd>,</kbd><span>Back one line</span></li>
        <li><kbd>.</kbd><span>Next line</span></li>
        <li><kbd>R</kbd><span>Say the line again</span></li>
        <li><kbd>L</kbd><span>Loop this line</span></li>
        <li><kbd>[</kbd> <kbd>]</kbd><span>Slower or faster</span></li>
        <li><kbd>F</kbd><span>Fullscreen</span></li>
        <li><kbd>?</kbd><span>Show or hide this list</span></li>
      </ul>
    </div>
  {/if}
</main>

<style>
  /* Study-mode player: video + dock on the left, live transcript on the
     right, the word sheet rising over the transcript. Colours come from
     theme.css (--color-*, --stage-*, and the shared .menu/.linkbar/
     .segmented/.check/.kbd classes); only the video card is always dark. */

  .page {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }

  /* ---- start screen (no video loaded yet) ---- */

  .start {
    width: 100%;
    max-width: 880px;
    margin: 0 auto;
    padding: 64px var(--space-8) 48px;
    display: flex;
    flex-direction: column;
    gap: var(--space-10);
  }

  .start-hero {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: var(--space-2);
  }

  .start-hero h1 {
    margin: 0;
    font: italic 500 44px/50px var(--font-serif);
    letter-spacing: -0.02em;
  }

  .start-hero p {
    margin: 0;
    max-width: 46ch;
    color: var(--color-text-muted);
    font: 400 16px/24px var(--font-ui);
  }

  .dropzone {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-4);
    min-height: 280px;
    padding: var(--space-10) var(--space-6);
    background: var(--color-surface);
    border: 2px dashed var(--color-border-strong);
    border-radius: var(--radius-lg);
    text-align: center;
  }

  .dropzone.dragging {
    background: var(--color-surface-hover);
  }

  .drop-icon {
    display: inline-grid;
    place-items: center;
    width: 56px;
    height: 56px;
    border-radius: 50%;
    background: var(--color-surface-hover);
    color: var(--color-text-muted);
  }

  .drop-title {
    margin: 0;
    font: 500 20px/28px var(--font-serif);
  }

  .or-divider {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    max-width: 480px;
    color: var(--color-text-faint);
    font: 400 13px/18px var(--font-ui);
  }

  .or-divider span {
    flex: 1;
    height: 1px;
    background: var(--color-border);
  }

  .start-recents {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .section-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-4);
  }

  .section-head h2 {
    margin: 0;
    font: 500 22px/28px var(--font-serif);
  }

  .section-link {
    font: 600 14px/20px var(--font-ui);
    color: var(--color-accent);
    text-decoration: none;
  }

  .section-link:hover {
    color: var(--color-text);
  }

  .recents-row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  /* ---- waiting + notices ---- */

  /* ---- YouTube download progress ---- */

  .download-panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    width: 100%;
    max-width: 560px;
    margin: 0 auto;
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
  }

  .dl-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-3);
    font: 400 14px/20px var(--font-ui);
  }

  .dl-head strong {
    font-weight: 600;
  }

  .dl-quality {
    color: var(--color-text-muted);
    font: 500 12px/16px var(--font-mono);
  }

  .dl-bar {
    position: relative;
    height: 8px;
    border-radius: var(--radius-pill);
    background: var(--color-surface-hover);
    overflow: hidden;
  }

  .dl-bar span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--color-accent);
    transition: width 160ms linear;
  }

  /* No real percentage yet (resolving) or no longer meaningful (merging):
     a sliding block instead of a bar stuck at 0% or 100%. */
  .dl-bar.indeterminate span {
    width: 35%;
    transition: none;
    animation: dl-slide 1.2s ease-in-out infinite;
  }

  @keyframes dl-slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(300%);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .dl-bar.indeterminate span {
      animation: none;
      width: 100%;
      opacity: 0.5;
    }
  }

  .dl-meta {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-3);
    color: var(--color-text-muted);
    font: 400 12px/16px var(--font-mono);
  }

  .dl-percent {
    color: var(--color-text);
    font-weight: 600;
  }

  .quality-select {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text-muted);
    font: 400 13px/20px var(--font-ui);
  }

  .quality-select select {
    padding: 4px var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    color: var(--color-text);
    font: 500 13px/20px var(--font-ui);
  }

  .working {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text-muted);
    font: italic 400 15px/22px var(--font-serif);
  }

  .dots {
    display: inline-flex;
    gap: 3px;
  }

  .dots i {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--color-accent);
    animation: breathe 1.2s var(--ease-out) infinite;
  }

  .dots i:nth-child(2) {
    animation-delay: 0.15s;
  }

  .dots i:nth-child(3) {
    animation-delay: 0.3s;
  }

  @keyframes breathe {
    0%,
    100% {
      opacity: 0.25;
      transform: translateY(0);
    }
    40% {
      opacity: 1;
      transform: translateY(-2px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .dots i {
      animation: none;
      opacity: 0.7;
    }
  }

  .notice {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
    max-width: 640px;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface);
    color: var(--color-text);
    font: 400 14px/20px var(--font-ui);
  }

  .notice b {
    font-weight: 600;
  }

  .notice-action {
    flex: none;
    margin-left: auto;
    align-self: center;
    white-space: nowrap;
  }

  .notice-icon {
    flex: none;
    margin-top: 1px;
  }

  .notice-error {
    border-color: var(--color-danger);
  }

  .notice-error .notice-icon {
    color: var(--color-danger);
  }

  .notice-info .notice-icon {
    color: var(--color-info);
  }

  /* ---- layout: video column + transcript rail (flex-wrap like the
     design, so the rail drops below the video on narrow windows) ---- */

  .player-screen {
    flex: 1;
    width: 100%;
    max-width: 1680px;
    margin: 0 auto;
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: var(--space-6);
    padding: var(--space-6) var(--space-8) var(--space-8);
  }

  .player-main {
    flex: 999 1 640px;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }

  /* ---- the player card: always dark ---- */

  .player-card {
    --player-accent: var(--stage-accent);
    --player-success: #93c08a;
    --player-danger: #f0907f;
    --player-bg: var(--stage);
    --player-surface: var(--stage-raised);
    --player-surface-hover: color-mix(in srgb, var(--stage-ink) 10%, transparent);
    --player-border: color-mix(in srgb, var(--stage-ink) 12%, transparent);
    --player-text: var(--stage-ink);
    --player-text-muted: var(--stage-muted);

    position: relative;
    background: var(--stage);
    color: var(--stage-ink);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    overflow: hidden;
  }

  .player-card.cursor-hidden {
    cursor: none;
  }

  .video-stage {
    position: relative;
    width: 100%;
    aspect-ratio: 16 / 9;
    background:
      radial-gradient(60% 70% at 66% 40%, #5a4532 0%, rgba(90, 69, 50, 0) 70%),
      radial-gradient(40% 55% at 30% 62%, #3a3026 0%, rgba(58, 48, 38, 0) 70%),
      #1d1914;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .video-stage canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: block;
    /* Letterboxes instead of stretching when the video's shape doesn't
       match the 16:9 stage. */
    object-fit: contain;
  }

  .youtube-embed-frame {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    border: none;
  }

  .fs-title {
    position: absolute;
    top: 24px;
    left: 28px;
    z-index: 6;
    display: inline-flex;
    align-items: center;
    height: 32px;
    padding: 0 14px;
    border-radius: var(--radius-pill);
    background: rgba(13, 11, 9, 0.6);
    color: var(--stage-ink);
    font: 600 13px/18px var(--font-ui);
  }

  .video-chip {
    position: absolute;
    top: var(--space-4);
    left: var(--space-4);
    z-index: 6;
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    height: 28px;
    padding: 0 var(--space-3);
    border-radius: var(--radius-pill);
    background: rgba(13, 11, 9, 0.6);
    color: var(--stage-ink);
    font: 600 12px/16px var(--font-ui);
  }

  .video-chip i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--stage-accent);
  }

  .drop-overlay {
    position: absolute;
    inset: 0;
    z-index: 20;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    background: rgba(13, 11, 9, 0.6);
    color: var(--stage-ink);
    font-weight: 600;
    border: 3px dashed var(--stage-accent);
    border-radius: var(--radius-lg);
    pointer-events: none;
  }

  .dropzone .drop-overlay {
    border: 0;
    border-radius: var(--radius-lg);
    background: color-mix(in srgb, var(--color-surface) 82%, transparent);
    color: var(--color-text);
  }

  /* ---- subtitles on the video ---- */

  .subtitle-overlay {
    position: absolute;
    bottom: 6%;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 0 0.28em;
    max-width: 90%;
    padding: 0.3em 0.6em;
    background: rgba(var(--subtitle-bg-rgb, 13, 11, 9), var(--subtitle-bg-alpha, 0.62));
    backdrop-filter: blur(calc(var(--subtitle-bg-alpha, 0.62) * 10px));
    border-radius: var(--radius-md);
    font-size: var(--sub-size, 1.5rem);
    z-index: 5;
    transition: bottom var(--dur-med) var(--ease-out);
  }

  .subtitle-overlay.pos-top {
    bottom: auto;
    top: 6%;
  }

  .subtitle-overlay.pos-middle {
    bottom: auto;
    top: 50%;
    transform: translate(-50%, -50%);
  }

  /* Only in fullscreen, where the dock floats over the picture as a pill. */
  .subtitle-overlay.controls-visible {
    bottom: 150px;
  }

  .subtitle-word {
    color: var(--subtitle-color, var(--stage-ink));
    font-size: inherit;
    font-family: inherit;
    font-weight: var(--subtitle-weight, 500);
    line-height: 1.35;
    cursor: pointer;
    border-radius: var(--radius-sm);
    padding: 0 0.08em;
    text-shadow: var(--subtitle-outline, none);
    transition: background var(--dur-fast) var(--ease-out);
  }

  .subtitle-word:hover {
    background: color-mix(in srgb, currentColor 20%, transparent);
  }

  .subtitle-word.is-open {
    background: var(--stage-ink);
    color: var(--stage);
    text-shadow: none;
  }

  .subtitle-word.is-saved {
    box-shadow: inset 0 -0.16em 0 var(--stage-marker);
  }

  .subtitle-word:focus-visible {
    outline: 2px solid var(--stage-accent);
    outline-offset: 1px;
  }

  /* ---- dock (the control bar, in the same dark card) ---- */

  .dock {
    padding: var(--space-2) var(--space-5) var(--space-4);
    border-top: 1px solid var(--stage-raised);
    color: var(--stage-ink);
  }

  .seek-wrap {
    position: relative;
    height: 22px;
    display: flex;
    align-items: center;
  }

  .seek {
    -webkit-appearance: none;
    appearance: none;
    position: relative;
    z-index: 2;
    width: 100%;
    height: 22px;
    margin: 0;
    background: transparent;
    cursor: pointer;
  }

  .seek::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: var(--radius-pill);
    background: linear-gradient(
      to right,
      var(--stage-accent) var(--progress, 0%),
      var(--stage-raised) var(--progress, 0%)
    );
  }

  .seek::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 14px;
    height: 14px;
    margin-top: -5px;
    border-radius: 50%;
    background: var(--stage-ink);
    box-shadow: 0 0 0 4px rgba(227, 141, 95, 0.35);
  }

  .seek:disabled {
    cursor: default;
    opacity: 0.6;
  }

  .seek-markers {
    position: absolute;
    left: 7px;
    right: 7px;
    top: 50%;
    height: 8px;
    transform: translateY(-50%);
    pointer-events: none;
    z-index: 1;
  }

  .seek-marker {
    position: absolute;
    top: 0;
    width: 2px;
    height: 8px;
    margin-left: -1px;
    border-radius: 1px;
    background: var(--stage-muted);
    opacity: 0.55;
  }

  .seek-line {
    position: absolute;
    top: -2px;
    height: 8px;
    border-radius: var(--radius-pill);
    background: rgba(227, 141, 95, 0.28);
  }

  .seek-times {
    display: flex;
    justify-content: space-between;
    margin-top: 2px;
    color: var(--stage-muted);
    font: 500 12px/16px var(--font-mono);
  }

  .seek-times b {
    color: var(--stage-ink);
    font-weight: 500;
  }

  .dock-row {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-2);
  }

  .dock-left,
  .dock-right {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .dock-right {
    justify-content: flex-end;
  }

  .transport {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .play-lg {
    width: 52px;
    height: 52px;
    background: var(--stage-ink);
    color: var(--stage);
  }

  .play-lg:hover:not(:disabled) {
    background: var(--stage-ink);
    transform: scale(1.04);
  }

  .play-lg :global(svg) {
    width: 22px;
    height: 22px;
  }

  .line-btn {
    display: inline-flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    min-width: 56px;
    padding: 6px 8px 4px;
    border: 0;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--stage-ink);
    font: 500 11px/14px var(--font-ui);
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out);
  }

  .line-btn span {
    color: var(--stage-muted);
  }

  .line-btn :global(svg) {
    width: 15px;
    height: 15px;
  }

  .line-btn:hover:not(:disabled) {
    background: var(--stage-raised);
  }

  .line-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .line-btn.is-active {
    background: var(--stage-raised);
    color: var(--stage-accent);
  }

  .line-btn.is-active span {
    color: var(--stage-accent);
  }

  .chip-btn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--stage-raised);
    border-radius: var(--radius-pill);
    background: transparent;
    color: var(--stage-ink);
    font: 500 12px/16px var(--font-mono);
    cursor: pointer;
  }

  .chip-btn :global(svg) {
    width: 15px;
    height: 15px;
  }

  .chip-btn:hover:not(:disabled) {
    background: var(--stage-raised);
  }

  .chip-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .volume {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding-right: var(--space-2);
  }

  .volume-track {
    -webkit-appearance: none;
    appearance: none;
    width: 72px;
    height: 16px;
    margin: 0;
    background: transparent;
    cursor: pointer;
  }

  .volume-track::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: var(--radius-pill);
    background: linear-gradient(to right, var(--stage-ink) var(--progress, 0%), var(--stage-raised) var(--progress, 0%));
  }

  .volume-track::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 12px;
    height: 12px;
    margin-top: -4px;
    border-radius: 50%;
    background: var(--stage-ink);
  }

  /* ---- fullscreen = cinema: floating control pill + floating word card ---- */

  .player-card:fullscreen {
    width: 100vw;
    height: 100vh;
    max-width: none;
    border-radius: 0;
    display: flex;
    flex-direction: column;
    background:
      radial-gradient(55% 65% at 68% 36%, #5a4532 0%, rgba(90, 69, 50, 0) 70%),
      radial-gradient(38% 52% at 26% 60%, #3a3026 0%, rgba(58, 48, 38, 0) 70%),
      #1d1914;
  }

  .player-card:fullscreen .video-stage {
    flex: 1;
    min-height: 0;
    aspect-ratio: auto;
    background: transparent;
  }

  /* Fullscreen subtitles read larger (design: 34px vs 26px windowed). */
  .player-card:fullscreen .subtitle-overlay {
    max-width: 70%;
    font-size: calc(var(--sub-size, 1.625rem) * 1.3);
  }

  .player-card:fullscreen .dock {
    position: absolute;
    left: 50%;
    bottom: 28px;
    transform: translateX(-50%);
    z-index: 15;
    width: min(980px, calc(100% - 40px));
    padding: var(--space-2) var(--space-5) 14px;
    border-top: 0;
    border-radius: var(--radius-lg);
    background: rgba(21, 19, 15, 0.88);
    box-shadow: 0 24px 48px rgba(0, 0, 0, 0.4);
    transition: opacity var(--dur-med) var(--ease-out);
  }

  .dock.hidden {
    opacity: 0;
    pointer-events: none;
  }

  /* ---- what you're watching ---- */

  .now {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
  }

  .now-text {
    min-width: 0;
  }

  .now-title {
    margin: 0;
    font: 500 26px/32px var(--font-serif);
    letter-spacing: -0.01em;
    overflow-wrap: anywhere;
  }

  .now-meta {
    margin: var(--space-1) 0 0;
    color: var(--color-text-muted);
    font: 400 14px/20px var(--font-ui);
  }

  .now-tools {
    flex: none;
    padding-top: var(--space-1);
  }

  .saved-strip {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }

  .strip-label {
    margin-right: var(--space-1);
    color: var(--color-text-muted);
    font: 600 12px/16px var(--font-ui);
    letter-spacing: 0.02em;
  }

  /* ---- transcript rail ---- */

  .rail {
    flex: 1 1 360px;
    max-width: 420px;
    position: relative;
    display: flex;
    flex-direction: column;
    height: 640px;
    max-height: calc(100vh - 8rem);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }

  .rail-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
  }

  .rail-body {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-2) var(--space-2) var(--space-10);
    scrollbar-width: thin;
  }

  .rail-empty {
    margin: var(--space-6) var(--space-4);
    color: var(--color-text-muted);
    font: italic 400 15px/22px var(--font-serif);
  }

  .cue {
    display: grid;
    grid-template-columns: 48px 1fr;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-md);
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out);
  }

  .cue:hover {
    background: var(--color-surface-hover);
  }

  .cue-time {
    padding-top: 3px;
    color: var(--color-text-faint);
    font: 500 12px/18px var(--font-mono);
  }

  .cue-text {
    color: var(--color-text);
    font: 400 17px/26px var(--font-serif);
  }

  .cue.is-past .cue-text {
    color: var(--color-text-muted);
  }

  .cue.is-now {
    background: var(--color-bg);
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
  }

  .cue.is-now .cue-time {
    color: var(--color-accent);
  }

  .cue.is-now .cue-text {
    font-weight: 500;
  }

  .cue-progress {
    grid-column: 2;
    height: 2px;
    margin-top: var(--space-1);
    border-radius: var(--radius-pill);
    background: var(--color-border);
    overflow: hidden;
  }

  .cue-progress span {
    display: block;
    height: 100%;
    background: var(--color-accent);
  }

  .cue-word {
    border-radius: 4px;
    padding: 0 1px;
  }

  .cue-word:hover {
    background: var(--color-surface-hover);
  }

  .cue-word.is-open {
    background: var(--color-text);
    color: var(--color-bg);
  }

  .cue-word.marked {
    padding: 0 0.12em;
    border-radius: 3px 9px 4px 8px;
    background: var(--color-marker);
    color: var(--color-text);
  }

  .saved-word {
    display: block;
    font: 500 20px/26px var(--font-serif);
  }

  .saved-context {
    display: block;
    color: var(--color-text-muted);
    font: italic 400 15px/22px var(--font-serif);
  }

  .speed-trigger {
    display: none;
  }

  .sheet-scrim {
    display: none;
  }

  .back-now {
    position: sticky;
    bottom: var(--space-4);
    display: flex;
    margin: var(--space-3) auto 0;
    box-shadow: var(--shadow-md);
    background: var(--color-surface);
  }

  .rail-foot {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    padding: 10px var(--space-4);
    border-top: 1px solid var(--color-border);
    color: var(--color-text-muted);
    font: 400 12px/16px var(--font-ui);
  }

  /* ---- word sheet (docked over the transcript) ---- */

  .sheet {
    --player-bg: var(--color-surface);
    --player-surface: var(--color-surface);
    --player-surface-hover: var(--color-surface-hover);
    --player-border: var(--color-border);
    --player-text: var(--color-text);
    --player-text-muted: var(--color-text-muted);

    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 5;
    max-height: 78%;
    overflow-y: auto;
    padding: var(--space-2) var(--space-5) var(--space-5);
    background: var(--color-surface);
    color: var(--color-text);
    border-top: 1px solid var(--color-border);
    border-radius: var(--radius-lg) var(--radius-lg) 0 0;
    box-shadow: 0 -12px 32px rgba(40, 25, 10, 0.12);
    animation: sheet-in var(--dur-med) var(--ease-out);
  }

  @keyframes sheet-in {
    from {
      transform: translateY(24px);
      opacity: 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .sheet {
      animation: none;
    }
  }

  .sheet-grip {
    width: 36px;
    height: 4px;
    margin: 0 auto var(--space-3);
    border-radius: var(--radius-pill);
    background: var(--color-border-strong);
  }

  /* ---- word card content (shared by the sheet and the fullscreen popup) ---- */

  .word-popup {
    position: absolute;
    transform: translate(-50%, -100%);
    margin-top: -0.9rem;
    width: 360px;
    max-width: calc(100% - 1rem);
    max-height: calc(100% - 1rem);
    overflow-y: auto;
    background: var(--color-surface);
    color: var(--color-text);
    border-radius: var(--radius-lg);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3), 0 24px 48px rgba(0, 0, 0, 0.45);
    padding: var(--space-5);
    text-align: left;
    z-index: 30;
    /* The sheet look reuses the light palette even though it floats inside
       the dark fullscreen stage. */
    --player-bg: var(--color-surface);
    --player-surface: var(--color-surface);
    --player-surface-hover: var(--color-surface-hover);
    --player-border: var(--color-border);
    --player-text: var(--color-text);
    --player-text-muted: var(--color-text-muted);
    --player-accent: var(--color-accent);
  }

  .popup-tail {
    position: absolute;
    left: 50%;
    margin-left: -8px;
    bottom: -8px;
    width: 16px;
    height: 16px;
    background: var(--color-surface);
    transform: rotate(45deg);
    border-radius: 3px;
  }

  .word-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .headword {
    margin: 0;
    font: 500 30px/34px var(--font-serif);
    letter-spacing: -0.01em;
    overflow-wrap: anywhere;
  }

  .sheet .headword {
    font-size: 28px;
    line-height: 32px;
  }

  .phonetic {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin: var(--space-1) 0 0;
    color: var(--player-text-muted);
    font: 400 15px/22px var(--font-serif);
  }

  .phonetic .icon-btn {
    color: var(--player-accent, var(--color-accent));
  }

  .icon-btn-sm {
    width: 30px;
    height: 30px;
  }

  .pos {
    margin: var(--space-4) 0 var(--space-1);
    color: var(--player-text-muted);
    font: italic 400 15px/22px var(--font-serif);
  }

  .defs {
    margin: 0;
    padding-left: 1.2em;
    font: 400 17px/26px var(--font-serif);
  }

  .sheet .defs {
    font-size: 16px;
    line-height: 24px;
  }

  .defs li + li {
    margin-top: var(--space-1);
  }

  .defs li::marker {
    color: var(--color-text-faint);
  }

  .popup-definition {
    margin: var(--space-1) 0;
    font: 400 17px/26px var(--font-serif);
  }

  .popup-example {
    margin: var(--space-1) 0;
    color: var(--player-text-muted);
    font: italic 400 16px/24px var(--font-serif);
  }

  .popup-status.error {
    color: var(--player-danger, var(--color-danger));
  }

  .popup-actions {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }

  .popup-actions .btn {
    flex: 1;
    min-height: 44px;
  }

  .popup-actions .btn-outline:disabled {
    color: var(--player-success, var(--color-success));
    border-color: var(--player-success, var(--color-success));
    opacity: 1;
  }

  .ai-explain .popup-definition {
    font-size: 16px;
    line-height: 24px;
  }

  /* ---- narrow windows: rail drops under the video ---- */

  @media (max-width: 1080px) {
    .rail {
      flex: 1 1 100%;
      max-width: none;
      height: 520px;
    }
  }

  /* ---- phones: full-bleed video, back-arrow header, compact dock,
     bottom-sheet word card over a scrim (per the phone screens) ---- */

  @media (max-width: 640px) {
    .start {
      padding: var(--space-8) var(--space-4) var(--space-10);
    }

    .start-hero h1 {
      font-size: 34px;
      line-height: 40px;
    }

    .player-screen {
      padding: 0;
      gap: 0;
    }

    .player-main {
      flex: 1 1 100%;
      gap: 0;
    }

    .player-card {
      border-radius: 0;
      box-shadow: none;
    }

    .dock {
      padding: 4px var(--space-3) 10px;
    }

    .dock-left,
    .dock-right {
      display: none;
    }

    .dock-row {
      grid-template-columns: 1fr;
      justify-items: center;
      margin-top: 0;
    }

    .transport {
      justify-content: center;
    }

    .seek-times {
      display: none;
    }

    .subtitle-overlay {
      max-width: 92%;
      padding: 0.25em 0.5em;
      border-radius: 8px;
      font-size: min(var(--sub-size, 1.5rem), 1rem);
    }

    /* Phone transport: Back / Again / Play / Next / Speed chip. */
    .line-btn-loop {
      display: none;
    }

    .speed-trigger {
      display: block;
    }

    .speed-btn {
      min-width: 56px;
    }

    .speed-btn .rate {
      font: 500 13px/16px var(--font-mono);
    }

    .now {
      padding: var(--space-4) var(--space-4) 0;
    }

    .saved-strip {
      padding: 0 var(--space-4);
    }

    .rail {
      flex: 1 1 auto;
      max-width: none;
      height: auto;
      min-height: 320px;
      border: 0;
      border-top: 1px solid var(--color-border);
      border-radius: 0;
    }

    .sheet {
      position: fixed;
      z-index: 50;
      max-height: 75vh;
      padding-bottom: 28px;
      box-shadow: 0 -12px 32px rgba(40, 25, 10, 0.16);
    }

    .sheet-scrim {
      display: block;
      position: fixed;
      inset: 0;
      z-index: 40;
      border: 0;
      padding: 0;
      background: rgba(22, 19, 15, 0.28);
      cursor: default;
    }
  }

  .shortcuts-list {
    display: flex;
    flex-direction: column;
    gap: 0.55rem;
    list-style: none;
    padding: 0;
    margin: 0.4rem 0 0;
  }

  .shortcuts-list li {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .shortcuts-list li span {
    margin-left: auto;
    color: var(--color-text-muted);
    font-size: 0.9rem;
  }

  .shortcuts-list kbd {
    min-width: 1.6rem;
    padding: 0.15rem 0.45rem;
    border: 1px solid var(--color-border-strong);
    border-radius: 5px;
    background: var(--color-surface-hover);
    font-family: var(--font-mono);
    font-size: 0.78rem;
    text-align: center;
  }

  .sync-offset-row {
    display: flex;
    gap: 0.35rem;
    padding: 0.2rem 0.6em 0.5em;
  }

  .btn-stage {
    background: var(--stage);
    color: var(--stage-ink);
  }

  /* ---- AI setup modal ---- */
  /* .modal-panel/.modal-header are centralized in theme.css */

  .modal-intro {
    color: var(--color-text-muted);
    font-size: 0.88rem;
    line-height: 1.45;
    margin: 0 0 1rem;
  }

  .setup-section {
    padding: 0.9rem 0;
    border-top: 1px solid var(--color-border);
  }

  .setup-section h3 {
    margin: 0 0 0.6rem;
    font-size: 0.95rem;
  }

  .setup-section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.6rem;
  }

  .setup-section-header h3 {
    margin: 0;
  }

  .status-pill {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.78rem;
    font-weight: 600;
    padding: 0.25em 0.7em;
    border-radius: 999px;
    background: color-mix(in srgb, var(--color-text-muted) 14%, transparent);
    color: var(--color-text-muted);
  }

  .status-pill.ok {
    background: color-mix(in srgb, #22c55e 18%, transparent);
    color: #16a34a;
  }

  .setup-progress {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    margin-top: 0.6rem;
  }

  .setup-progress.inline {
    margin-top: 0;
    min-width: 8rem;
  }

  .setup-progress-bar {
    flex: 1;
    height: 5px;
    border-radius: 999px;
    background: var(--color-border);
    overflow: hidden;
  }

  .setup-progress-bar span {
    display: block;
    height: 100%;
    background: var(--color-accent);
    transition: width 0.2s ease;
  }

  .setup-progress-label {
    font-size: 0.78rem;
    color: var(--color-text-muted);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .model-catalog {
    list-style: none;
    margin: 0 0 0.9rem;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.55rem;
  }

  .model-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.8rem;
  }

  .model-info {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }

  .model-info strong {
    font-size: 0.92rem;
  }

  .model-desc {
    font-size: 0.78rem;
    color: var(--color-text-muted);
  }

  .custom-model-row {
    display: flex;
    gap: 0.5rem;
  }

  .custom-model-row input {
    flex: 1;
    min-width: 0;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: 0.5em 0.8em;
    font-size: 0.88rem;
    font-family: inherit;
    color: var(--color-text);
    background: var(--color-bg);
  }

  .active-model-picker {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.6rem;
    margin-top: 0.9rem;
    font-size: 0.85rem;
    color: var(--color-text-muted);
  }

  .active-model-picker select {
    font-family: inherit;
    font-size: 0.85rem;
    padding: 0.35em 0.6em;
    border-radius: 8px;
    border: 1px solid var(--color-border);
    background: var(--color-surface);
    color: var(--color-text);
  }
</style>
