import { invoke, Channel } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { baseName, upsertRecent, updateRecentProgress, removeRecent, type RecentItem } from "./recents";

export type SubtitleCue = {
  start_ms: number;
  end_ms: number;
  text: string;
};

export type EmbeddedSubtitleTrack = {
  id: number;
  language: string | null;
  name: string | null;
};

type LoadFileResponse = {
  duration_ms: number;
  width: number;
  height: number;
  subtitles: SubtitleCue[];
  title: string | null;
};

export type QualityOption = { label: string; maxDimension: number };
export const QUALITY_OPTIONS: QualityOption[] = [
  { label: "Standard", maxDimension: 960 },
  { label: "High", maxDimension: 1280 },
  { label: "Full HD", maxDimension: 1920 },
];

/** Max video height to download when a YouTube video has to be downloaded
 * (see `#downloadAndPlay`). `height: 0` = Auto: follow the playback-quality
 * preset and accept whatever is already downloaded. Playback itself is
 * capped at 1920px wide, so nothing above 1080p would ever be shown. */
export type DownloadQualityOption = { label: string; height: number };
export const DOWNLOAD_QUALITY_OPTIONS: DownloadQualityOption[] = [
  { label: "Auto", height: 0 },
  { label: "360p", height: 360 },
  { label: "480p", height: 480 },
  { label: "720p HD", height: 720 },
  { label: "1080p Full HD", height: 1080 },
];
const DOWNLOAD_QUALITY_STORAGE_KEY = "youtubeDownloadQualityHeight";

function loadStoredDownloadQuality(): DownloadQualityOption {
  if (typeof localStorage === "undefined") return DOWNLOAD_QUALITY_OPTIONS[0];
  const stored = Number(localStorage.getItem(DOWNLOAD_QUALITY_STORAGE_KEY));
  return DOWNLOAD_QUALITY_OPTIONS.find((o) => o.height === stored) ?? DOWNLOAD_QUALITY_OPTIONS[0];
}

/** Live status of a YouTube download, streamed from the backend. Mirrors
 * `DownloadProgress` (Rust) in src-tauri/src/youtube.rs. */
export type YoutubeDownloadProgress = {
  phase: "video" | "audio" | "merging";
  stage: number;
  stages: number;
  stage_percent: number;
  overall_percent: number;
  downloaded_bytes: number;
  total_bytes: number | null;
  speed_bps: number | null;
  eta_secs: number | null;
};

export type AspectOption = { label: string; num: number; den: number };
export const ASPECT_OPTIONS: AspectOption[] = [
  { label: "Auto (Original)", num: 0, den: 0 },
  { label: "16:9", num: 16, den: 9 },
  { label: "4:3", num: 4, den: 3 },
  { label: "21:9", num: 21, den: 9 },
  { label: "1:1", num: 1, den: 1 },
];

export type FontSizeOption = { label: string; rem: number };
export const FONT_SIZE_OPTIONS: FontSizeOption[] = [
  { label: "Small", rem: 1.0 },
  { label: "Medium", rem: 1.3 },
  { label: "Large", rem: 1.625 },
  { label: "X-Large", rem: 2.1 },
  { label: "XX-Large", rem: 2.6 },
];
const DEFAULT_FONT_SIZE_REM = 1.625;

export const FONT_FAMILY_OPTIONS = [
  "Figtree",
  "Inter",
  "Arial",
  "Georgia",
  "Verdana",
  "Times New Roman",
  "Courier New",
  "Trebuchet MS",
  "Comic Sans MS",
] as const;
const DEFAULT_FONT_FAMILY = "Figtree";

const FONT_SIZE_STORAGE_KEY = "subtitleFontSizeRem";
const FONT_FAMILY_STORAGE_KEY = "subtitleFontFamily";

function loadStoredFontSize(): number {
  if (typeof localStorage === "undefined") return DEFAULT_FONT_SIZE_REM;
  const stored = Number(localStorage.getItem(FONT_SIZE_STORAGE_KEY));
  return FONT_SIZE_OPTIONS.some((o) => o.rem === stored) ? stored : DEFAULT_FONT_SIZE_REM;
}

function loadStoredFontFamily(): string {
  if (typeof localStorage === "undefined") return DEFAULT_FONT_FAMILY;
  const stored = localStorage.getItem(FONT_FAMILY_STORAGE_KEY);
  return stored && (FONT_FAMILY_OPTIONS as readonly string[]).includes(stored) ? stored : DEFAULT_FONT_FAMILY;
}

export type SubtitleBackgroundOption = { label: string; alpha: number };
export const SUBTITLE_BACKGROUND_OPTIONS: SubtitleBackgroundOption[] = [
  { label: "Light", alpha: 0.35 },
  { label: "Boxed", alpha: 0.6 },
  { label: "Solid", alpha: 0.92 },
  { label: "None", alpha: 0 },
];
const DEFAULT_SUBTITLE_BG_ALPHA = 0.6;

export const SUBTITLE_OUTLINE_OPTIONS = ["Off", "Normal", "Strong"] as const;
export type SubtitleOutline = (typeof SUBTITLE_OUTLINE_OPTIONS)[number];
const DEFAULT_SUBTITLE_OUTLINE: SubtitleOutline = "Off";

export type SubtitleColorOption = { label: string; value: string };
/** "" means the theme default (a warm off-white). */
export const SUBTITLE_TEXT_COLORS: SubtitleColorOption[] = [
  { label: "Default", value: "" },
  { label: "White", value: "#ffffff" },
  { label: "Yellow", value: "#ffe14d" },
  { label: "Cyan", value: "#5fe3ff" },
  { label: "Green", value: "#8dff9f" },
  { label: "Black", value: "#000000" },
];
export const SUBTITLE_BG_COLORS: SubtitleColorOption[] = [
  { label: "Dark", value: "#0d0b09" },
  { label: "Black", value: "#000000" },
  { label: "White", value: "#ffffff" },
  { label: "Navy", value: "#0b1f4a" },
];
export const SUBTITLE_WEIGHT_OPTIONS = [
  { label: "Regular", weight: 400 },
  { label: "Medium", weight: 500 },
  { label: "Bold", weight: 700 },
] as const;
export const SUBTITLE_POSITION_OPTIONS = ["Top", "Middle", "Bottom"] as const;
export type SubtitlePosition = (typeof SUBTITLE_POSITION_OPTIONS)[number];

const SUBTITLE_TEXT_COLOR_STORAGE_KEY = "subtitleTextColor";
const SUBTITLE_BG_COLOR_STORAGE_KEY = "subtitleBgColor";
const SUBTITLE_WEIGHT_STORAGE_KEY = "subtitleWeight";
const SUBTITLE_POSITION_STORAGE_KEY = "subtitlePosition";

function readStored(key: string): string | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStored(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // a preference only
  }
}

function loadStoredOption(key: string, allowed: readonly string[], fallback: string): string {
  const stored = readStored(key);
  return stored !== null && allowed.includes(stored) ? stored : fallback;
}

/** Ready-made looks for when the default is hard to read over a bright scene. */
export const SUBTITLE_PRESETS = [
  { label: "Default", textColor: "", bgColor: "#0d0b09", bgAlpha: 0.6, outline: "Off", weight: 500 },
  { label: "High contrast", textColor: "#ffffff", bgColor: "#000000", bgAlpha: 0.92, outline: "Normal", weight: 700 },
  { label: "Yellow", textColor: "#ffe14d", bgColor: "#000000", bgAlpha: 0.75, outline: "Normal", weight: 700 },
  { label: "Dark on light", textColor: "#000000", bgColor: "#ffffff", bgAlpha: 0.92, outline: "Off", weight: 500 },
] as const;

const SUBTITLE_BG_STORAGE_KEY = "subtitleBgAlpha";
const SUBTITLE_OUTLINE_STORAGE_KEY = "subtitleOutline";

