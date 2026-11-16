// Theme picker — ported from epub-reader: theme (a color palette) and
// appearance (light/dark) are two independent, fully user-controlled axes,
// applied via `data-theme`/`data-appearance` attributes on <html> (see
// src/lib/theme.css for the `html[data-theme][data-appearance]` blocks).
// Deliberately not synced to `prefers-color-scheme` after the first launch,
// matching the sibling app exactly — once a user picks light or dark, it
// stays picked.

export type ThemeId =
  | "default"
  | "flexoki"
  | "ayu"
  | "catppuccin"
  | "everforest"
  | "gruvbox"
  | "nord"
  | "rosepine"
  | "solarized"
  | "parchment"
  | "almond"
  | "sage"
  | "mist"
  | "blush"
  | "eink"
  | "einkwarm";

export type Appearance = "light" | "dark";

export type ThemeCatalogEntry = { id: ThemeId; label: string; tagline: string };

export const THEME_CATALOG: ThemeCatalogEntry[] = [
  { id: "default", label: "Paper", tagline: "Warm & quiet" },
  { id: "flexoki", label: "Flexoki", tagline: "Ink-inspired" },
  { id: "ayu", label: "Ayu", tagline: "Bright & crisp" },
  { id: "catppuccin", label: "Catppuccin", tagline: "Pastel & soft" },
  { id: "everforest", label: "Everforest", tagline: "Forest greens" },
  { id: "gruvbox", label: "Gruvbox", tagline: "Retro warm" },
  { id: "nord", label: "Nord", tagline: "Cool & arctic" },
  { id: "rosepine", label: "Rosé Pine", tagline: "Muted rose" },
  { id: "solarized", label: "Solarized", tagline: "Balanced contrast" },
  { id: "parchment", label: "Parchment", tagline: "Old book pages" },
  { id: "almond", label: "Almond", tagline: "Soft & nutty" },
  { id: "sage", label: "Sage", tagline: "Calm green" },
  { id: "mist", label: "Mist", tagline: "Pale & hazy" },
  { id: "blush", label: "Blush", tagline: "Gentle pink" },
  { id: "eink", label: "E-Ink", tagline: "E-paper look" },
  { id: "einkwarm", label: "E-Ink Warm", tagline: "Warm e-paper" },
];

/** [bg, surface, accent] from each theme's light variant — used only to
 * render a small swatch preview on the theme picker's cards, independent of
 * the actual `html[data-theme]` CSS system (so previewing a theme doesn't
 * require applying it first). */
export const THEME_SWATCHES: Record<ThemeId, [string, string, string]> = {
  default: ["#f6f1e8", "#fffcf6", "#a84a22"],
  flexoki: ["#fffcf0", "#f2f0e5", "#af3029"],
  ayu: ["#fafafa", "#ffffff", "#f29718"],
  catppuccin: ["#eff1f5", "#e6e9ef", "#8839ef"],
  everforest: ["#fdf6e3", "#f4f0d9", "#8da101"],
  gruvbox: ["#fbf1c7", "#f2e5bc", "#af3a03"],
  nord: ["#eceff4", "#e5e9f0", "#5e81ac"],
  rosepine: ["#faf4ed", "#fffaf3", "#b4637a"],
  solarized: ["#fdf6e3", "#eee8d5", "#cb4b16"],
  parchment: ["#f4ecd8", "#fbf4e3", "#a6642c"],
  almond: ["#f7f2e9", "#fffaf1", "#9a6a3c"],
  sage: ["#e9efe4", "#f1f5ec", "#5f7d4f"],
  mist: ["#eceff2", "#f4f6f8", "#4a6f8a"],
  blush: ["#f5ece9", "#fcf4f1", "#a25f66"],
  eink: ["#f4f4f2", "#ffffff", "#2b2b2b"],
  einkwarm: ["#f2ede3", "#faf6ee", "#3a352c"],
};

export type ThemeSettings = { theme: ThemeId; appearance: Appearance };

const STORAGE_KEY = "videoLearnTheme";

function systemPrefersDark(): boolean {
  return typeof window !== "undefined" && window.matchMedia?.("(prefers-color-scheme: dark)").matches === true;
}

/** Only used the very first time (no stored settings yet) — after that,
 * whatever the user picks is respected verbatim, with no further OS sync. */
function defaultSettings(): ThemeSettings {
  return { theme: "default", appearance: systemPrefersDark() ? "dark" : "light" };
}

export function loadThemeSettings(): ThemeSettings {
  if (typeof localStorage === "undefined") return defaultSettings();
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return defaultSettings();
    const parsed = JSON.parse(raw);
    const theme: ThemeId = THEME_CATALOG.some((t) => t.id === parsed.theme) ? parsed.theme : "default";
    const appearance: Appearance = parsed.appearance === "dark" ? "dark" : "light";
    return { theme, appearance };
  } catch {
    return defaultSettings();
  }
}

export function saveThemeSettings(settings: ThemeSettings): void {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // storage full/unavailable — theme choice just won't persist
  }
}

export function applyTheme(settings: ThemeSettings): void {
  if (typeof document === "undefined") return;
  document.documentElement.dataset.theme = settings.theme;
  document.documentElement.dataset.appearance = settings.appearance;
}
