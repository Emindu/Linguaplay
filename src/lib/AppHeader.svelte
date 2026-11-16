<script lang="ts">
  import ThemeSwitcher from "./ThemeSwitcher.svelte";
  import type { Snippet } from "svelte";
  import { goto } from "$app/navigation";
  import { player } from "./player.svelte";

  // Shared top bar for every route, per the Player Screens design: brand
  // (accent dot + Newsreader-italic wordmark), an optional YouTube-link
  // pill (player page only, and only once a video is loaded — the start
  // screen carries the big link form in its dropzone instead), the
  // Library / Your words pills, and the hamburger menu.
  //
  // On narrow (phone) widths, when `mobileTitle` is set the brand, link
  // bar and nav pills collapse into a back-arrow + title + menu row,
  // matching the phone player screens.
  let {
    wordsCount = 0,
    showLinkbar = false,
    linkValue = $bindable(""),
    linkLoading = false,
    linkPlaceholder = "Paste a YouTube link, or drop a file",
    onLinkSubmit,
    mobileTitle = "",
    mobileMeta = "",
    actionsExtras,
    menuExtras,
  }: {
    wordsCount?: number;
    showLinkbar?: boolean;
    linkValue?: string;
    linkLoading?: boolean;
    linkPlaceholder?: string;
    onLinkSubmit?: (value: string) => void;
    mobileTitle?: string;
    mobileMeta?: string;
    actionsExtras?: Snippet;
    menuExtras?: Snippet;
  } = $props();

  let headerMenuOpen = $state(false);
  let themeSwitcherOpen = $state(false);

  /** "Home" is the start screen, which is the player route with nothing
   * loaded — so close any open video, then go to "/". */
  function goHome(event: MouseEvent) {
    event.preventDefault();
    player.closeVideo();
    void goto("/");
  }

  function submitLink(event: SubmitEvent) {
    event.preventDefault();
    if (!linkValue.trim()) return;
    onLinkSubmit?.(linkValue);
    linkValue = "";
  }
</script>

{#snippet iconLink()}
  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M9.5 14.5l5-5" />
    <path d="M11 6.5l1-1a3.5 3.5 0 0 1 5 5l-1 1" />
    <path d="M13 17.5l-1 1a3.5 3.5 0 0 1-5-5l1-1" />
  </svg>
{/snippet}

{#snippet iconClock()}
  <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <circle cx="12" cy="12" r="9" />
    <path d="M12 7v5l3.5 2" />
  </svg>
{/snippet}

{#snippet iconBook()}
  <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M4 5.5C4 4.7 4.7 4 5.5 4H12v16H5.5c-.8 0-1.5-.7-1.5-1.5v-13z" />
    <path d="M20 5.5c0-.8-.7-1.5-1.5-1.5H12v16h6.5c.8 0 1.5-.7 1.5-1.5v-13z" />
  </svg>
{/snippet}

{#snippet iconMenu()}
  <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
    <path d="M4 6h16M4 12h16M4 18h16" />
  </svg>
{/snippet}

{#snippet iconBack()}
  <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M15 5l-7 7 7 7" />
  </svg>
{/snippet}

{#snippet iconPalette()}
  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M12 3a9 9 0 1 0 0 18c1.1 0 1.6-.6 1.6-1.4 0-.4-.2-.7-.4-1a1.6 1.6 0 0 1 1.2-2.6H16a4 4 0 0 0 4-4c0-4.4-3.6-9-8-9z" />
    <circle cx="7.5" cy="10.5" r="1.1" fill="currentColor" stroke="none" />
    <circle cx="11" cy="7" r="1.1" fill="currentColor" stroke="none" />
    <circle cx="15.5" cy="8" r="1.1" fill="currentColor" stroke="none" />
  </svg>
{/snippet}

<header class="app-header" class:phone={!!mobileTitle}>
  {#if mobileTitle}
    <a class="icon-btn mobile-back" href="/" onclick={goHome} aria-label="Back to home">{@render iconBack()}</a>
    <div class="mobile-title">
      <p class="mobile-title-text">{mobileTitle}</p>
      {#if mobileMeta}<p class="mobile-title-meta">{mobileMeta}</p>{/if}
    </div>
  {:else}
    <a class="brand" href="/" onclick={goHome} aria-label="Linguaplay — home">
      <span class="brand-dot"></span>
      <span class="brand-name">Linguaplay</span>
    </a>
  {/if}

  {#if showLinkbar}
    <form class="linkbar" onsubmit={submitLink}>
      {@render iconLink()}
      <label class="sr-only" for="header-link">YouTube link</label>
      <input id="header-link" type="text" placeholder={linkPlaceholder} aria-label="YouTube link" bind:value={linkValue} disabled={linkLoading} />
      <button class="btn btn-primary" type="submit" disabled={linkLoading || !linkValue.trim()}>
        {linkLoading ? "Opening…" : "Load"}
      </button>
    </form>
  {/if}

  <div class="header-actions">
    {@render actionsExtras?.()}
    <a class="nav-pill" href="/library">{@render iconClock()} Library</a>
    <a class="nav-pill" href="/vocabulary">
      {@render iconBook()} Your words
      {#if wordsCount > 0}<span class="count">{wordsCount}</span>{/if}
    </a>
    <div class="menu-trigger">
      <button class="icon-btn" onclick={() => (headerMenuOpen = !headerMenuOpen)} aria-label="Menu" aria-expanded={headerMenuOpen}>
        {@render iconMenu()}
      </button>
      {#if headerMenuOpen}
        <button class="menu-backdrop" aria-label="Close menu" onclick={() => (headerMenuOpen = false)}></button>
        <div class="menu menu-down" role="menu">
          <button class="menu-item" role="menuitem" onclick={() => { headerMenuOpen = false; themeSwitcherOpen = true; }}>
            {@render iconPalette()} Theme
          </button>
          <!-- The Library / Your words pills are hidden in the phone layout, so
               the menu carries them there. -->
          <a class="menu-item menu-mobile-only" role="menuitem" href="/" onclick={goHome}>Home</a>
          <a class="menu-item menu-mobile-only" role="menuitem" href="/library">Library</a>
          <a class="menu-item menu-mobile-only" role="menuitem" href="/vocabulary">Your words</a>
          <!-- Clicking any extra item bubbles up here and closes the menu. -->
          <div role="none" onclick={() => (headerMenuOpen = false)}>
            {@render menuExtras?.()}
          </div>
        </div>
      {/if}
    </div>
    <ThemeSwitcher hideTrigger bind:open={themeSwitcherOpen} />
  </div>
</header>

<style>
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }

  /* Phone layout: back arrow + title/meta replace the brand and pills. */
  .mobile-back {
    margin-left: -10px;
  }

  .mobile-title {
    flex: 1;
    min-width: 0;
  }

  .mobile-title-text {
    margin: 0;
    font: 500 17px/22px var(--font-serif);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mobile-title-meta {
    margin: 0;
    color: var(--color-text-muted);
    font: 400 12px/16px var(--font-ui);
  }

  .menu-mobile-only {
    display: none;
  }

  @media (max-width: 640px) {
    .app-header {
      padding: 10px var(--space-4);
    }

    .app-header.phone .menu-mobile-only {
      display: flex;
    }
  }

  /* When the phone title row is active, the desktop brand/linkbar/pills
     are hidden and the title row takes their place. */
  @media (max-width: 640px) {
    .app-header.phone .brand,
    .app-header.phone .linkbar,
    .app-header.phone .nav-pill {
      display: none;
    }
  }
</style>