function loadStoredSubtitleBgAlpha(): number {
  if (typeof localStorage === "undefined") return DEFAULT_SUBTITLE_BG_ALPHA;
  const raw = localStorage.getItem(SUBTITLE_BG_STORAGE_KEY);
  // Nothing saved yet must mean "default": Number(null) is 0, which would
  // otherwise match the "None" option and leave first-time users with no
  // background behind the subtitles at all.
  if (raw === null) return DEFAULT_SUBTITLE_BG_ALPHA;
  const stored = Number(raw);
  return SUBTITLE_BACKGROUND_OPTIONS.some((o) => o.alpha === stored) ? stored : DEFAULT_SUBTITLE_BG_ALPHA;
}

function loadStoredSubtitleOutline(): SubtitleOutline {
  if (typeof localStorage === "undefined") return DEFAULT_SUBTITLE_OUTLINE;
  const stored = localStorage.getItem(SUBTITLE_OUTLINE_STORAGE_KEY);
  return (SUBTITLE_OUTLINE_OPTIONS as readonly string[]).includes(stored ?? "")
    ? (stored as SubtitleOutline)
    : DEFAULT_SUBTITLE_OUTLINE;
}

export type PlaybackRateOption = { label: string; value: number };
export const PLAYBACK_RATE_OPTIONS: PlaybackRateOption[] = [
  { label: "0.5x", value: 0.5 },
  { label: "0.75x", value: 0.75 },
  { label: "1x (Normal)", value: 1 },
  { label: "1.25x", value: 1.25 },
  { label: "1.5x", value: 1.5 },
  { label: "1.75x", value: 1.75 },
  { label: "2x", value: 2 },
];
const DEFAULT_PLAYBACK_RATE = 1;
const PLAYBACK_RATE_STORAGE_KEY = "playbackRate";

function loadStoredPlaybackRate(): number {
  if (typeof localStorage === "undefined") return DEFAULT_PLAYBACK_RATE;
  const stored = Number(localStorage.getItem(PLAYBACK_RATE_STORAGE_KEY));
  return PLAYBACK_RATE_OPTIONS.some((o) => o.value === stored) ? stored : DEFAULT_PLAYBACK_RATE;
}

export const LANGUAGE_NAMES: Record<string, string> = {
  eng: "English",
  spa: "Spanish",
  fre: "French",
  fra: "French",
  ger: "German",
  deu: "German",
  jpn: "Japanese",
  chi: "Chinese",
  zho: "Chinese",
  kor: "Korean",
  rus: "Russian",
  ita: "Italian",
  por: "Portuguese",
  ara: "Arabic",
  hin: "Hindi",
};

/** Must match `youtube::RESTRICTED_STREAM_ERROR` in the Rust backend
 * exactly — the marker `load_youtube` returns when its Range probe finds a
 * stream YouTube's CDN will cut off partway through. */
const RESTRICTED_STREAM_ERROR = "YOUTUBE_STREAM_RESTRICTED";

/** Pulls the 11-character video ID out of the common YouTube URL shapes. */
export function extractYoutubeVideoId(url: string): string | null {
  const match = url
    .trim()
    .match(/(?:youtube\.com\/(?:watch\?(?:.*&)?v=|shorts\/|embed\/)|youtu\.be\/)([a-zA-Z0-9_-]{11})/);
  return match ? match[1] : null;
}

export function trackLabel(t: EmbeddedSubtitleTrack): string {
  const lang = t.language ? (LANGUAGE_NAMES[t.language.toLowerCase()] ?? t.language.toUpperCase()) : null;
  // YouTube's own caption "name" is often just the language's English name
  // again (e.g. name="English" for lang="en") — skip the redundant suffix
  // when it doesn't add anything beyond the resolved language label.
  if (lang && t.name && t.name.toLowerCase() !== lang.toLowerCase()) return `${lang} — ${t.name}`;
  return lang ?? t.name ?? `Track ${t.id}`;
}

type PlaybackStatus = {
  is_playing: boolean;
  position_ms: number;
  duration_ms: number;
  width: number;
  height: number;
  volume: number;
};

export type DefinitionMeaning = {
  part_of_speech: string;
  definitions: string[];
};

export type WordDefinition = {
  word: string;
  phonetic: string | null;
  audio_url: string | null;
  meanings: DefinitionMeaning[];
  summary: string | null;
};

export type WordPopup = {
  word: string;
  sentence: string;
  timestampMs: number;
  videoSource: string;
  x: number;
  y: number;
  loading: boolean;
  definition: WordDefinition | null;
  error: string;
  added: boolean;
  aiLoading: boolean;
  aiText: string;
  aiError: string;
  examplesLoading: boolean;
  examplesText: string;
  examplesError: string;
};

// ---- Local LLM (context-aware "Explain in context") ----

export type LlmModel = { path: string; name: string; size_bytes: number };
export type LlmStatus = {
  binary_exists: boolean;
  binary_path: string;
  models_dir: string;
  models: LlmModel[];
};

export type CatalogModel = {
  id: string; // file stem — matches what Rust's file_stem() produces
  label: string;
  desc: string;
  repo: string; // Hugging Face repo, e.g. "Qwen/Qwen2.5-1.5B-Instruct-GGUF"
  file: string; // exact filename inside that repo
  sizeMb: number;
};

// Small, curated set from one trusted publisher (Qwen's own GGUF repos),
// spanning fast/balanced/best-quality.
export const LLM_CATALOG: CatalogModel[] = [
  { id: "qwen2.5-0.5b-instruct-q4_k_m", label: "Qwen2.5 0.5B Instruct", desc: "Fastest, lower quality", repo: "Qwen/Qwen2.5-0.5B-Instruct-GGUF", file: "qwen2.5-0.5b-instruct-q4_k_m.gguf", sizeMb: 469 },
  { id: "qwen2.5-1.5b-instruct-q4_k_m", label: "Qwen2.5 1.5B Instruct", desc: "Balanced (recommended)", repo: "Qwen/Qwen2.5-1.5B-Instruct-GGUF", file: "qwen2.5-1.5b-instruct-q4_k_m.gguf", sizeMb: 1066 },
  { id: "qwen2.5-3b-instruct-q4_k_m", label: "Qwen2.5 3B Instruct", desc: "Best quality, slower", repo: "Qwen/Qwen2.5-3B-Instruct-GGUF", file: "qwen2.5-3b-instruct-q4_k_m.gguf", sizeMb: 2008 },
];

function modelGgufUrl(m: CatalogModel): string {
  return `https://huggingface.co/${m.repo}/resolve/main/${m.file}`;
}

// GitHub's API is needed (rather than the `/releases/latest/download/<name>`
// redirect trick used elsewhere) because llama.cpp's asset filenames embed a
// changing build number (e.g. "llama-b10099-bin-win-cpu-x64.zip" today,
// something else next release) — there's no stable filename to hardcode.
// The same release also ships cuda/vulkan/rocm/sycl variants; the anchored
// suffix match below deliberately excludes those since we can't assume the
// right GPU drivers are present.
async function llamaBinaryUrl(): Promise<{ url: string; archiveName: string } | null> {
  const ua = navigator.userAgent;
  const platform = (navigator as any).userAgentData?.platform || "";
  const isWindows = /Windows/i.test(ua);
  const isMac = /Mac/i.test(ua) || platform === "macOS";
  const isLinux = /Linux/i.test(ua) && !/Android/i.test(ua);

  let suffix: string;
  let ext: "zip" | "tar.gz";
  if (isWindows) {
    suffix = "win-cpu-x64";
    ext = "zip";
  } else if (isMac) {
    const isArm = /arm64|aarch64/i.test(ua) || /Mac.*ARM/i.test(platform);
    suffix = isArm ? "macos-arm64" : "macos-x64";
    ext = "tar.gz";
  } else if (isLinux) {
    suffix = "ubuntu-x64";
    ext = "tar.gz";
  } else {
    return null;
  }

  let assets: Array<{ name: string; browser_download_url: string }>;
  try {
    const res = await fetch("https://api.github.com/repos/ggml-org/llama.cpp/releases/latest");
    if (!res.ok) throw new Error(`GitHub API returned ${res.status}`);
    const data = await res.json();
    assets = data.assets || [];
  } catch (err) {
    throw new Error("Couldn't reach GitHub to find the latest llama.cpp release. Check your connection and try again.");
  }

  const re = new RegExp(`^llama-b\\d+-bin-${suffix.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\.(zip|tar\\.gz)$`);
  const asset = assets.find((a) => re.test(a.name));
  if (!asset) return null;

  return { url: asset.browser_download_url, archiveName: `_llm_download.${ext}` };
}

