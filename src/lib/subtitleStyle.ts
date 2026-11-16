import type { SubtitlePosition } from "./player.svelte";

type StyleSource = {
  subtitleFontSize: number;
  subtitleFontFamily: string;
  subtitleBgAlpha: number;
  subtitleBgColor: string;
  subtitleTextColor: string;
  subtitleWeight: number;
  subtitleOutline: string;
};

function hexToRgb(hex: string): string {
  const m = /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
  return m ? `${parseInt(m[1], 16)}, ${parseInt(m[2], 16)}, ${parseInt(m[3], 16)}` : "13, 11, 9";
}

/** The outline has to contrast with the letters: a black one is invisible
 * around black text, so dark text gets a white one. */
function outlineShadow(outline: string, textColor: string): string {
  const c = textColor.toLowerCase() === "#000000" ? "#fff" : "#000";
  if (outline === "Normal") return `-1px -1px 0 ${c}, 1px -1px 0 ${c}, -1px 1px 0 ${c}, 1px 1px 0 ${c}`;
  if (outline === "Strong")
    return `-2px -2px 2px ${c}, 2px -2px 2px ${c}, -2px 2px 2px ${c}, 2px 2px 2px ${c}, 0 0 6px ${c}`;
  return "none";
}

/** Inline style for an element that shows subtitle text (the on-video
 * overlay and the preview in the style panel). */
export function subtitleStyleVars(s: StyleSource): string {
  return [
    `--sub-size: ${s.subtitleFontSize}rem`,
    `font-family: '${s.subtitleFontFamily}', sans-serif`,
    `--subtitle-bg-alpha: ${s.subtitleBgAlpha}`,
    `--subtitle-bg-rgb: ${hexToRgb(s.subtitleBgColor)}`,
    `--subtitle-outline: ${outlineShadow(s.subtitleOutline, s.subtitleTextColor)}`,
    s.subtitleTextColor ? `--subtitle-color: ${s.subtitleTextColor}` : "",
    `--subtitle-weight: ${s.subtitleWeight}`,
  ]
    .filter(Boolean)
    .join("; ");
}

export function positionClass(position: SubtitlePosition): string {
  return position === "Top" ? "pos-top" : position === "Middle" ? "pos-middle" : "";
}
