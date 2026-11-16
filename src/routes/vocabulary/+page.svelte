<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { player } from "$lib/player.svelte";
  import AppHeader from "$lib/AppHeader.svelte";

  type VocabularyEntry = {
    id: number;
    word: string;
    definition: string | null;
    context_sentence: string | null;
    source_video: string | null;
    timestamp_ms: number | null;
    date_added: string;
    ease_factor: number;
    interval_days: number;
    next_review_date: string | null;
    review_count: number;
  };

  type SortMode = "newest" | "oldest" | "alpha";

  let entries = $state<VocabularyEntry[]>([]);
  let search = $state("");
  let sortMode = $state<SortMode>("newest");
  let errorMessage = $state("");
  let loading = $state(true);
  let dueCount = $state(0);
  let wordsCount = $state(0);

  // ---- Spaced-repetition review session ----

  type ReviewAiState = {
    aiLoading: boolean;
    aiText: string;
    aiError: string;
    examplesLoading: boolean;
    examplesText: string;
    examplesError: string;
  };

  function freshReviewAiState(): ReviewAiState {
    return { aiLoading: false, aiText: "", aiError: "", examplesLoading: false, examplesText: "", examplesError: "" };
  }

  let reviewQueue = $state<VocabularyEntry[]>([]);
  let reviewIdx = $state(0);
  let reviewOpen = $state(false);
  let revealed = $state(false);
  let reviewAi = $state<ReviewAiState>(freshReviewAiState());
  let sessionReviewed = $state(0);
  let reviewDone = $state(false);

  const currentCard = $derived<VocabularyEntry | null>(reviewOpen ? (reviewQueue[reviewIdx] ?? null) : null);

  // A word graduates to "mastered" once its SM-2 interval has grown past
  // three weeks — by then it's being reviewed rarely enough that it's
  // functionally learned, not just "not new anymore".
  const MASTERED_INTERVAL_DAYS = 21;
  const stats = $derived.by(() => {
    const total = entries.length;
    const newCount = entries.filter((e) => e.review_count === 0).length;
    const mastered = entries.filter((e) => e.interval_days >= MASTERED_INTERVAL_DAYS).length;
    return { total, newCount, mastered, learning: Math.max(0, total - newCount - mastered) };
  });

  async function refreshDueCount() {
    try {
      dueCount = (await invoke<VocabularyEntry[]>("list_due_vocabulary")).length;
    } catch {
      // leave last-known count on failure
    }
  }

  async function openReview() {
    errorMessage = "";
    try {
      const due = await invoke<VocabularyEntry[]>("list_due_vocabulary");
      if (due.length === 0) {
        errorMessage = "Nothing due for review right now.";
        return;
      }
      reviewQueue = shuffle(due);
      reviewIdx = 0;
      revealed = false;
      reviewAi = freshReviewAiState();
      sessionReviewed = 0;
      reviewDone = false;
      reviewOpen = true;
    } catch (err) {
      errorMessage = String(err);
    }
  }

  function closeReview() {
    reviewOpen = false;
  }

  function shuffle<T>(list: T[]): T[] {
    const copy = [...list];
    for (let i = copy.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [copy[i], copy[j]] = [copy[j], copy[i]];
    }
    return copy;
  }

  function reveal() {
    revealed = true;
  }

  function advanceReview() {
    reviewIdx++;
    revealed = false;
    reviewAi = freshReviewAiState();
    if (reviewIdx >= reviewQueue.length) {
      reviewDone = true;
      refreshDueCount();
    }
  }

  async function rate(rating: 1 | 3 | 5) {
    const card = currentCard;
    if (!card) return;
    try {
      const updated = await invoke<VocabularyEntry>("review_vocabulary_word", { id: card.id, rating });
      entries = entries.map((e) => (e.id === updated.id ? updated : e));
      sessionReviewed++;
      // A lapse (Again) comes back in 60s, same session — reinsert it a
      // few cards ahead rather than dropping it from the queue entirely.
      if (rating < 3) {
        const rest = reviewQueue.slice(reviewIdx + 1);
        const reinsertAt = Math.min(3, rest.length);
        reviewQueue = [
          ...reviewQueue.slice(0, reviewIdx + 1),
          ...rest.slice(0, reinsertAt),
          card,
          ...rest.slice(reinsertAt),
        ];
      }
    } catch (err) {
      errorMessage = String(err);
    }
    advanceReview();
  }

  function onReviewKey(e: KeyboardEvent) {
    if (!reviewOpen) return;
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA")) return;
    if (e.key === "Escape") { closeReview(); return; }
    if (reviewDone) return;
    if (!revealed) {
      if (e.key === " " || e.key === "Enter") { e.preventDefault(); reveal(); }
      return;
    }
    if (e.key === "1") void rate(1);
    else if (e.key === "2") void rate(3);
    else if (e.key === "3") void rate(5);
  }

  function skipCard() {
    advanceReview();
  }

  async function removeCurrentCard() {
    const card = currentCard;
    if (!card) return;
    try {
      await invoke("delete_vocabulary_word", { id: card.id });
      entries = entries.filter((e) => e.id !== card.id);
    } catch (err) {
      errorMessage = String(err);
    }
    advanceReview();
  }

  /// Mirrors the Rust `sm2_update` interval math (src-tauri/src/vocabulary.rs)
  /// for display only — the backend command is the source of truth for what
  /// actually gets persisted when a rating button is clicked.
  function previewDays(card: VocabularyEntry, rating: 1 | 3 | 5): string {
    if (rating < 3) return "later today";
    let interval: number;
    if (card.review_count === 0) interval = 1;
    else if (card.review_count === 1) interval = rating >= 5 ? 4 : 3;
    else interval = Math.max(1, Math.round(card.interval_days * card.ease_factor));
    return interval === 1 ? "in 1 day" : `in ${interval} days`;
  }

  function escapeHtml(s: string): string {
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }

  /// Bolds the reviewed word where it appears in its saved context sentence.
  /// The sentence comes from subtitle/caption text (external, untrusted), so
  /// it's HTML-escaped *before* the `<strong>` wrapper is added — the only
  /// real markup in the result is ours.
  function highlightedContext(card: VocabularyEntry): string {
    const sentence = card.context_sentence ?? "";
    const escaped = escapeHtml(sentence);
    if (!card.word) return escaped;
    const escapedWord = escapeHtml(card.word).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    return escaped.replace(new RegExp(`(${escapedWord})`, "i"), '<span class="marked">$1</span>');
  }

  async function explainCard() {
    const card = currentCard;
    const model = player.currentLlmModel();
    if (!card || !model || reviewAi.aiLoading) return;
    reviewAi = { ...reviewAi, aiLoading: true, aiError: "", aiText: "" };
    try {
      const text = await invoke<string>("llm_explain", {
        word: card.word,
        sentence: card.context_sentence ?? "",
        modelPath: model.path,
      });
      reviewAi = { ...reviewAi, aiLoading: false, aiText: text };
    } catch (err) {
      reviewAi = { ...reviewAi, aiLoading: false, aiError: String(err) };
    }
  }

  async function examplesCard() {
    const card = currentCard;
    const model = player.currentLlmModel();
    if (!card || !model || reviewAi.examplesLoading) return;
    reviewAi = { ...reviewAi, examplesLoading: true, examplesError: "", examplesText: "" };
    try {
      const text = await invoke<string>("llm_word_examples", {
        word: card.word,
        sentence: card.context_sentence ?? "",
        modelPath: model.path,
      });
      reviewAi = { ...reviewAi, examplesLoading: false, examplesText: text };
    } catch (err) {
      reviewAi = { ...reviewAi, examplesLoading: false, examplesError: String(err) };
    }
  }

  function baseName(path: string | null): string {
    if (!path) return "";
    return path.split(/[\\/]/).pop() ?? path;
  }

  function formatDate(iso: string): string {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return iso;
    const diffDays = Math.floor((Date.now() - d.getTime()) / 86_400_000);
    if (diffDays < 1) return "Saved today";
    if (diffDays === 1) return "Saved yesterday";
    if (diffDays < 7) return `Saved ${diffDays} days ago`;
    const sameYear = d.getFullYear() === new Date().getFullYear();
    return "Saved " + d.toLocaleDateString(undefined, sameYear ? { day: "numeric", month: "short" } : { day: "numeric", month: "short", year: "numeric" });
  }

  function isDue(entry: VocabularyEntry): boolean {
    if (!entry.next_review_date) return false;
    const t = new Date(entry.next_review_date).getTime();
    return !Number.isNaN(t) && t <= Date.now();
  }

  function formatTimestamp(ms: number | null): string {
    if (ms == null) return "";
    const total = Math.floor(ms / 1000);
    const h = Math.floor(total / 3600);
    const m = Math.floor((total % 3600) / 60);
    const sec = String(total % 60).padStart(2, "0");
    return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
  }

  const visibleEntries = $derived.by(() => {
    const query = search.trim().toLowerCase();
    let list = query ? entries.filter((e) => e.word.toLowerCase().includes(query)) : entries;
    list = [...list];
    if (sortMode === "alpha") {
      list.sort((a, b) => a.word.localeCompare(b.word));
    } else if (sortMode === "oldest") {
      list.sort((a, b) => a.id - b.id);
    } else {
      list.sort((a, b) => b.id - a.id);
    }
    return list;
  });

  async function loadEntries() {
    loading = true;
    try {
      entries = await invoke<VocabularyEntry[]>("list_vocabulary");
      wordsCount = entries.length;
    } catch (err) {
      errorMessage = String(err);
    } finally {
      loading = false;
    }
  }

  async function removeEntry(id: number) {
    try {
      await invoke("delete_vocabulary_word", { id });
      entries = entries.filter((e) => e.id !== id);
    } catch (err) {
      errorMessage = String(err);
    }
  }

  onMount(() => {
    loadEntries();
    refreshDueCount();
    player.refreshLlmStatus();
  });