export type DownloadProgress = { receivedBytes: number; estimateBytes: number; phase: string };

const LLM_MODEL_STORAGE_KEY = "llmModelPath";

function loadStoredLlmModelPath(): string {
  if (typeof localStorage === "undefined") return "";
  return localStorage.getItem(LLM_MODEL_STORAGE_KEY) ?? "";
}

/**
 * Holds all player state and Tauri interaction logic as a module-level
 * singleton, so it survives SvelteKit client-side navigation. The "/" page
 * component gets destroyed and recreated every time you navigate to
 * /vocabulary and back — if this state lived in that component instead,
 * `currentFile` (and everything else) would reset to defaults on return,
 * which disables every control even though the video is still playing on
 * the Rust side. The canvas element is the one thing that's genuinely
 * per-mount (a fresh <canvas> exists only while "/" is mounted); pages
 * register/unregister it via `registerCanvas`/`unregisterCanvas`.
 */
const PAUSE_AFTER_LINE_STORAGE_KEY = "videoLearnPauseAfterLine";

function loadStoredBool(key: string): boolean {
  try {
    return typeof localStorage !== "undefined" && localStorage.getItem(key) === "1";
  } catch {
    return false;
  }
}

const EMBED_MODE_STORAGE_KEY = "videoLearnYoutubeEmbedMode";

/** Embed mode defaults to on (it needs no yt-dlp), but once the user has
 * chosen either way that choice is kept across launches. */
function loadStoredEmbedMode(): boolean {
  try {
    const stored = typeof localStorage === "undefined" ? null : localStorage.getItem(EMBED_MODE_STORAGE_KEY);
    return stored === null ? true : stored === "1";
  } catch {
    return true;
  }
}

export type SavedWord = { word: string; context: string | null; timestampMs: number | null };

/** Early-stop margin: playback status is polled, so stopping exactly at a
 * line's end would overshoot into the next line by up to a poll interval. */
const LINE_END_LEAD_MS = 100;

class PlayerStore {
  currentFile = $state<string | null>(null);
  mediaKind = $state<"local" | "youtube" | "youtube-embed">("local");
  youtubeTitle = $state<string | null>(null);
  youtubeLoading = $state(false);
  /** True while a stalled/restricted direct-stream video is being
   * downloaded (audio+video merged locally) as the fallback before Embed
   * mode — see `#downloadAndPlay`. Separate from `youtubeLoading` since the
   * UI shows a different hint for it (a real download, not just resolving
   * a URL, so it can take much longer). */
  youtubeDownloading = $state(false);
  /** Latest progress of the running download; null until the first update
   * arrives (yt-dlp is still resolving the video) and again once finished. */
  downloadProgress = $state<YoutubeDownloadProgress | null>(null);
  downloadQuality = $state<DownloadQualityOption>(loadStoredDownloadQuality());
  /** User-facing toggle: load YouTube links via our decode pipeline (full
   * features) or via YouTube's own <iframe> embed (no yt-dlp dependency,
   * but no click-to-define/custom subtitles/quality controls). Defaults to
   * the embed since it works with zero setup (no yt-dlp install needed);
   * users who want click-to-define on YouTube can switch it off per load. */
  youtubeEmbedMode = $state(loadStoredEmbedMode());
  youtubeEmbedVideoId = $state<string | null>(null);
  /** Seconds into the video the embed iframe's `start` param should begin
   * at — 0 for a fresh load, set when resuming from the library. */
  youtubeEmbedStartSeconds = $state(0);
  isPlaying = $state(false);
  positionMs = $state(0);
  durationMs = $state(0);
  errorMessage = $state("");
  /** A neutral, non-error note (e.g. "downloaded it so it can play"). */
  infoMessage = $state("");
  seeking = $state(false);
  subtitleCues = $state<SubtitleCue[]>([]);
  embeddedTracks = $state<EmbeddedSubtitleTrack[]>([]);
  activeSubtitleLabel = $state("Off");
  subtitleLoading = $state(false);
  wordPopup = $state<WordPopup | null>(null);
  volume = $state(100);
  quality = $state<QualityOption>(QUALITY_OPTIONS[0]);
  qualityLoading = $state(false);
  aspect = $state<AspectOption>(ASPECT_OPTIONS[0]);
  aspectLoading = $state(false);
  videoWidth = $state(16);
  videoHeight = $state(9);
  subtitleFontSize = $state(loadStoredFontSize());
  subtitleFontFamily = $state(loadStoredFontFamily());
  subtitleBgAlpha = $state(loadStoredSubtitleBgAlpha());
  subtitleOutline = $state<SubtitleOutline>(loadStoredSubtitleOutline());
  subtitleTextColor = $state(loadStoredOption(SUBTITLE_TEXT_COLOR_STORAGE_KEY, SUBTITLE_TEXT_COLORS.map((o) => o.value), ""));
  subtitleBgColor = $state(loadStoredOption(SUBTITLE_BG_COLOR_STORAGE_KEY, SUBTITLE_BG_COLORS.map((o) => o.value), "#0d0b09"));
  subtitleWeight = $state(Number(loadStoredOption(SUBTITLE_WEIGHT_STORAGE_KEY, SUBTITLE_WEIGHT_OPTIONS.map((o) => String(o.weight)), "500")));
  subtitlePosition = $state<SubtitlePosition>(loadStoredOption(SUBTITLE_POSITION_STORAGE_KEY, SUBTITLE_POSITION_OPTIONS, "Bottom") as SubtitlePosition);
  /** +ms delays subtitles relative to the video, -ms advances them — for
   * files/tracks whose timing drifts out of sync. Not persisted: it's a
   * per-file fixup, not a durable preference. */
  subtitleOffsetMs = $state(0);
  playbackRate = $state(loadStoredPlaybackRate());

  /** Study-mode practice aids. "Loop this line" repeats the line that was
   * current when it was switched on (or the one you jump to); "pause after
   * each line" stops playback at the end of every subtitle line so the
   * learner can tap words before moving on. */
  loopLine = $state(false);
  pauseAfterLine = $state(loadStoredBool(PAUSE_AFTER_LINE_STORAGE_KEY));
  /** Words saved from the video that is currently loaded (lowercased
   * lookup lives in the page) — drives the highlighter in subtitles and
   * transcript and the "Saved from this video" strip. */
  savedWords = $state<SavedWord[]>([]);
  #loopIdx = -1;
  #pausedForIdx = -1;

  llmStatus = $state<LlmStatus | null>(null);
  llmModelPath = $state(loadStoredLlmModelPath());
  llmSetupError = $state("");
  llmBinaryInstalling = $state(false);
  llmBinaryProgress = $state<DownloadProgress>({ receivedBytes: 0, estimateBytes: 0, phase: "" });
  llmModelInstalling = $state<Record<string, DownloadProgress>>({});
  llmCustomUrl = $state("");
  llmCustomInstalling = $state(false);
  llmCustomProgress = $state<DownloadProgress>({ receivedBytes: 0, estimateBytes: 0, phase: "" });

  #lastDownloadError = "";
  #canvasEl: HTMLCanvasElement | null = null;
  #ctx: CanvasRenderingContext2D | null = null;
  #decoding = false;
  #subscribed = false;
  #pollInterval: ReturnType<typeof setInterval> | null = null;
  /** Recent-library id (path/URL) of whatever is currently loaded, so the
   * status poll below knows which entry to keep progress-updated. */
  #recentId: string | null = null;
  #lastRecentSaveAt = 0;

