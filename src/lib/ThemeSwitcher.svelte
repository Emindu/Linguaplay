<script lang="ts">
  import { onMount } from "svelte";
  import {
    THEME_CATALOG,
    THEME_SWATCHES,
    loadThemeSettings,
    saveThemeSettings,
    applyTheme,
    type ThemeId,
    type Appearance,
  } from "./themeSettings";

  // `hideTrigger` lets a page fold this into its own header menu instead of
  // showing the default standalone pill button — the caller drives `open`
  // via the bindable prop in that case (e.g. a "Theme" item in a dropdown).
  let { hideTrigger = false, open = $bindable(false) }: { hideTrigger?: boolean; open?: boolean } = $props();

  let theme = $state<ThemeId>("default");
  let appearance = $state<Appearance>("light");

  function sync() {
    const s = loadThemeSettings();
    theme = s.theme;
    appearance = s.appearance;
  }

  function openPicker() {
    sync();
    open = true;
  }

  // Covers both trigger paths: our own pill button (openPicker, above) and
  // a caller setting `open` directly (hideTrigger mode) — either way the
  // picker reflects the latest saved settings whenever it becomes visible.
  $effect(() => {
    if (open) sync();
  });

  function pickTheme(id: ThemeId) {
    theme = id;
    saveThemeSettings({ theme, appearance });
    applyTheme({ theme, appearance });
  }

  function pickAppearance(a: Appearance) {
    appearance = a;
    saveThemeSettings({ theme, appearance });
    applyTheme({ theme, appearance });
  }

  onMount(sync);
</script>

{#snippet iconPalette()}
  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <path
      d="M12 3a9 9 0 1 0 0 18c1.1 0 1.6-.6 1.6-1.4 0-.4-.2-.7-.4-1a1.6 1.6 0 0 1 1.2-2.6H16a4 4 0 0 0 4-4c0-4.4-3.6-9-8-9z"
    />
    <circle cx="7.5" cy="10.5" r="1.1" fill="currentColor" stroke="none" />
    <circle cx="11" cy="7" r="1.1" fill="currentColor" stroke="none" />
    <circle cx="15.5" cy="8" r="1.1" fill="currentColor" stroke="none" />
  </svg>
{/snippet}

{#snippet iconClose()}
  <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"
    ><path d="M6 6l12 12M18 6L6 18" /></svg
  >
{/snippet}

{#if !hideTrigger}
  <button class="nav-pill" onclick={openPicker}>{@render iconPalette()} Theme</button>
{/if}

{#if open}
  <div class="modal-backdrop" onclick={() => (open = false)} role="presentation"></div>
  <div class="modal-panel theme-modal" role="dialog" aria-label="Theme settings">
    <div class="theme-head">
      <h2>Theme</h2>
      <div class="appearance-toggle" role="group" aria-label="Light or dark">
        <button aria-pressed={appearance === "light"} onclick={() => pickAppearance("light")}>Light</button>
        <button aria-pressed={appearance === "dark"} onclick={() => pickAppearance("dark")}>Dark</button>
      </div>
      <button class="modal-close" onclick={() => (open = false)} aria-label="Close">{@render iconClose()}</button>
    </div>

    <div class="theme-grid">
      {#each THEME_CATALOG as entry (entry.id)}
        <button class="theme-card" aria-pressed={theme === entry.id} onclick={() => pickTheme(entry.id)}>
          <span class="theme-swatch" aria-hidden="true" style="background: {THEME_SWATCHES[entry.id][0]}">
            <i style="width: 40%; background: {THEME_SWATCHES[entry.id][1]}"></i>
            <i style="width: 18%; background: {THEME_SWATCHES[entry.id][2]}"></i>
          </span>
          <strong>{entry.label}</strong>
          <span class="theme-tagline">{entry.tagline}</span>
        </button>
      {/each}
    </div>
  </div>
{/if}

<style>
  .theme-modal {
    width: min(36rem, calc(100vw - 2rem));
  }

  .theme-head {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-bottom: var(--space-5);
  }

  .theme-head h2 {
    margin: 0;
    margin-right: auto;
    font-family: var(--font-serif);
    font-style: italic;
    font-weight: 500;
    font-size: 22px;
    line-height: 28px;
  }

  .appearance-toggle {
    display: inline-flex;
    gap: 2px;
    background: var(--color-surface-hover);
    border-radius: var(--radius-pill);
    padding: 3px;
  }

  .appearance-toggle button {
    border: none;
    background: transparent;
    border-radius: var(--radius-pill);
    padding: 0.35rem 0.95rem;
    font-family: inherit;
    font-size: 0.86rem;
    font-weight: 600;
    color: var(--color-text-muted);
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-out), color var(--dur-fast) var(--ease-out);
  }

  .appearance-toggle button[aria-pressed="true"] {
    background: var(--color-surface);
    color: var(--color-text);
    box-shadow: var(--shadow-sm);
  }

  .theme-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(9.5rem, 1fr));
    gap: var(--space-3);
    max-height: 60vh;
    overflow-y: auto;
    padding: 2px;
  }

  .theme-card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.25rem;
    text-align: left;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-3);
    font-family: inherit;
    cursor: pointer;
    transition: transform var(--dur-fast) var(--ease-out), border-color var(--dur-fast) var(--ease-out);
  }

  .theme-card:hover {
    transform: translateY(-1px);
    border-color: var(--color-border-strong);
  }

  .theme-card[aria-pressed="true"] {
    border-color: var(--color-text);
    box-shadow: 0 0 0 1px var(--color-text);
  }

  .theme-card strong {
    font-size: 0.9rem;
    font-weight: 600;
    color: var(--color-text);
  }

  .theme-tagline {
    font-size: 0.78rem;
    color: var(--color-text-muted);
  }

  .theme-swatch {
    display: flex;
    align-items: flex-end;
    gap: 6px;
    width: 100%;
    height: 44px;
    padding: 8px;
    box-sizing: border-box;
    border-radius: var(--radius-sm);
    border: 1px solid var(--color-border);
    margin-bottom: 4px;
  }

  .theme-swatch i {
    display: block;
    height: 10px;
    border-radius: 5px;
    outline: 1px solid rgba(0, 0, 0, 0.08);
  }
</style>