</script>

{#snippet iconSearch()}
  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
    ><circle cx="11" cy="11" r="7" /><path d="M21 21l-4.3-4.3" /></svg
  >
{/snippet}

{#snippet iconTrash()}
  <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13" />
  </svg>
{/snippet}

{#snippet iconBook()}
  <svg viewBox="0 0 24 24" width="2.2rem" height="2.2rem" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
    <path d="M4 5.5C4 4.7 4.7 4 5.5 4H12v16H5.5c-.8 0-1.5-.7-1.5-1.5v-13z" />
    <path d="M20 5.5c0-.8-.7-1.5-1.5-1.5H12v16h6.5c.8 0 1.5-.7 1.5-1.5v-13z" />
  </svg>
{/snippet}

{#snippet iconRepeat()}
  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <path d="M4 12a8 8 0 0 1 14.5-4.5M20 12a8 8 0 0 1-14.5 4.5" />
    <path d="M18.5 3v4.5H14M5.5 21v-4.5H10" />
  </svg>
{/snippet}

{#snippet iconClose()}
  <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"
    ><path d="M6 6l12 12M18 6L6 18" /></svg
  >
{/snippet}

{#snippet iconPlaySmall()}
  <svg viewBox="0 0 24 24" width="15" height="15" fill="currentColor" aria-hidden="true"><path d="M8 5v14l11-7z" /></svg>
{/snippet}

{#snippet iconSparkle()}
  <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor">
    <path d="M12 3l1.6 4.9L18.5 9.5l-4.9 1.6L12 16l-1.6-4.9L5.5 9.5l4.9-1.6L12 3z" />
    <path d="M19 14l.7 2.1 2.1.7-2.1.7-.7 2.1-.7-2.1-2.1-.7 2.1-.7L19 14z" />
  </svg>
{/snippet}

<main class="page">
  <AppHeader wordsCount={wordsCount}>
    {#snippet actionsExtras()}
      <button class="nav-pill" onclick={openReview} disabled={dueCount === 0}>
        {@render iconRepeat()} Review{#if dueCount > 0} <span class="count">{dueCount}</span>{/if}
      </button>
    {/snippet}
    {#snippet menuExtras()}
      <div class="menu-divider"></div>
      <a class="menu-item" role="menuitem" href="/">Back to the player</a>
    {/snippet}
  </AppHeader>

  <div class="vocab-body">
    <div class="section-head">
      <h1>Your words</h1>
      <span class="section-meta">{entries.length} {entries.length === 1 ? "word" : "words"} saved</span>
    </div>

  {#if entries.length > 0}
    <div class="stats-row" role="group" aria-label="Your progress">
      <div class="stat-tile">
        <span class="stat-value">{stats.total}</span>
        <span class="stat-label">Words saved</span>
      </div>
      <div class="stat-tile" class:stat-tile-attn={dueCount > 0}>
        <span class="stat-value">{dueCount}</span>
        <span class="stat-label">Ready to review</span>
      </div>
      <div class="stat-tile">
        <span class="stat-value">{stats.newCount}</span>
        <span class="stat-label">New</span>
      </div>
      <div class="stat-tile">
        <span class="stat-value">{stats.learning}</span>
        <span class="stat-label">Learning</span>
      </div>
      <div class="stat-tile">
        <span class="stat-value">{stats.mastered}</span>
        <span class="stat-label">Known</span>
      </div>
    </div>
  {/if}

  <div class="toolbar">
    <div class="search-box">
      {@render iconSearch()}
      <input type="search" placeholder="Find a word" aria-label="Find a word" bind:value={search} />
    </div>
    <div class="segmented" role="group" aria-label="Sort words">
      <button aria-pressed={sortMode === "newest"} onclick={() => (sortMode = "newest")}>Newest</button>
      <button aria-pressed={sortMode === "oldest"} onclick={() => (sortMode = "oldest")}>Oldest</button>
      <button aria-pressed={sortMode === "alpha"} onclick={() => (sortMode = "alpha")}>A to Z</button>
    </div>
  </div>

  {#if errorMessage}
    <p class="error-banner">{errorMessage}</p>
  {/if}

  {#if loading}
    <p class="hint">Opening your words…</p>
  {:else if entries.length === 0}
    <div class="empty-state">
      <div class="empty-mark">{@render iconBook()}</div>
      <h3>No words yet</h3>
      <p>While you watch, tap any word in the subtitles and choose "Save word". It'll be waiting here, with the sentence you found it in.</p>
      <a class="nav-pill" href="/">Start watching</a>
    </div>
  {:else if visibleEntries.length === 0}
    <p class="hint">No words match “{search}”.</p>
  {:else}
    <ul class="entry-list">
      {#each visibleEntries as entry (entry.id)}
        <li class="entry-card">
          <div class="entry-header">
            <strong class="entry-word">{entry.word}</strong>
            {#if isDue(entry)}<span class="badge-due">Due</span>{/if}
            <span class="entry-date">{formatDate(entry.date_added)}</span>
            <button class="icon-btn" onclick={() => removeEntry(entry.id)} aria-label="Delete this word" title="Delete this word">
              {@render iconTrash()}
            </button>
          </div>
          {#if entry.definition}
            <p class="entry-definition">{entry.definition}</p>
          {:else}
            <p class="entry-definition entry-definition-empty">No meaning saved for this one yet.</p>
          {/if}
          {#if entry.context_sentence}
            <p class="entry-context">“{@html highlightedContext(entry)}”</p>
          {/if}
          {#if entry.source_video}
            <p class="entry-source">
              {@render iconPlaySmall()}
              {baseName(entry.source_video)}{#if entry.timestamp_ms != null}<span aria-hidden="true">&nbsp;·&nbsp;</span>{formatTimestamp(entry.timestamp_ms)}{/if}
            </p>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  {#if reviewOpen}
    <div class="modal-backdrop" onclick={closeReview} role="presentation"></div>
    <div class="modal-panel review-panel" role="dialog" aria-label="Review your words">
      <div class="review-header">
        <span class="review-progress">{reviewDone ? reviewQueue.length : reviewIdx + 1} of {reviewQueue.length}</span>
        <button class="modal-close" onclick={closeReview} aria-label="Close">{@render iconClose()}</button>
      </div>

      {#if reviewDone}
        <div class="review-card review-finished">
          <strong class="review-word">That’s all {sessionReviewed} — see you tomorrow.</strong>
          <button class="btn btn-primary review-reveal" onclick={closeReview}>Done</button>
        </div>
      {:else if currentCard}
        {@const card = currentCard}
        <div class="review-card">
          <strong class="review-word">{card.word}</strong>
          {#if card.context_sentence}
            <p class="review-context">{@html highlightedContext(card)}</p>
          {/if}

          {#if !revealed}
            <button class="btn btn-primary review-reveal" onclick={reveal}>Show the meaning</button>
          {:else}
            {#if card.definition}
              <p class="review-definition">{card.definition}</p>
            {/if}

            {#if player.isLlmReady()}
              <div class="ai-explain">
                <div class="ai-actions">
                  {#if !reviewAi.aiLoading && !reviewAi.aiText && !reviewAi.aiError}
                    <button class="btn btn-outline ai-btn" onclick={explainCard}>
                      {@render iconSparkle()} Explain this
                    </button>
                  {/if}
                  {#if !reviewAi.examplesLoading && !reviewAi.examplesText && !reviewAi.examplesError}
                    <button class="btn btn-outline ai-btn" onclick={examplesCard}>
                      {@render iconSparkle()} More examples
                    </button>
                  {/if}
                </div>

                {#if reviewAi.aiLoading}
                  <p class="popup-status">Thinking…</p>
                {:else if reviewAi.aiError}
                  <p class="popup-status error">{reviewAi.aiError}</p>
                  <button class="btn btn-outline ai-btn" onclick={explainCard}>Retry</button>
                {:else if reviewAi.aiText}
                  <p class="ai-label">{@render iconSparkle()} In this scene</p>
                  <p class="review-definition">{reviewAi.aiText}</p>
                {/if}

                {#if reviewAi.examplesLoading}
                  <p class="popup-status">Finding examples…</p>
                {:else if reviewAi.examplesError}
                  <p class="popup-status error">{reviewAi.examplesError}</p>
                  <button class="btn btn-outline ai-btn" onclick={examplesCard}>Retry</button>
                {:else if reviewAi.examplesText}
                  <p class="ai-label">{@render iconSparkle()} More examples</p>
                  <p class="review-definition examples-text">{reviewAi.examplesText}</p>
                {/if}
              </div>
            {/if}

            <div class="review-ratings">
              <button class="btn rating-btn rating-again" onclick={() => rate(1)}>
                Again<span class="rating-interval">{previewDays(card, 1)}</span>
              </button>
              <button class="btn rating-btn rating-good" onclick={() => rate(3)}>
                Got it<span class="rating-interval">{previewDays(card, 3)}</span>
              </button>
              <button class="btn rating-btn rating-easy" onclick={() => rate(5)}>
                Easy<span class="rating-interval">{previewDays(card, 5)}</span>
              </button>
            </div>

            <div class="review-secondary-actions">
              <button class="link-btn" onclick={skipCard}>Skip for now</button>
              <button class="link-btn danger" onclick={removeCurrentCard}>Delete word</button>
            </div>
          {/if}
        </div>
      {/if}

      {#if !reviewDone}<p class="review-session-count">{sessionReviewed} reviewed so far</p>{/if}
    </div>
  {/if}
  </div>
</main>
<svelte:window onkeydown={onReviewKey} />
<style>
  /* "Your words" page in the same design language as the player screens:
     shared AppHeader, serif headings, surface cards on the paper background.
     .btn / .nav-pill / .segmented / menu / modal classes live in theme.css. */

  .page {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
  }

  .vocab-body {
    width: 100%;
    max-width: 880px;
    margin: 0 auto;
    padding: var(--space-8) var(--space-8) var(--space-10);
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
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

  /* ---- progress stats ---- */

  .stats-row {
    display: flex;
    flex-wrap: wrap;
    align-items: stretch;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .stat-tile {
    flex: 1 1 96px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    padding: var(--space-4) var(--space-5);
  }

  .stat-tile + .stat-tile {
    border-left: 1px solid var(--color-border);
  }

  .stat-value {
    font: 500 32px/36px var(--font-serif);
  }

  .stat-tile-attn .stat-value {
    color: var(--color-accent);
  }

  .stat-label {
    font: 400 13px/18px var(--font-ui);
    color: var(--color-text-muted);
  }

  /* ---- toolbar: search pill + sort segmented ---- */

  .toolbar {
    display: flex;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  .search-box {
    flex: 1;
    min-width: 200px;
    min-height: 44px;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-pill);
    padding: 0 var(--space-4);
    color: var(--color-text-muted);
  }

  .search-box:focus-within {
    border-color: var(--color-text);
  }

  .search-box input {
    flex: 1;
    min-width: 0;
    border: none;
    outline: none;
    background: transparent;
    font: 400 15px/22px var(--font-ui);
    color: var(--color-text);
  }

  .search-box input:focus-visible {
    outline: none;
  }

  /* ---- word list ---- */

  .entry-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .entry-card {
    text-align: left;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-5);
    transition:
      box-shadow var(--dur-fast) var(--ease-out),
      border-color var(--dur-fast) var(--ease-out);
  }

  .entry-card:hover {
    border-color: var(--color-border-strong);
    box-shadow: var(--shadow-md);
  }

  .entry-header {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
  }

  .entry-word {
    flex: 1;
    min-width: 0;
    font: 500 24px/30px var(--font-serif);
    overflow-wrap: anywhere;
  }

  .badge-due {
    display: inline-flex;
    align-items: center;
    height: 24px;
    padding: 0 var(--space-2);
    border-radius: var(--radius-pill);
    background: var(--color-accent);
    color: var(--color-on-accent);
    font: 600 12px/24px var(--font-ui);
  }

  .entry-date {
    color: var(--color-text-muted);
    font: 400 13px/18px var(--font-ui);
    white-space: nowrap;
  }

  .entry-card .icon-btn {
    width: 32px;
    height: 32px;
    flex-shrink: 0;
    color: var(--color-text-muted);
  }

  .entry-card .icon-btn:hover {
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
    color: var(--color-danger);
  }

  .entry-definition {
    margin: var(--space-2) 0 0;
    font: 400 17px/26px var(--font-serif);
  }

  .entry-definition-empty {
    color: var(--color-text-faint);
    font-style: italic;
  }

  .entry-context {
    margin: var(--space-3) 0 0;
    font: italic 400 16px/24px var(--font-serif);
    color: var(--color-text-muted);
  }

  /* Highlighter mark on the saved word, matching the transcript's
     saved-word highlight. */
  .entry-context :global(.marked),
  .review-context :global(.marked) {
    padding: 0 0.12em;
    border-radius: 3px 9px 4px 8px;
    background: var(--color-marker);
    color: var(--color-text);
    font-style: normal;
  }

  .entry-source {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    margin: var(--space-3) 0 0;
    font: 400 13px/18px var(--font-ui);
    color: var(--color-text-faint);
  }

  /* ---- empty state ---- */

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

  /* ---- review modal ---- */
  /* .modal-backdrop/.modal-panel/.btn/.ai-explain/.ai-label are in theme.css */

  .popup-status.error {
    color: var(--color-danger);
  }

  .review-panel {
    width: min(28rem, calc(100vw - 2rem));
    max-height: min(88vh, 44rem);
    padding: var(--space-6);
  }

  .review-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.6rem;
  }

  .review-progress {
    font: 500 13px/18px var(--font-mono);
    color: var(--color-text-muted);
  }

  .review-card {
    text-align: center;
    padding: 0.6rem 0 0.2rem;
  }

  .review-word {
    display: block;
    font: 500 36px/40px var(--font-serif);
    letter-spacing: -0.01em;
    margin: var(--space-2) 0 var(--space-3);
  }

  .review-context {
    text-align: center;
    max-width: 34ch;
    margin: 0 auto var(--space-5);
    color: var(--color-text-muted);
    font: italic 400 17px/26px var(--font-serif);
  }

  .review-reveal {
    width: 100%;
    justify-content: center;
  }

  .review-definition {
    text-align: center;
    margin: var(--space-5) 0 0;
    padding-top: var(--space-5);
    border-top: 1px dashed var(--color-border-strong);
    font: 400 17px/26px var(--font-serif);
  }

  .review-ratings {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-6);
  }

  /* Tinted-outline ratings rather than solid fills — a grade is a signal to
     glance at, not a loud call to action like the primary buttons. */
  .rating-btn {
    flex: 1;
    flex-direction: column;
    gap: 0.15rem;
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md);
    padding: var(--space-3) var(--space-2);
    font-weight: 600;
  }

  .rating-btn:hover:not(:disabled) {
    background: var(--color-surface-hover);
    transform: translateY(-1px);
  }

  .rating-interval {
    font: 500 12px/16px var(--font-mono);
    color: var(--color-text-muted);
  }

  .rating-again {
    color: var(--color-danger);
  }

  .rating-good {
    color: var(--color-success);
  }

  .rating-easy {
    color: var(--color-info);
  }

  .review-secondary-actions {
    display: flex;
    justify-content: center;
    gap: 1rem;
    margin-top: 0.8rem;
  }

  .link-btn {
    background: none;
    border: none;
    padding: 0;
    font: 400 0.82rem var(--font-ui);
    color: var(--color-text-muted);
    cursor: pointer;
    text-decoration: underline;
  }

  .link-btn.danger:hover {
    color: var(--color-danger);
  }

  .review-session-count {
    text-align: center;
    margin: 1rem 0 0;
    font: 400 13px/18px var(--font-ui);
    color: var(--color-text-muted);
  }

  @media (max-width: 640px) {
    .vocab-body {
      padding: var(--space-6) var(--space-4) var(--space-10);
    }
  }
</style>