  /** Called by the "/" page's onMount with its fresh <canvas>. */
  registerCanvas(el: HTMLCanvasElement) {
    this.#canvasEl = el;
    this.#ctx = el.getContext("2d");
    if (this.currentFile) {
      el.width = this.videoWidth;
      el.height = this.videoHeight;
    }
    this.#ensureSubscribed();
    this.#ensurePolling();
    this.refreshLlmStatus();
  }

  /** Called by the "/" page's onDestroy so a stale canvas isn't drawn to. */
  unregisterCanvas(el: HTMLCanvasElement) {
    if (this.#canvasEl === el) {
      this.#canvasEl = null;
      this.#ctx = null;
    }
  }

  #ensureSubscribed() {
    if (this.#subscribed) return;
    this.#subscribed = true;

    const frameChannel = new Channel<ArrayBuffer>();
    frameChannel.onmessage = (data) => {
      // Drop frames that arrive while the previous one is still decoding,
      // or while no page is currently displaying the canvas, rather than
      // queuing, so a slow moment (or a navigated-away page) never builds
      // up a backlog.
      if (!this.#ctx || !this.#canvasEl || this.#decoding) return;
      this.#decoding = true;
      const canvasEl = this.#canvasEl;
      const ctx = this.#ctx;
      const blob = new Blob([data], { type: "image/jpeg" });
      createImageBitmap(blob)
        .then((bitmap) => {
          ctx.drawImage(bitmap, 0, 0, canvasEl.width, canvasEl.height);
          bitmap.close();
        })
        .catch(() => {})
        .finally(() => {
          this.#decoding = false;
        });
    };
    invoke("subscribe_video_frames", { channel: frameChannel });
  }

  #ensurePolling() {
    if (this.#pollInterval) return;
    this.#pollInterval = setInterval(async () => {
      // The embed mode has no backend player behind it at all — polling
      // get_status here would just report stale state from whatever was
      // loaded before switching into embed mode.
      if (!this.currentFile || this.seeking || this.mediaKind === "youtube-embed") return;
      try {
        const status = await invoke<PlaybackStatus>("get_status");
        this.isPlaying = status.is_playing;
        this.positionMs = status.position_ms;
        this.durationMs = status.duration_ms;
        this.volume = status.volume;
        void this.#applyLineBehaviours();

        // Throttled so playback progress doesn't hit localStorage 5x/sec —
        // the library's "resume where you left off" only needs to be
        // accurate to within a few seconds.
        const now = Date.now();
        if (this.#recentId && now - this.#lastRecentSaveAt > 4000) {
          this.#lastRecentSaveAt = now;
          updateRecentProgress(this.#recentId, this.positionMs, this.durationMs);
        }
      } catch {
        // player not ready yet
      }
    }, 200);
  }

