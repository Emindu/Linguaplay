<script lang="ts">
  import {
    player,
    FONT_SIZE_OPTIONS,
    FONT_FAMILY_OPTIONS,
    SUBTITLE_BACKGROUND_OPTIONS,
    SUBTITLE_OUTLINE_OPTIONS,
    SUBTITLE_TEXT_COLORS,
    SUBTITLE_BG_COLORS,
    SUBTITLE_WEIGHT_OPTIONS,
    SUBTITLE_POSITION_OPTIONS,
    SUBTITLE_PRESETS,
  } from "$lib/player.svelte";
  import { subtitleStyleVars } from "$lib/subtitleStyle";

  const previewStyle = $derived(subtitleStyleVars(player));
  // Cheap check for "is this preset what's applied now", to mark it selected.
  const activePreset = $derived(
    SUBTITLE_PRESETS.find(
      (p) =>
        p.textColor === player.subtitleTextColor &&
        p.bgColor === player.subtitleBgColor &&
        p.bgAlpha === player.subtitleBgAlpha &&
        p.outline === player.subtitleOutline &&
        p.weight === player.subtitleWeight,
    )?.label,
  );
</script>

<div class="style-panel">
  <!-- Preview on a split light/dark strip so a style that disappears on a
       bright or a dark scene is obvious right here. -->
  <div class="sp-preview" aria-hidden="true">
    <div class="sp-preview-light"></div>
    <div class="sp-preview-dark"></div>
    <div class="sp-preview-text subtitle-overlay-preview" style={previewStyle}>
      <span>Words you can tap</span>
    </div>
  </div>

  <div class="sp-label">Presets</div>
  <div class="sp-chips">
    {#each SUBTITLE_PRESETS as preset (preset.label)}
      <button class="sp-chip" class:is-on={activePreset === preset.label} onclick={() => player.applySubtitlePreset(preset)}>
        {preset.label}
      </button>
    {/each}
  </div>

  <div class="sp-label">Text color</div>
  <div class="sp-swatches" role="radiogroup" aria-label="Text color">
    {#each SUBTITLE_TEXT_COLORS as opt (opt.label)}
      <button
        class="sp-swatch"
        class:is-on={player.subtitleTextColor === opt.value}
        style="--sw: {opt.value || '#f3ede3'}"
        role="radio"
        aria-checked={player.subtitleTextColor === opt.value}
        aria-label={opt.label}
        title={opt.label}
        onclick={() => player.setSubtitleTextColor(opt.value)}
      ></button>
    {/each}
  </div>

  <div class="sp-label">Background color</div>
  <div class="sp-swatches" role="radiogroup" aria-label="Background color">
    {#each SUBTITLE_BG_COLORS as opt (opt.label)}
      <button
        class="sp-swatch"
        class:is-on={player.subtitleBgColor === opt.value}
        style="--sw: {opt.value}"
        role="radio"
        aria-checked={player.subtitleBgColor === opt.value}
        aria-label={opt.label}
        title={opt.label}
        onclick={() => player.setSubtitleBgColor(opt.value)}
      ></button>
    {/each}
  </div>

  <div class="sp-label">Background</div>
  <div class="sp-chips">
    {#each SUBTITLE_BACKGROUND_OPTIONS as opt (opt.label)}
      <button class="sp-chip" class:is-on={player.subtitleBgAlpha === opt.alpha} onclick={() => player.setSubtitleBgAlpha(opt.alpha)}>
        {opt.label}
      </button>
    {/each}
  </div>

  <div class="sp-label">Outline</div>
  <div class="sp-chips">
    {#each SUBTITLE_OUTLINE_OPTIONS as opt (opt)}
      <button class="sp-chip" class:is-on={player.subtitleOutline === opt} onclick={() => player.setSubtitleOutline(opt)}>{opt}</button>
    {/each}
  </div>

  <div class="sp-label">Weight</div>
  <div class="sp-chips">
    {#each SUBTITLE_WEIGHT_OPTIONS as opt (opt.label)}
      <button
        class="sp-chip"
        class:is-on={player.subtitleWeight === opt.weight}
        style="font-weight: {opt.weight}"
        onclick={() => player.setSubtitleWeight(opt.weight)}
      >
        {opt.label}
      </button>
    {/each}
  </div>

  <div class="sp-label">Size</div>
  <div class="sp-chips">
    {#each FONT_SIZE_OPTIONS as opt (opt.label)}
      <button class="sp-chip" class:is-on={player.subtitleFontSize === opt.rem} onclick={() => player.setSubtitleFontSize(opt.rem)}>
        {opt.label}
      </button>
    {/each}
  </div>

  <div class="sp-label">Position</div>
  <div class="sp-chips">
    {#each SUBTITLE_POSITION_OPTIONS as opt (opt)}
      <button class="sp-chip" class:is-on={player.subtitlePosition === opt} onclick={() => player.setSubtitlePosition(opt)}>{opt}</button>
    {/each}
  </div>

  <div class="sp-label">Font</div>
  <div class="sp-chips">
    {#each FONT_FAMILY_OPTIONS as font (font)}
      <button
        class="sp-chip"
        class:is-on={player.subtitleFontFamily === font}
        style="font-family: '{font}', sans-serif"
        onclick={() => player.setSubtitleFontFamily(font)}
      >
        {font}
      </button>
    {/each}
  </div>

  <button class="sp-reset" onclick={() => player.resetSubtitleStyle()}>Reset to default</button>
</div>

<style>
  .style-panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    width: min(340px, 86vw);
    padding: var(--space-1) var(--space-2) var(--space-2);
  }

  .sp-label {
    margin-top: var(--space-2);
    color: var(--player-text-muted, var(--color-text-muted));
    font: 600 11px/16px var(--font-ui);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .sp-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .sp-chip {
    padding: 4px 10px;
    border: 1px solid var(--player-border, var(--color-border));
    border-radius: var(--radius-pill);
    background: transparent;
    color: var(--player-text, var(--color-text));
    font: 500 12px/18px var(--font-ui);
    cursor: pointer;
  }

  .sp-chip:hover {
    background: var(--player-surface-hover, var(--color-surface-hover));
  }

  .sp-chip.is-on {
    border-color: var(--player-accent, var(--color-accent));
    box-shadow: inset 0 0 0 1px var(--player-accent, var(--color-accent));
  }

  .sp-swatches {
    display: flex;
    gap: 8px;
  }

  .sp-swatch {
    width: 26px;
    height: 26px;
    border-radius: 50%;
    border: 2px solid var(--player-border, var(--color-border));
    background: var(--sw);
    cursor: pointer;
    padding: 0;
  }

  .sp-swatch.is-on {
    border-color: var(--player-accent, var(--color-accent));
    outline: 2px solid var(--player-accent, var(--color-accent));
    outline-offset: 1px;
  }

  .sp-chip:focus-visible,
  .sp-swatch:focus-visible,
  .sp-reset:focus-visible {
    outline: 2px solid var(--player-accent, var(--color-accent));
    outline-offset: 2px;
  }

  .sp-reset {
    align-self: flex-start;
    margin-top: var(--space-3);
    padding: 4px 10px;
    border: 0;
    background: transparent;
    color: var(--player-text-muted, var(--color-text-muted));
    font: 500 12px/18px var(--font-ui);
    text-decoration: underline;
    cursor: pointer;
  }

  /* ---- preview ---- */

  .sp-preview {
    position: relative;
    display: grid;
    grid-template-columns: 1fr 1fr;
    height: 64px;
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .sp-preview-light {
    background: linear-gradient(135deg, #ffffff, #e8eef5);
  }

  .sp-preview-dark {
    background: linear-gradient(135deg, #1a1d24, #07080b);
  }

  .sp-preview-text {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    /* The real size is far too big for a swatch; scale the preview down. */
    font-size: min(var(--sub-size, 1.5rem), 1.15rem);
  }

  .sp-preview-text span {
    padding: 0.15em 0.5em;
    background: rgba(var(--subtitle-bg-rgb, 13, 11, 9), var(--subtitle-bg-alpha, 0.6));
    border-radius: var(--radius-sm);
    color: var(--subtitle-color, #f3ede3);
    font-weight: var(--subtitle-weight, 500);
    text-shadow: var(--subtitle-outline, none);
    white-space: nowrap;
  }
</style>