  /** Polls `get_status` until playback has actually started, then seeks —
   * calling `seek_ms` immediately after a fresh `load_file`/`load_youtube`
   * can be silently dropped by libVLC if it hasn't reached the Playing
   * state yet (the same race `reload_current_media` guards against on the
   * Rust side for quality/aspect changes). Only relevant when resuming a
   * library entry at a non-zero position. */
  async #seekOnceReady(positionMs: number) {
    if (positionMs <= 0) return;
    const deadline = Date.now() + 3000;
    while (Date.now() < deadline) {
      try {
        const status = await invoke<PlaybackStatus>("get_status");
        if (status.is_playing) break;
      } catch {
        // not ready yet
      }
      await new Promise((r) => setTimeout(r, 150));
    }
    try {
      await invoke("seek_ms", { positionMs });
      this.positionMs = positionMs;
    } catch {
      // best-effort resume — not fatal if it doesn't land
    }
  }

  /** `titleOverride` is a human title to show instead of the file name (a
   * downloaded YouTube video's file is just named after its video id).
   * `youtubeUrl` marks the file as a cached download of that link: the
   * Library entry is then keyed by the URL, not the cache path, so it keeps
   * working — and re-downloads — if the cached file is ever deleted. */
  async #loadLocalFile(path: string, resumeMs = 0, titleOverride?: string, youtubeUrl?: string) {
    this.errorMessage = "";
    this.infoMessage = "";
    try {
      const response = await invoke<LoadFileResponse>("load_file", { path });
      this.currentFile = path;
      this.mediaKind = "local";
      this.youtubeTitle = titleOverride ?? null;
      this.youtubeEmbedVideoId = null;
      this.isPlaying = true;
      this.durationMs = response.duration_ms;
      this.subtitleCues = response.subtitles;
      this.embeddedTracks = [];
      this.activeSubtitleLabel =
        response.subtitles.length > 0 ? (youtubeUrl ? "English (YouTube captions)" : "External file") : "Off";
      this.subtitleOffsetMs = 0;
      this.videoWidth = response.width;
      this.videoHeight = response.height;
      if (this.#canvasEl) {
        this.#canvasEl.width = response.width;
        this.#canvasEl.height = response.height;
      }

      if (youtubeUrl) {
        // Drop any entry an older run saved under the cache file path.
        removeRecent(path);
        this.#recentId = youtubeUrl;
        upsertRecent({
          id: youtubeUrl,
          kind: "youtube",
          path: youtubeUrl,
          title: titleOverride || youtubeUrl,
          positionMs: resumeMs,
          durationMs: response.duration_ms,
        });
      } else {
        this.#recentId = path;
        upsertRecent({
          id: path,
          kind: "local",
          path,
          title: titleOverride || baseName(path),
          positionMs: resumeMs,
          durationMs: response.duration_ms,
        });
      }

      // Runs after playback has already started: for large files without a
      // Matroska SeekHead this can take a moment, so it must not block
      // opening the video.
      invoke<EmbeddedSubtitleTrack[]>("list_embedded_subtitle_tracks", { path })
        .then((tracks) => {
          if (this.currentFile === path) this.embeddedTracks = tracks;
        })
        .catch(() => {});

      await this.#seekOnceReady(resumeMs);
      await this.#applyStoredPlaybackRate();
    } catch (err) {
      this.errorMessage = String(err);
    }
  }

  async pickFile() {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Video", extensions: ["mp4", "mkv", "avi", "mov", "webm"] }],
    });
    if (!selected || Array.isArray(selected)) return;
    await this.#loadLocalFile(selected);
  }

  /** Entry point for opening a video dropped onto the window (Tauri's own
   * OS-level drag-drop, not HTML5 DnD — see the "/" page's onDragDropEvent
   * listener), as an alternative to the file-picker dialog in `pickFile`. */
  async openDroppedPath(path: string) {
    await this.#loadLocalFile(path);
  }

  /** Unloads whatever is open and returns to the start screen ("home").
   * The player and the start screen share the "/" route (the screen shown
   * depends on `currentFile`), so going home means clearing that state —
   * there's no separate route to navigate to. Progress is saved first so the
   * library's "resume where you left off" is accurate. */
  closeVideo() {
    if (!this.currentFile) return;
    if (this.#recentId && this.durationMs > 0) {
      updateRecentProgress(this.#recentId, this.positionMs, this.durationMs);
    }
    // Fire-and-forget: navigation must never wait on (or be blocked by) the
    // backend, so the UI state is cleared synchronously below.
    invoke("pause").catch(() => {
      // nothing was loaded in the backend player — fine
    });
    this.currentFile = null;
    this.mediaKind = "local";
    this.youtubeTitle = null;
    this.youtubeEmbedVideoId = null;
    this.youtubeEmbedStartSeconds = 0;
    this.isPlaying = false;
    this.positionMs = 0;
    this.durationMs = 0;
    this.errorMessage = "";
    this.infoMessage = "";
    this.subtitleCues = [];
    this.embeddedTracks = [];
    this.activeSubtitleLabel = "Off";
    this.subtitleOffsetMs = 0;
    this.wordPopup = null;
    this.loopLine = false;
    this.savedWords = [];
    this.#recentId = null;
  }

  /** Opens `trimmed` via YouTube's own `<iframe>` embed — no backend decode
   * pipeline involved at all. Shared by the normal embed-mode path and the
   * automatic fallback when a direct-stream load turns out to be
   * unplayable (see RESTRICTED_STREAM_ERROR below). Returns false only if
   * the URL itself couldn't be recognized as YouTube. */
  async #openEmbed(trimmed: string, resumeMs: number): Promise<boolean> {
    const videoId = extractYoutubeVideoId(trimmed);
    if (!videoId) {
      this.errorMessage = "Couldn't recognize that as a YouTube link.";
      return false;
    }
    // The embed is a completely separate iframe player with no connection
    // to our decode pipeline, so stop whatever the backend player (a local
    // file or a previous direct-stream YouTube load) was doing rather than
    // leaving it playing silently underneath.
    try {
      await invoke("pause");
    } catch {
      // nothing was loaded in the backend player — fine
    }
    this.currentFile = trimmed;
    this.mediaKind = "youtube-embed";
    this.youtubeTitle = null;
    this.youtubeEmbedVideoId = videoId;
    this.youtubeEmbedStartSeconds = Math.floor(resumeMs / 1000);
    this.isPlaying = false;
    this.subtitleCues = [];
    this.embeddedTracks = [];
    this.activeSubtitleLabel = "Off";
    this.subtitleOffsetMs = 0;
    this.wordPopup = null;

    this.#recentId = trimmed;
    upsertRecent({ id: trimmed, kind: "youtube", path: trimmed, title: trimmed, positionMs: resumeMs, durationMs: 0 });
    return true;
  }

  async #loadYoutube(trimmed: string, resumeMs = 0) {
    this.errorMessage = "";
    this.infoMessage = "";

    if (this.youtubeEmbedMode) {
      await this.#openEmbed(trimmed, resumeMs);
      return;
    }

    this.youtubeLoading = true;
    try {
      const response = await invoke<LoadFileResponse>("load_youtube", { url: trimmed });
      this.currentFile = trimmed;
      this.mediaKind = "youtube";
      this.youtubeEmbedVideoId = null;
      this.youtubeTitle = response.title;
      this.isPlaying = true;
      this.durationMs = response.duration_ms;
      this.subtitleCues = [];
      this.embeddedTracks = [];
      this.activeSubtitleLabel = "Off";
      this.subtitleOffsetMs = 0;
      this.videoWidth = response.width;
      this.videoHeight = response.height;
      if (this.#canvasEl) {
        this.#canvasEl.width = response.width;
        this.#canvasEl.height = response.height;
      }

      this.#recentId = trimmed;
      upsertRecent({
        id: trimmed,
        kind: "youtube",
        path: trimmed,
        title: response.title || trimmed,
        positionMs: resumeMs,
        durationMs: response.duration_ms,
      });

      // Resolving a YouTube URL already involves one yt-dlp round trip;
      // listing captions is a separate one, so it must not block playback
      // starting.
      invoke<EmbeddedSubtitleTrack[]>("list_youtube_captions")
        .then((tracks) => {
          if (this.currentFile === trimmed) this.embeddedTracks = tracks;
        })
        .catch(() => {});

      await this.#seekOnceReady(resumeMs);
      await this.#applyStoredPlaybackRate();
      this.#watchForStallAndFallback(trimmed, resumeMs);
    } catch (err) {
      if (String(err) === RESTRICTED_STREAM_ERROR) {
        // The backend already confirmed (via a Range probe) that YouTube's
        // CDN will cut this stream off partway through — no point letting
        // the user hit a stalled video before finding out.
        await this.#fallbackFromDirectStream(trimmed, resumeMs);
      } else {
        this.errorMessage = String(err);
      }
    } finally {
      this.youtubeLoading = false;
    }
  }

  /** Backstop for the cases the upfront probe in `load_youtube` can't catch:
   * YouTube's CDN cutoff for a restricted stream isn't a fixed byte offset
   * every time, so a single one-shot probe can pass a stream that then
   * stalls anyway once real sustained playback starts hitting it with many
   * more requests. This watches actual playback progress for a few seconds
   * after a load that *looked* successful, and falls back if the position
   * never moves — instead of leaving a stalled black frame on screen with
   * no explanation, which is what prompted this in the first place. */
  #watchForStallAndFallback(trimmed: string, resumeMs: number) {
    const startPositionMs = this.positionMs;
    const STALL_CHECK_DELAY_MS = 6000;
    const STALL_PROGRESS_THRESHOLD_MS = 500;
    setTimeout(async () => {
      // Bail if a different video/file has since loaded, or embed mode was
      // manually switched to in the meantime — this check is stale either way.
      if (this.mediaKind !== "youtube" || this.currentFile !== trimmed) return;
      if (this.positionMs > startPositionMs + STALL_PROGRESS_THRESHOLD_MS) return;

      await this.#fallbackFromDirectStream(trimmed, resumeMs);
    }, STALL_CHECK_DELAY_MS);
  }

  /** Direct-stream turned out to be unplayable (probe-caught or stalled
   * mid-playback) — tries downloading the video properly (separate
   * audio+video, merged locally) before giving up on our own decode
   * pipeline entirely and falling back to YouTube's own embedded player.
   * Downloading is slower to start than either of those but, unlike
   * direct-stream, keeps click-to-define/custom subtitles/quality controls
   * working once it does. */
  async #fallbackFromDirectStream(trimmed: string, resumeMs: number) {
    if (await this.#downloadAndPlay(trimmed, resumeMs)) {
      // Playback worked, so this is a heads-up, not an error.
      this.infoMessage = "Downloaded this video so it plays in full — YouTube doesn't allow direct streaming for it.";
      return;
    }
    const opened = await this.#openEmbed(trimmed, resumeMs);
    const detail = this.#lastDownloadError ? ` (${this.#lastDownloadError})` : "";
    this.errorMessage = opened
      ? `This video can't stream directly or be downloaded (YouTube limits it) — switched to Embed mode.${detail}`
      : "";
  }

  /** Downloads `trimmed` via yt-dlp's normal separate-streams+ffmpeg-merge
   * path (see `download_youtube_video` on the Rust side) and plays the
   * result through the same local-file pipeline as any other video. Returns
   * false on any failure (missing ffmpeg, network error, etc.) so the
   * caller can fall further back to Embed mode. */
  async #downloadAndPlay(trimmed: string, resumeMs: number): Promise<boolean> {
    const videoId = extractYoutubeVideoId(trimmed);
    if (!videoId) return false;

    this.youtubeDownloading = true;
    this.downloadProgress = null;
    this.#lastDownloadError = "";

    // Streams yt-dlp's progress (per stream: video, audio, then the merge).
    const progress = new Channel<YoutubeDownloadProgress>();
    progress.onmessage = (p) => {
      this.downloadProgress = p;
    };

    try {
      const result = await invoke<{ path: string; title: string }>("download_youtube_video", {
        url: trimmed,
        videoId,
        // 0 = Auto -> no explicit height; the backend picks one.
        qualityHeight: this.downloadQuality.height || null,
        onProgress: progress,
      });
      await this.#loadLocalFile(result.path, resumeMs, result.title, trimmed);
      return true;
    } catch (err) {
      this.#lastDownloadError = String(err);
      return false;
    } finally {
      this.youtubeDownloading = false;
      this.downloadProgress = null;
    }
  }

  /** Turns "use YouTube's own player" on or off, and remembers the choice. */
  setYoutubeEmbedMode(on: boolean) {
    this.youtubeEmbedMode = on;
    try {
      localStorage.setItem(EMBED_MODE_STORAGE_KEY, on ? "1" : "0");
    } catch {
      // a preference only — fine if storage is unavailable
    }
  }

  /** From the embed player's notice: reopens the video that's showing in
   * YouTube's own player in this app's player instead, where subtitle words
   * can be tapped for meanings. Falls back to the embed again (with the
   * reason) if the video can't be streamed or downloaded. */
  async openWithWordLookup() {
    const url = this.currentFile;
    if (!url || this.mediaKind !== "youtube-embed" || this.youtubeLoading || this.youtubeDownloading) return;
    this.setYoutubeEmbedMode(false);
    await this.#loadYoutube(url, this.youtubeEmbedStartSeconds * 1000);
  }

  setDownloadQuality(option: DownloadQualityOption) {
    this.downloadQuality = option;
    try {
      localStorage.setItem(DOWNLOAD_QUALITY_STORAGE_KEY, String(option.height));
    } catch {
      // a preference only — fine if storage is unavailable
    }
  }

  async loadYoutubeUrl(url: string) {
    const trimmed = url.trim();
    if (!trimmed || this.youtubeLoading || this.youtubeDownloading) return;
    await this.#loadYoutube(trimmed, 0);
  }

  /** Entry point for the library page: reopens a previously-opened local
   * file or YouTube link and resumes at its saved position. */
  async resumeRecent(item: RecentItem) {
    if (item.kind === "local") {
      await this.#loadLocalFile(item.path, item.positionMs);
    } else {
      await this.#loadYoutube(item.path, item.positionMs);
    }
  }

  async pickExternalSubtitles(closeMenu: () => void) {
    this.errorMessage = "";
    closeMenu();
    const selected = await open({
      multiple: false,
      filters: [{ name: "Subtitles", extensions: ["srt", "vtt"] }],
    });
    if (!selected || Array.isArray(selected)) return;

    try {
      this.subtitleCues = await invoke<SubtitleCue[]>("load_subtitles", { path: selected });
      this.activeSubtitleLabel = "External file";
      this.subtitleOffsetMs = 0;
    } catch (err) {
      this.errorMessage = String(err);
    }
  }

  async selectEmbeddedTrack(track: EmbeddedSubtitleTrack, closeMenu: () => void) {
    if (!this.currentFile) return;
    this.errorMessage = "";
    closeMenu();
    // The first selection for a local file has to scan the whole container
    // and can take several seconds; later selections for the same file are
    // served from a cache and resolve near-instantly. YouTube caption
    // tracks are already resolved URLs by this point, so those are quick.
    this.subtitleLoading = true;
    try {
      this.subtitleCues =
        this.mediaKind === "youtube"
          ? await invoke<SubtitleCue[]>("load_youtube_caption", { trackId: track.id })
          : await invoke<SubtitleCue[]>("load_embedded_subtitle_track", {
              path: this.currentFile,
              trackId: track.id,
            });
      this.activeSubtitleLabel = trackLabel(track);
      this.subtitleOffsetMs = 0;
    } catch (err) {
      this.errorMessage = String(err);
    } finally {
      this.subtitleLoading = false;
    }
  }

  turnSubtitlesOff(closeMenu: () => void) {
    this.subtitleCues = [];
    this.activeSubtitleLabel = "Off";
    this.subtitleOffsetMs = 0;
    closeMenu();
  }

  nudgeSubtitleOffset(deltaMs: number) {
    this.subtitleOffsetMs += deltaMs;
  }

  resetSubtitleOffset() {
    this.subtitleOffsetMs = 0;
  }

  async changeQuality(option: QualityOption, closeMenu: () => void) {
    if (!this.currentFile || this.qualityLoading) return;
    closeMenu();
    if (option.maxDimension === this.quality.maxDimension) return;

    this.qualityLoading = true;
    this.errorMessage = "";
    try {
      const response = await invoke<{ width: number; height: number }>("set_video_quality", {
        maxDimension: option.maxDimension,
      });
      this.quality = option;
      this.videoWidth = response.width;
      this.videoHeight = response.height;
      if (this.#canvasEl) {
        this.#canvasEl.width = response.width;
        this.#canvasEl.height = response.height;
      }
    } catch (err) {
      this.errorMessage = String(err);
    } finally {
      this.qualityLoading = false;
    }
  }

  async changeAspect(option: AspectOption, closeMenu: () => void) {
    if (!this.currentFile || this.aspectLoading) return;
    closeMenu();
    if (option.num === this.aspect.num && option.den === this.aspect.den) return;

    this.aspectLoading = true;
    this.errorMessage = "";
    try {
      const response = await invoke<{ width: number; height: number }>("set_aspect_ratio", {
        num: option.num,
        den: option.den,
      });
      this.aspect = option;
      this.videoWidth = response.width;
      this.videoHeight = response.height;
      if (this.#canvasEl) {
        this.#canvasEl.width = response.width;
        this.#canvasEl.height = response.height;
      }
    } catch (err) {
      this.errorMessage = String(err);
    } finally {
      this.aspectLoading = false;
    }
  }

  setSubtitleFontSize(rem: number, closeMenu?: () => void) {
    this.subtitleFontSize = rem;
    localStorage.setItem(FONT_SIZE_STORAGE_KEY, String(rem));
    closeMenu?.();
  }

  setSubtitleFontFamily(family: string, closeMenu?: () => void) {
    this.subtitleFontFamily = family;
    localStorage.setItem(FONT_FAMILY_STORAGE_KEY, family);
    closeMenu?.();
  }

  setSubtitleBgAlpha(alpha: number, closeMenu?: () => void) {
    this.subtitleBgAlpha = alpha;
    localStorage.setItem(SUBTITLE_BG_STORAGE_KEY, String(alpha));
    closeMenu?.();
  }

  setSubtitleTextColor(value: string) {
    this.subtitleTextColor = value;
    writeStored(SUBTITLE_TEXT_COLOR_STORAGE_KEY, value);
  }

  setSubtitleBgColor(value: string) {
    this.subtitleBgColor = value;
    writeStored(SUBTITLE_BG_COLOR_STORAGE_KEY, value);
  }

  setSubtitleWeight(weight: number) {
    this.subtitleWeight = weight;
    writeStored(SUBTITLE_WEIGHT_STORAGE_KEY, String(weight));
  }

  setSubtitlePosition(position: SubtitlePosition) {
    this.subtitlePosition = position;
    writeStored(SUBTITLE_POSITION_STORAGE_KEY, position);
  }

  /** Applies a whole look at once (colors, background, outline, weight); size,
   * font and position are left as the user set them. */
  applySubtitlePreset(preset: (typeof SUBTITLE_PRESETS)[number]) {
    this.setSubtitleTextColor(preset.textColor);
    this.setSubtitleBgColor(preset.bgColor);
    this.setSubtitleBgAlpha(preset.bgAlpha);
    this.setSubtitleOutline(preset.outline as SubtitleOutline);
    this.setSubtitleWeight(preset.weight);
  }

  /** Back to everything default, including size, font and position. */
  resetSubtitleStyle() {
    this.applySubtitlePreset(SUBTITLE_PRESETS[0]);
    this.setSubtitleFontSize(DEFAULT_FONT_SIZE_REM);
    this.setSubtitleFontFamily(DEFAULT_FONT_FAMILY);
    this.setSubtitlePosition("Bottom");
  }

  setSubtitleOutline(outline: SubtitleOutline, closeMenu?: () => void) {
    this.subtitleOutline = outline;
    localStorage.setItem(SUBTITLE_OUTLINE_STORAGE_KEY, outline);
    closeMenu?.();
  }

  async refreshLlmStatus() {
    try {
      this.llmStatus = await invoke<LlmStatus>("llm_status");
    } catch {
      this.llmStatus = null;
    }
  }

  isLlmReady(): boolean {
    return !!this.llmStatus && this.llmStatus.binary_exists && this.llmStatus.models.length > 0;
  }

  currentLlmModel(): LlmModel | null {
    if (!this.isLlmReady()) return null;
    const models = this.llmStatus!.models;
    return models.find((m) => m.path === this.llmModelPath) ?? models[0];
  }

  setLlmModelPath(path: string) {
    this.llmModelPath = path;
    localStorage.setItem(LLM_MODEL_STORAGE_KEY, path);
  }

  /** Asks the local model to explain the popped-up word as used in its
   * subtitle sentence — the context-aware counterpart to the plain
   * dictionary lookup that already ran when the popup opened. */
  async explainInContext() {
    if (!this.wordPopup || this.wordPopup.aiLoading) return;
    const model = this.currentLlmModel();
    if (!model) return;
    const popup = this.wordPopup;
    this.wordPopup = { ...popup, aiLoading: true, aiError: "", aiText: "" };
    try {
      const text = await invoke<string>("llm_explain", {
        word: popup.word,
        sentence: popup.sentence,
        modelPath: model.path,
      });
      if (this.wordPopup && this.wordPopup.word === popup.word) {
        this.wordPopup = { ...this.wordPopup, aiLoading: false, aiText: text };
      }
    } catch (err) {
      if (this.wordPopup && this.wordPopup.word === popup.word) {
        this.wordPopup = { ...this.wordPopup, aiLoading: false, aiError: String(err) };
      }
    }
  }

  /** Asks the local model for a few example sentences of the popped-up word
   * in other senses/contexts — the counterpart to `explainInContext` for
   * viewers who want a feel for a word beyond its one occurrence here. */
  async showWordExamples() {
    if (!this.wordPopup || this.wordPopup.examplesLoading) return;
    const model = this.currentLlmModel();
    if (!model) return;
    const popup = this.wordPopup;
    this.wordPopup = { ...popup, examplesLoading: true, examplesError: "", examplesText: "" };
    try {
      const text = await invoke<string>("llm_word_examples", {
        word: popup.word,
        sentence: popup.sentence,
        modelPath: model.path,
      });
      if (this.wordPopup && this.wordPopup.word === popup.word) {
        this.wordPopup = { ...this.wordPopup, examplesLoading: false, examplesText: text };
      }
    } catch (err) {
      if (this.wordPopup && this.wordPopup.word === popup.word) {
        this.wordPopup = { ...this.wordPopup, examplesLoading: false, examplesError: String(err) };
      }
    }
  }

  // ---- LLM setup: download the llama-cli binary + GGUF models ----

  /** Downloads `url` into `<llm_root>/<relPath>` while polling the
   * destination file's size every 500ms to report live progress — we don't
   * know the total size up front, so the caller supplies an estimate for a
   * percentage bar (falls back to a bytes-only readout without one). */
  async #downloadWithProgress(url: string, relPath: string, estimateMb: number, onProgress: (p: DownloadProgress) => void) {
    const estimateBytes = estimateMb > 0 ? estimateMb * 1024 * 1024 : 0;
    let cancelled = false;
    const poll = setInterval(async () => {
      if (cancelled) return;
      try {
        const size = await invoke<number>("llm_file_size", { relPath });
        onProgress({ receivedBytes: size, estimateBytes, phase: "Downloading" });
      } catch {
        // file not written yet
      }
    }, 500);

    try {
      const finalSize = await invoke<number>("llm_download_file", { url, relPath });
      onProgress({ receivedBytes: finalSize, estimateBytes: finalSize, phase: "Done" });
    } finally {
      cancelled = true;
      clearInterval(poll);
    }
  }

  async installLlmBinary() {
    if (this.llmBinaryInstalling) return;
    this.llmSetupError = "";
    this.llmBinaryInstalling = true;
    this.llmBinaryProgress = { receivedBytes: 0, estimateBytes: 40 * 1024 * 1024, phase: "Resolving latest release…" };
    try {
      const spec = await llamaBinaryUrl();
      if (!spec) {
        throw new Error("No prebuilt llama.cpp binary found for your platform — install manually.");
      }
      await this.#downloadWithProgress(spec.url, spec.archiveName, 40, (p) => (this.llmBinaryProgress = p));
      this.llmBinaryProgress = { ...this.llmBinaryProgress, phase: "Extracting" };
      await invoke("llm_extract_downloaded_archive", { archiveName: spec.archiveName });
      this.llmBinaryProgress = { ...this.llmBinaryProgress, phase: "Done" };
    } catch (err) {
      this.llmSetupError = String(err);
    } finally {
      this.llmBinaryInstalling = false;
      await this.refreshLlmStatus();
    }
  }

  async installLlmModel(model: CatalogModel) {
    if (this.llmModelInstalling[model.id]) return;
    this.llmSetupError = "";
    this.llmModelInstalling[model.id] = { receivedBytes: 0, estimateBytes: model.sizeMb * 1024 * 1024, phase: "Downloading" };
    try {
      await this.#downloadWithProgress(modelGgufUrl(model), `models/${model.file}`, model.sizeMb, (p) => {
        this.llmModelInstalling[model.id] = p;
      });
    } catch (err) {
      this.llmSetupError = `${model.label}: ${String(err)}`;
    } finally {
      const { [model.id]: _removed, ...rest } = this.llmModelInstalling;
      this.llmModelInstalling = rest;
      await this.refreshLlmStatus();
    }
  }

  /** Extracts a safe destination filename from a user-pasted URL: the last
   * path segment, URL-decoded, validated as a plain "name.gguf" with no
   * path separators — this becomes `rel_path` for `llm_download_file`,
   * which joins it straight onto the llm root on the Rust side, so an
   * unsanitized "../../something" here would let a pasted URL write
   * outside the models folder. */
  #ggufFilenameFromUrl(rawUrl: string): string {
    let u: URL;
    try {
      u = new URL(rawUrl);
    } catch {
      throw new Error("Enter a valid URL.");
    }
    if (u.protocol !== "https:" && u.protocol !== "http:") {
      throw new Error("URL must start with http:// or https://.");
    }
    const segments = u.pathname.split("/").filter(Boolean);
    const last = decodeURIComponent(segments[segments.length - 1] || "");
    if (!/^[^\\/]+\.gguf$/i.test(last)) {
      throw new Error("URL must point directly to a .gguf file.");
    }
    return last;
  }

  async installLlmCustomModel() {
    const rawUrl = this.llmCustomUrl.trim();
    if (!rawUrl || this.llmCustomInstalling) return;
    let filename: string;
    try {
      filename = this.#ggufFilenameFromUrl(rawUrl);
    } catch (err) {
      this.llmSetupError = String((err as Error)?.message ?? err);
      return;
    }

    this.llmSetupError = "";
    this.llmCustomInstalling = true;
    this.llmCustomProgress = { receivedBytes: 0, estimateBytes: 0, phase: "Starting…" };
    try {
      await this.#downloadWithProgress(rawUrl, `models/${filename}`, 0, (p) => (this.llmCustomProgress = p));
      this.llmCustomProgress = { ...this.llmCustomProgress, phase: "Done" };
      this.llmCustomUrl = "";
    } catch (err) {
      this.llmSetupError = `${filename}: ${String(err)}`;
    } finally {
      this.llmCustomInstalling = false;
      await this.refreshLlmStatus();
    }
  }

  async togglePlay() {
    // In embed mode YouTube's own iframe player owns play/pause; there's
    // no backend media loaded for these keyboard shortcuts to act on.
    if (!this.currentFile || this.mediaKind === "youtube-embed") return;
    try {
      this.isPlaying = await invoke<boolean>("toggle_play");
    } catch (err) {
      this.errorMessage = String(err);
    }
  }

  async changeVolume(delta: number) {
    if (!this.currentFile || this.mediaKind === "youtube-embed") return;
    const next = Math.max(0, Math.min(100, this.volume + delta));
    this.volume = next;
    try {
      await invoke("set_volume", { volume: next });
    } catch (err) {
      this.errorMessage = String(err);
    }
  }

  async seekBy(deltaMs: number) {
    if (!this.currentFile || this.mediaKind === "youtube-embed") return;
    const next = Math.max(0, Math.min(this.durationMs || 0, this.positionMs + deltaMs));
    this.positionMs = next;
    this.seeking = true;
    try {
      await invoke("seek_ms", { positionMs: next });
    } catch (err) {
      this.errorMessage = String(err);
    } finally {
      this.seeking = false;
    }
  }

  async seekTo(positionMs: number) {
    this.#loopIdx = -1;
    this.#pausedForIdx = -1;
    await this.#seekRaw(positionMs);
  }

  async #seekRaw(positionMs: number) {
    if (!this.currentFile || this.mediaKind === "youtube-embed") return;
    const clamped = Math.max(0, Math.min(this.durationMs || 0, positionMs));
    this.positionMs = clamped;
    try {
      await invoke("seek_ms", { positionMs: clamped });
    } catch (err) {
      this.errorMessage = String(err);
    }
  }

  toggleLoopLine() {
    this.loopLine = !this.loopLine;
    this.#loopIdx = this.loopLine ? this.#cueIndexAt(this.positionMs) : -1;
  }

  setPauseAfterLine(value: boolean) {
    this.pauseAfterLine = value;
    this.#pausedForIdx = -1;
    try {
      localStorage.setItem(PAUSE_AFTER_LINE_STORAGE_KEY, value ? "1" : "0");
    } catch {
      // preference just won't persist
    }
  }

  async #applyLineBehaviours() {
    if (!this.isPlaying || this.wordPopup || this.seeking || this.subtitleCues.length === 0) return;
    if (!this.loopLine && !this.pauseAfterLine) return;
    const idx = this.#cueIndexAt(this.positionMs);
    if (idx < 0) return;
    const adjusted = this.positionMs - this.subtitleOffsetMs;

    if (this.loopLine) {
      if (this.#loopIdx < 0 || this.#loopIdx >= this.subtitleCues.length) this.#loopIdx = idx;
      const loopCue = this.subtitleCues[this.#loopIdx];
      if (adjusted >= loopCue.end_ms - LINE_END_LEAD_MS) {
        await this.#seekRaw(loopCue.start_ms + this.subtitleOffsetMs);
      }
      return;
    }

    const cue = this.subtitleCues[idx];
    if (adjusted >= cue.end_ms - LINE_END_LEAD_MS && this.#pausedForIdx !== idx) {
      this.#pausedForIdx = idx;
      try {
        await invoke("pause");
        this.isPlaying = false;
      } catch (err) {
        this.errorMessage = String(err);
      }
    }
  }

  /** Reloads the list of words saved from whatever is loaded right now. */
  async refreshSavedWords() {
    const source = this.currentFile;
    if (!source) {
      this.savedWords = [];
      return;
    }
    try {
      const all = await invoke<
        { word: string; context_sentence: string | null; timestamp_ms: number | null; source_video: string | null }[]
      >("list_vocabulary");
      if (this.currentFile !== source) return;
      this.savedWords = all
        .filter((e) => e.source_video === source)
        .map((e) => ({ word: e.word, context: e.context_sentence, timestampMs: e.timestamp_ms }));
    } catch {
      // leave the previous list in place
    }
  }

  onSeekInput(value: number) {
    this.positionMs = value;
  }

  async onSeekCommit(value: number) {
    this.seeking = false;
    await this.seekTo(value);
  }

  /** Index of the last cue whose start has already passed `positionMs`
   * (accounting for the sync offset), or -1 before the first cue / when
   * there are no cues. Cues are assumed sorted ascending by start_ms, as
   * every subtitle source (SRT/VTT parser, MKV demux, YouTube captions)
   * already produces them. */
  #cueIndexAt(positionMs: number): number {
    const adjusted = positionMs - this.subtitleOffsetMs;
    let idx = -1;
    for (let i = 0; i < this.subtitleCues.length; i++) {
      if (this.subtitleCues[i].start_ms <= adjusted) idx = i;
      else break;
    }
    return idx;
  }

  /** Jumps to the previous subtitle line's start — or, if we're more than
   * 1.5s into the current line, restarts that line instead of skipping past
   * it, mirroring "previous track" behavior in music players. */
  async jumpToPreviousCue() {
    if (!this.currentFile || this.mediaKind === "youtube-embed" || this.subtitleCues.length === 0) return;
    const idx = this.#cueIndexAt(this.positionMs);
    if (idx < 0) return;
    const RESTART_THRESHOLD_MS = 1500;
    const cue = this.subtitleCues[idx];
    const intoLine = this.positionMs - this.subtitleOffsetMs - cue.start_ms;
    const target = idx > 0 && intoLine <= RESTART_THRESHOLD_MS ? this.subtitleCues[idx - 1] : cue;
    await this.seekTo(target.start_ms + this.subtitleOffsetMs);
  }

  async jumpToNextCue() {
    if (!this.currentFile || this.mediaKind === "youtube-embed" || this.subtitleCues.length === 0) return;
    const next = this.subtitleCues[this.#cueIndexAt(this.positionMs) + 1];
    if (!next) return;
    await this.seekTo(next.start_ms + this.subtitleOffsetMs);
  }

  /** Rewinds to the start of whichever line is currently showing — for
   * re-listening/shadowing a line without hunting for its exact start. */
  async replayCurrentCue() {
    if (!this.currentFile || this.mediaKind === "youtube-embed" || this.subtitleCues.length === 0) return;
    const idx = this.#cueIndexAt(this.positionMs);
    if (idx < 0) return;
    await this.seekTo(this.subtitleCues[idx].start_ms + this.subtitleOffsetMs);
  }

  async setPlaybackRate(value: number, closeMenu?: () => void) {
    closeMenu?.();
    this.playbackRate = value;
    localStorage.setItem(PLAYBACK_RATE_STORAGE_KEY, String(value));
    if (!this.currentFile || this.mediaKind === "youtube-embed") return;
    try {
      await invoke("set_playback_rate", { rate: value });
    } catch (err) {
      this.errorMessage = String(err);
    }
  }

  /** Backend playback rate resets to 1x on every fresh media load (a new
   * libVLC input always starts there); reapply the user's remembered
   * choice so it carries across videos and app restarts. */
  async #applyStoredPlaybackRate() {
    if (this.playbackRate === 1) return;
    try {
      await invoke("set_playback_rate", { rate: this.playbackRate });
    } catch {
      // best-effort — playback just stays at 1x if this fails
    }
  }

  async onWordActivate(word: string, sentence: string, x: number, y: number) {
    if (!this.currentFile) return;

    try {
      await invoke("pause");
      this.isPlaying = false;
    } catch (err) {
      this.errorMessage = String(err);
    }

    this.wordPopup = {
      word,
      sentence,
      timestampMs: this.positionMs,
      videoSource: this.currentFile,
      x,
      y,
      loading: true,
      definition: null,
      error: "",
      added: false,
      aiLoading: false,
      aiText: "",
      aiError: "",
      examplesLoading: false,
      examplesText: "",
      examplesError: "",
    };

    try {
      const definition = await invoke<WordDefinition>("define_word", { word });
      if (this.wordPopup && this.wordPopup.word === word) {
        this.wordPopup = { ...this.wordPopup, loading: false, definition };
      }
    } catch (err) {
      if (this.wordPopup && this.wordPopup.word === word) {
        this.wordPopup = { ...this.wordPopup, loading: false, error: String(err) };
      }
    }
  }

  closePopup() {
    this.wordPopup = null;
  }

  async addToVocabulary() {
    if (!this.wordPopup || this.wordPopup.added) return;
    const popup = this.wordPopup;
    const def = popup.definition;
    const definitionText = def?.meanings[0]?.definitions[0] ?? def?.summary ?? null;
    try {
      await invoke("add_vocabulary_word", {
        word: popup.word,
        definition: definitionText,
        contextSentence: popup.sentence,
        sourceVideo: popup.videoSource,
        timestampMs: popup.timestampMs,
      });
      if (this.wordPopup === popup) {
        this.wordPopup = { ...popup, added: true };
      }
      void this.refreshSavedWords();
    } catch (err) {
      this.errorMessage = String(err);
    }
  }

  /** Plays the dictionary API's recorded pronunciation clip for the popped-
   * up word, when it returned one. Some entries in dictionaryapi.dev return
   * protocol-relative audio URLs ("//...") — normalize those to https since
   * `new Audio()` won't resolve a bare "//" against the app's own origin. */
  pronounceWord() {
    const url = this.wordPopup?.definition?.audio_url;
    if (!url) return;
    const normalized = url.startsWith("//") ? `https:${url}` : url;
    new Audio(normalized).play().catch(() => {
      // autoplay/policy or network failure — no UI feedback needed for this
    });
  }

  async resumeFromPopup() {
    this.wordPopup = null;
    try {
      await invoke("play");
      this.isPlaying = true;
    } catch (err) {
      this.errorMessage = String(err);
    }
  }
}

export const player = new PlayerStore();
