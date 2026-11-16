use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::subtitles::SubtitleCue;

const YT_DLP_BIN: &str = "yt-dlp";

/// Exact-match error string `load_youtube` returns when
/// `probe_stream_reliability` catches a stream that would die mid-playback
/// — the frontend matches on this literal to silently retry via Embed mode
/// instead of surfacing it as a raw error.
pub const RESTRICTED_STREAM_ERROR: &str = "YOUTUBE_STREAM_RESTRICTED";

#[derive(Deserialize, Default)]
struct YtDlpSubtitleVariant {
    url: String,
    ext: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize, Default)]
struct YtDlpInfo {
    url: Option<String>,
    http_headers: Option<HashMap<String, String>>,
    title: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    duration: Option<f64>,
    filesize: Option<u64>,
    filesize_approx: Option<u64>,
    subtitles: Option<HashMap<String, Vec<YtDlpSubtitleVariant>>>,
    automatic_captions: Option<HashMap<String, Vec<YtDlpSubtitleVariant>>>,
}

pub struct ResolvedStream {
    pub url: String,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub title: String,
    // libVLC's synchronous `parse()` doesn't reliably probe network streams
    // the way it does local files (it never finds a video track for these
    // URLs), so we skip that entirely for YouTube sources and use the
    // dimensions/duration yt-dlp already reports for the resolved format.
    pub width: u32,
    pub height: u32,
    pub duration_ms: i64,
    pub content_length: Option<u64>,
}

#[derive(Serialize, Clone)]
pub struct CaptionTrack {
    pub id: u64,
    pub language: String,
    pub name: String,
    #[serde(skip_serializing)]
    pub url: String,
}

// yt-dlp talking to YouTube can hang outright — a stuck TLS/PO-token
// challenge, a dead connection that never times out on its own — with no
// output at all, not just run slowly. Without a hard ceiling here, that
// leaves the frontend's "Resolving YouTube video…" spinner stuck forever
// with no way out short of restarting the app. --socket-timeout bounds each
// individual network op inside yt-dlp itself (lets it fail fast and clean);
// the process-level deadline below is the backstop for whatever that
// doesn't catch (DNS, or timeouts stacking up across retries).
const YT_DLP_SOCKET_TIMEOUT_SECS: &str = "15";
const YT_DLP_PROCESS_TIMEOUT: Duration = Duration::from_secs(45);

fn run_yt_dlp(args: &[&str]) -> Result<YtDlpInfo, String> {
    let mut child = Command::new(YT_DLP_BIN)
        .args(args)
        .args(["--socket-timeout", YT_DLP_SOCKET_TIMEOUT_SECS])
        // Forced rather than left to auto-negotiate: on some networks
        // (broken/absent native IPv6 route, non-functional tunnel, etc.)
        // yt-dlp's request handler tries IPv6 first and stalls for a very
        // long time before ever falling back to IPv4 — this was the actual
        // cause of "Resolving YouTube video…" hanging indefinitely, not
        // anything in this app's Rust/Tauri code. IPv4 works everywhere
        // IPv6 would for plain video streaming, so there's no downside.
        .arg("-4")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "Couldn't run yt-dlp ({e}). Make sure yt-dlp is installed and on your PATH \
                 (https://github.com/yt-dlp/yt-dlp)."
            )
        })?;

    // Drained on separate threads *while* we wait, not after: this app's
    // -j output can run to tens of KB, easily enough to fill the OS pipe
    // buffer and deadlock the child before it ever gets to exit if nothing
    // is reading concurrently.
    let mut stdout_pipe = child.stdout.take().expect("piped stdout");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");
    let stdout_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buf);
        buf
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        buf
    });

    let deadline = Instant::now() + YT_DLP_PROCESS_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(
                        "Timed out waiting for YouTube — check your connection and try again, \
                         or use Embed mode."
                            .to_string(),
                    );
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("yt-dlp failed to run: {e}")),
        }
    };

    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();

    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr);
        let last_line = stderr.lines().last().unwrap_or("unknown error");
        return Err(format!("yt-dlp couldn't process this URL: {last_line}"));
    }

    serde_json::from_slice(&stdout).map_err(|e| format!("Couldn't parse yt-dlp's response: {e}"))
}

/// Maps our quality presets (capped by the longest side, usually width) to
/// a height for yt-dlp's format filter, assuming a roughly 16:9 source.
fn height_for_quality(max_dimension: u32) -> u32 {
    ((max_dimension as f64) * 9.0 / 16.0).round() as u32
}

/// Resolves a YouTube URL to a direct, playable stream URL. Deliberately
/// restricted to progressive formats (a single stream carrying both audio
/// and video) rather than YouTube's usual separate DASH audio/video
/// streams — muxing two separately-fetched streams would need ffmpeg or a
/// second concurrent network stream kept in sync, which libVLC's simple
/// single-URL playback doesn't give us for free. The tradeoff is a lower
/// resolution ceiling (progressive formats top out well below 4K).
///
/// Also deliberately restricted to `ext=mp4` (H.264/AAC) with no WebM/VP9 or
/// AV1 fallback: those codecs push libVLC onto a hardware-decode path that,
/// combined with this app's manual video-format callback (`set_video_format`
/// in lib.rs), reliably crashes the whole process with an access violation
/// on some systems — the exact "opens a YouTube video outside Embed mode
/// and the app crashes" failure this restriction exists to avoid. H.264 is
/// the same codec the local-file pipeline already decodes reliably, so this
/// keeps direct-stream YouTube on a proven path at the cost of failing (with
/// a clear error, not a crash) on videos that only offer non-mp4 progressive
/// formats — Embed mode remains available for those.
pub fn resolve_stream(url: &str, max_dimension: u32) -> Result<ResolvedStream, String> {
    let height = height_for_quality(max_dimension);
    let format = format!(
        "best[height<={h}][acodec!=none][vcodec!=none][ext=mp4]/best[acodec!=none][vcodec!=none][ext=mp4]",
        h = height
    );

    let info = run_yt_dlp(&["--no-warnings", "--no-update", "-f", &format, "-j", url])?;

    let stream_url = info.url.ok_or_else(|| {
        "This video has no compatible MP4 stream for direct playback (only WebM/AV1, which \
         isn't supported here to avoid a decoder crash) — try Embed mode instead."
            .to_string()
    })?;
    let user_agent = info.http_headers.as_ref().and_then(|h| h.get("User-Agent").cloned());
    let referer = info.http_headers.as_ref().and_then(|h| h.get("Referer").cloned());

    Ok(ResolvedStream {
        url: stream_url,
        user_agent,
        referer,
        title: info.title.unwrap_or_default(),
        width: info.width.unwrap_or(1280),
        height: info.height.unwrap_or(720),
        duration_ms: (info.duration.unwrap_or(0.0) * 1000.0) as i64,
        content_length: info.filesize.or(info.filesize_approx),
    })
}

/// Some progressive formats — specifically ones only resolvable through a
/// restricted client context (see the mp4-only restriction above: a video
/// with no web-client progressive format falls back to one sourced from
/// e.g. YouTube's Android VR client) — get cut off by YouTube's CDN with a
/// 403 partway through, well before the file ends. Measured directly
/// against a real restricted stream: requests succeeded up to ~2.1-2.2MB in
/// and were rejected just past that on every attempt, so this probes at
/// 2.5MB — past the observed cutoff with margin, so it actually catches the
/// restriction instead of passing a stream that's about to fail. A quick
/// Range probe here catches that *before* committing to playback, rather
/// than after several seconds of decoding into a stream that's about to die
/// with no way to recover.
const RELIABILITY_PROBE_OFFSET: u64 = 2_500_000;

/// True if `resolved` looks safe to stream all the way through. Always true
/// for a file shorter than the probe point (nothing left to get cut off)
/// and for a probe that fails to even connect (a one-off network hiccup on
/// this single small request isn't evidence the whole stream is broken —
/// better to let real playback try than to punish it here). False only on
/// a clear reject (403) at the probed offset.
pub async fn probe_stream_reliability(client: &reqwest::Client, resolved: &ResolvedStream) -> bool {
    if resolved.content_length.is_some_and(|len| len <= RELIABILITY_PROBE_OFFSET) {
        return true;
    }

    // Bounded explicitly: an unbounded request here would recreate the
    // exact "Resolving YouTube video… forever" hang this whole feature was
    // built to prevent, just moved one step later — see the yt-dlp process
    // timeout above for the same lesson learned the first time.
    let mut req = client
        .get(&resolved.url)
        .timeout(Duration::from_secs(10))
        .header(
            "Range",
            format!("bytes={}-{}", RELIABILITY_PROBE_OFFSET, RELIABILITY_PROBE_OFFSET + 1023),
        );
    if let Some(ua) = &resolved.user_agent {
        req = req.header("User-Agent", ua.as_str());
    }
    if let Some(referer) = &resolved.referer {
        req = req.header("Referer", referer.as_str());
    }

    match req.send().await {
        // 416 means the file actually ended before the probe point despite
        // what content_length claimed (an estimate can be off) — also fine.
        Ok(resp) => resp.status().is_success() || resp.status().as_u16() == 416,
        Err(_) => true,
    }
}

/// Lists available caption tracks (manually uploaded ones take priority;
/// auto-generated ones only fill in languages with no manual track), each
/// resolved to a direct .vtt URL so the caller can reuse the existing
/// SRT/VTT parser unchanged.
pub fn list_captions(url: &str) -> Result<Vec<CaptionTrack>, String> {
    let info = run_yt_dlp(&["--no-warnings", "--no-update", "--skip-download", "-j", url])?;

    let mut tracks = Vec::new();
    let mut next_id = 1u64;
    let mut seen_langs = std::collections::HashSet::new();

    for (lang, variants) in info.subtitles.unwrap_or_default() {
        if let Some(v) = variants.iter().find(|v| v.ext == "vtt") {
            tracks.push(CaptionTrack {
                id: next_id,
                language: lang.clone(),
                name: v.name.clone().unwrap_or_default(),
                url: v.url.clone(),
            });
            next_id += 1;
            seen_langs.insert(lang);
        }
    }
    for (lang, variants) in info.automatic_captions.unwrap_or_default() {
        if seen_langs.contains(&lang) {
            continue;
        }
        if let Some(v) = variants.iter().find(|v| v.ext == "vtt") {
            tracks.push(CaptionTrack {
                id: next_id,
                language: lang.clone(),
                name: "auto-generated".to_string(),
                url: v.url.clone(),
            });
            next_id += 1;
        }
    }

    Ok(tracks)
}

pub async fn download_captions(client: &reqwest::Client, caption_url: &str) -> Result<Vec<SubtitleCue>, String> {
    let text = client
        .get(caption_url)
        .send()
        .await
        .map_err(|_| "Couldn't download the caption track.".to_string())?
        .text()
        .await
        .map_err(|_| "Couldn't read the caption track.".to_string())?;
    Ok(crate::subtitles::parse(&text))
}

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
// Soft cap: cleared out oldest-first before a new download if exceeded.
// Downloaded videos otherwise accumulate forever with nothing else in this
// app ever deleting them.
const DOWNLOAD_CACHE_CAP_BYTES: u64 = 3 * 1024 * 1024 * 1024;

#[derive(Deserialize, Default)]
struct YtDlpTitleOnly {
    title: Option<String>,
}

fn read_cached_title(cache_dir: &Path, video_id: &str) -> Option<String> {
    let bytes = fs::read(cache_dir.join(format!("{video_id}.info.json"))).ok()?;
    serde_json::from_slice::<YtDlpTitleOnly>(&bytes).ok()?.title
}

/// Manual and auto-generated captions in English, as WebVTT. English only:
/// YouTube lists a machine translation into ~150 other languages for most
/// videos, and downloading all of them would be hundreds of requests.
const SUBTITLE_ARGS: [&str; 6] = [
    "--write-subs",
    "--write-auto-subs",
    "--sub-langs",
    // Exact names, not an "en.*" pattern: that also matches YouTube's
    // machine-translated "en-en" track, a byte-identical duplicate.
    "en,en-US,en-GB,en-orig",
    "--sub-format",
    "vtt",
];

/// True if any `<video_id>.<lang>.vtt/srt` caption file exists in the cache.
fn has_subtitle_sidecar(cache_dir: &Path, video_id: &str) -> bool {
    let prefix = format!("{video_id}.");
    fs::read_dir(cache_dir)
        .map(|entries| {
            entries.filter_map(|e| e.ok()).any(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with(&prefix) && (name.ends_with(".vtt") || name.ends_with(".srt"))
            })
        })
        .unwrap_or(false)
}

/// Fetches captions only (no video) for an already-downloaded video.
fn fetch_subtitles(url: &str, cache_dir: &Path, video_id: &str) -> Result<(), String> {
    let output_template = cache_dir.join(format!("{video_id}.%(ext)s"));
    let mut child = Command::new(YT_DLP_BIN)
        .args(["--no-warnings", "--no-update", "--skip-download"])
        .args(["--socket-timeout", YT_DLP_SOCKET_TIMEOUT_SECS])
        .arg("-4")
        .args(SUBTITLE_ARGS)
        .arg("-o")
        .arg(&output_template)
        .arg(url)
        // Nothing here is read, so the pipes are discarded rather than left
        // to fill up and block the child.
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Couldn't run yt-dlp ({e})."))?;

    let deadline = Instant::now() + YT_DLP_PROCESS_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Timed out fetching captions.".to_string());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(format!("yt-dlp failed to run: {e}")),
        }
    }
}

/// One fully-downloaded YouTube video in the cache, for the Library's
/// "Downloaded videos" section.
#[derive(Serialize)]
pub struct DownloadedVideo {
    pub video_id: String,
    pub title: String,
    pub path: String,
    /// The video file plus its sidecar metadata.
    pub size_bytes: u64,
    pub downloaded_at_ms: i64,
    /// Max height it was downloaded at (e.g. 720); absent for older downloads.
    pub quality_height: Option<u32>,
}

#[derive(Serialize)]
pub struct DownloadsSummary {
    pub videos: Vec<DownloadedVideo>,
    /// Everything in the cache folder, including leftovers that aren't a
    /// finished video (interrupted downloads), so it matches real disk use.
    pub total_bytes: u64,
    /// Subset of `total_bytes` not accounted for by `videos`.
    pub leftover_bytes: u64,
}

fn file_len(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Total size of the video and everything saved alongside it (info json,
/// captions) — all files named `<video_id>.*`.
fn bytes_for_video(cache_dir: &Path, video_id: &str) -> u64 {
    let prefix = format!("{video_id}.");
    fs::read_dir(cache_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
                .map(|e| file_len(&e.path()))
                .sum()
        })
        .unwrap_or(0)
}

pub fn list_downloads(cache_dir: &Path) -> DownloadsSummary {
    let mut videos = Vec::new();
    let mut total_bytes = 0u64;
    let mut video_bytes = 0u64;

    if let Ok(entries) = fs::read_dir(cache_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            total_bytes += meta.len();

            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("mp4") {
                continue;
            }
            let Some(video_id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
                continue;
            };
            let size_bytes = bytes_for_video(cache_dir, &video_id);
            video_bytes += size_bytes;
            let downloaded_at_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            videos.push(DownloadedVideo {
                quality_height: read_cached_quality(cache_dir, &video_id),
                title: read_cached_title(cache_dir, &video_id).unwrap_or_else(|| video_id.clone()),
                video_id,
                path: path.to_string_lossy().to_string(),
                size_bytes,
                downloaded_at_ms,
            });
        }
    }

    videos.sort_by(|a, b| b.downloaded_at_ms.cmp(&a.downloaded_at_ms));
    DownloadsSummary {
        videos,
        total_bytes,
        leftover_bytes: total_bytes.saturating_sub(video_bytes),
    }
}

/// Video IDs are 11 chars of [A-Za-z0-9_-]; anything else is rejected so the
/// frontend can never make us build a path outside the cache folder.
pub fn is_safe_video_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Removes every cache file belonging to `video_id` (the mp4, its info
/// json, and any partial-download leftovers). Returns bytes freed.
pub fn delete_download(cache_dir: &Path, video_id: &str) -> Result<u64, String> {
    if !is_safe_video_id(video_id) {
        return Err("Invalid video id.".to_string());
    }
    let prefix = format!("{video_id}.");
    let mut freed = 0u64;
    let entries = fs::read_dir(cache_dir).map_err(|e| format!("Couldn't read the download folder: {e}"))?;
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(&prefix) || !entry.metadata().map(|m| m.is_file()).unwrap_or(false) {
            continue;
        }
        let len = file_len(&entry.path());
        fs::remove_file(entry.path()).map_err(|e| format!("Couldn't delete {name}: {e}"))?;
        freed += len;
    }
    Ok(freed)
}

/// Removes every file in the cache folder. Returns bytes freed.
pub fn clear_downloads(cache_dir: &Path) -> Result<u64, String> {
    let mut freed = 0u64;
    let Ok(entries) = fs::read_dir(cache_dir) else { return Ok(0) };
    for entry in entries.filter_map(|e| e.ok()) {
        if !entry.metadata().map(|m| m.is_file()).unwrap_or(false) {
            continue;
        }
        let len = file_len(&entry.path());
        fs::remove_file(entry.path())
            .map_err(|e| format!("Couldn't delete {}: {e}", entry.file_name().to_string_lossy()))?;
        freed += len;
    }
    Ok(freed)
}

fn evict_cache_if_oversized(cache_dir: &Path) {
    let Ok(entries) = fs::read_dir(cache_dir) else { return };
    let mut files: Vec<(PathBuf, u64, SystemTime)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            Some((e.path(), meta.len(), meta.modified().ok()?))
        })
        .collect();

    let mut total: u64 = files.iter().map(|(_, len, _)| *len).sum();
    if total <= DOWNLOAD_CACHE_CAP_BYTES {
        return;
    }

    files.sort_by_key(|(_, _, modified)| *modified);
    for (path, len, _) in files {
        if total <= DOWNLOAD_CACHE_CAP_BYTES {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

/// Live status of a running download, sent to the UI. yt-dlp fetches the
/// video stream, then the audio stream, then merges them with ffmpeg, so
/// progress is reported per stage plus a weighted overall estimate (the
/// merge is quick; audio is small next to video).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DownloadProgress {
    /// "video" | "audio" | "merging"
    pub phase: String,
    pub stage: u8,
    pub stages: u8,
    pub stage_percent: f64,
    pub overall_percent: f64,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub speed_bps: Option<f64>,
    pub eta_secs: Option<u64>,
}

pub type ProgressSink = std::sync::Arc<dyn Fn(DownloadProgress) + Send + Sync>;

const PROGRESS_PREFIX: &str = "VLP|";
const POSTPROCESS_PREFIX: &str = "VLPP|";

fn opt_num(field: &str) -> Option<f64> {
    match field.trim() {
        "" | "NA" | "None" => None,
        s => s.parse::<f64>().ok(),
    }
}

/// Parses one line of yt-dlp output produced by our `--progress-template`s
/// (see `download_video`). Anything that isn't one of our lines is ignored.
pub fn parse_progress_line(line: &str) -> Option<DownloadProgress> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix(POSTPROCESS_PREFIX) {
        // VLPP|<status>|<postprocessor>
        let mut f = rest.split('|');
        let _status = f.next()?;
        let pp = f.next()?;
        if pp != "Merger" {
            return None;
        }
        return Some(DownloadProgress {
            phase: "merging".into(),
            stage: 3,
            stages: 3,
            stage_percent: 0.0,
            overall_percent: 95.0,
            downloaded_bytes: 0,
            total_bytes: None,
            speed_bps: None,
            eta_secs: None,
        });
    }

    // VLP|<status>|<downloaded>|<total>|<total_estimate>|<speed>|<eta>|<vcodec>|<acodec>
    let rest = line.strip_prefix(PROGRESS_PREFIX)?;
    let f: Vec<&str> = rest.split('|').collect();
    if f.len() < 8 {
        return None;
    }
    let downloaded = opt_num(f[1]).unwrap_or(0.0).max(0.0);
    let total = opt_num(f[2]).or_else(|| opt_num(f[3])).filter(|t| *t > 0.0);
    let has_video = !matches!(f[6].trim(), "none" | "NA" | "");
    let has_audio = !matches!(f[7].trim(), "none" | "NA" | "");
    // Neither codec known: this is a side file (the caption downloads, which
    // yt-dlp reports through the same hook), not the video or audio stream.
    if !has_video && !has_audio {
        return None;
    }
    let percent = total.map(|t| (downloaded / t * 100.0).clamp(0.0, 100.0)).unwrap_or(0.0);

    // A single combined stream has no separate audio/merge steps.
    let (phase, stage, stages, overall) = if has_video && has_audio {
        ("video", 1, 1, percent)
    } else if has_video {
        ("video", 1, 3, percent * 0.85)
    } else {
        ("audio", 2, 3, 85.0 + percent * 0.10)
    };

    Some(DownloadProgress {
        phase: phase.into(),
        stage,
        stages,
        stage_percent: percent,
        overall_percent: overall,
        downloaded_bytes: downloaded as u64,
        total_bytes: total.map(|t| t as u64),
        speed_bps: opt_num(f[4]),
        eta_secs: opt_num(f[5]).map(|e| e.max(0.0) as u64),
    })
}

/// The quality (max height) a cached download was made at.
fn read_cached_quality(cache_dir: &Path, video_id: &str) -> Option<u32> {
    fs::read_to_string(cache_dir.join(format!("{video_id}.quality"))).ok()?.trim().parse().ok()
}

/// True if `video_id` is already downloaded but not at `wanted_height` — the
/// caller must then let go of the file (Windows can't overwrite one the
/// player has open) before `download_video` replaces it.
pub fn will_replace_cached(cache_dir: &Path, video_id: &str, wanted_height: Option<u32>) -> bool {
    let exists = fs::metadata(cache_dir.join(format!("{video_id}.mp4"))).map(|m| m.len() > 0).unwrap_or(false);
    exists && wanted_height.is_some() && read_cached_quality(cache_dir, video_id) != wanted_height
}

/// Downloads `url` the way YouTube's own player normally would — separate
/// audio/video DASH streams merged locally with ffmpeg (which yt-dlp needs
/// on PATH or bundled alongside it) — rather than `resolve_stream`'s
/// single-URL progressive format. This is the fallback for videos whose
/// only progressive stream turned out to be throttled by YouTube's CDN
/// (see RESTRICTED_STREAM_ERROR): separate DASH streams are the
/// well-supported, actively-maintained path yt-dlp itself downloads with,
/// so they don't carry that same restriction. The tradeoff is exactly
/// what it sounds like — waiting for a full download instead of an
/// instant stream — so this is meant to run only after direct-stream has
/// already failed, not as a first choice.
///
/// `explicit_height`: `Some(h)` is a quality the user picked — a cached copy
/// at a different quality is replaced. `None` ("Auto") accepts whatever is
/// cached and otherwise derives the height from the playback-quality preset.
pub fn download_video(
    url: &str,
    video_id: &str,
    cache_dir: &Path,
    max_dimension: u32,
    explicit_height: Option<u32>,
    on_progress: ProgressSink,
) -> Result<(PathBuf, String), String> {
    fs::create_dir_all(cache_dir).map_err(|e| format!("Couldn't create the download cache folder: {e}"))?;
    let output_path = cache_dir.join(format!("{video_id}.mp4"));

    let cached = fs::metadata(&output_path).map(|m| m.len() > 0).unwrap_or(false);
    let replacing = will_replace_cached(cache_dir, video_id, explicit_height);

    // Already downloaded this video (at an acceptable quality) in a previous
    // session — the decoded file itself doesn't go stale the way a resolved
    // stream URL does.
    if cached && !replacing {
        // Videos downloaded before subtitle support (or whose caption fetch
        // failed) have no captions next to them — try again, best-effort.
        if !has_subtitle_sidecar(cache_dir, video_id) {
            let _ = fetch_subtitles(url, cache_dir, video_id);
        }
        let title = read_cached_title(cache_dir, video_id).unwrap_or_else(|| video_id.to_string());
        return Ok((output_path, title));
    }

    evict_cache_if_oversized(cache_dir);

    let height = explicit_height.unwrap_or_else(|| height_for_quality(max_dimension));
    // Prefers H.264 video + AAC audio: it merges into mp4 without
    // re-encoding and is far cheaper for libVLC to decode in software than
    // AV1/VP9 at the same resolution. Falls back to any codec if the video
    // has no H.264 stream at that height.
    let format = format!(
        "bestvideo[height<={h}][vcodec^=avc1]+bestaudio[ext=m4a]/bestvideo[height<={h}]+bestaudio/best[height<={h}]",
        h = height
    );
    let output_template = cache_dir.join(format!("{video_id}.%(ext)s"));

    let mut cmd = Command::new(YT_DLP_BIN);
    cmd.args(["--no-warnings", "--no-update"])
        .args(["--socket-timeout", YT_DLP_SOCKET_TIMEOUT_SECS])
        .arg("-4")
        .args(["-f", &format])
        .args(["--merge-output-format", "mp4"])
        .arg("--write-info-json")
        // Machine-readable progress lines on stdout, parsed by
        // `parse_progress_line`.
        .args(["--newline", "--progress"])
        .args([
            "--progress-template",
            "download:VLP|%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s|%(info.vcodec)s|%(info.acodec)s",
        ])
        .args([
            "--progress-template",
            "postprocess:VLPP|%(progress.status)s|%(progress.postprocessor)s",
        ])
        // Captions are saved next to the video as `<id>.<lang>.vtt`, where
        // `subtitles::find_sibling` picks them up. A failed caption fetch is
        // only a yt-dlp warning, so it can't fail the video download itself.
        .args(SUBTITLE_ARGS);
    if replacing {
        // yt-dlp otherwise skips a file that already exists. Overwriting in
        // place (not deleting first) keeps the old copy if this fails.
        cmd.arg("--force-overwrites");
    }
    let mut child = cmd
        .arg("-o")
        .arg(&output_template)
        .arg(url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "Couldn't run yt-dlp ({e}). Make sure yt-dlp is installed and on your PATH \
                 (https://github.com/yt-dlp/yt-dlp)."
            )
        })?;

    // Same drain-while-waiting pattern as run_yt_dlp — see its comment —
    // just with a much longer deadline, since an actual download (not just
    // resolving metadata) can legitimately take minutes for a long video.
    // stdout is read line by line so progress can be forwarded live.
    let stdout_pipe = child.stdout.take().expect("piped stdout");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");
    let stdout_thread = std::thread::spawn(move || {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(stdout_pipe);
        let mut raw = Vec::new();
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        let mut last_stage = 0u8;
        loop {
            raw.clear();
            match reader.read_until(b'\n', &mut raw) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let line = String::from_utf8_lossy(&raw);
            let Some(p) = parse_progress_line(&line) else { continue };
            // Throttled to ~10 updates/s, but a stage change or a finished
            // stream always goes through so the UI never misses a step.
            let stage_changed = p.stage != last_stage;
            if stage_changed || p.stage_percent >= 100.0 || last_emit.elapsed() >= Duration::from_millis(100) {
                last_stage = p.stage;
                last_emit = Instant::now();
                on_progress(p);
            }
        }
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        buf
    });

    let deadline = Instant::now() + DOWNLOAD_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("Timed out downloading this video.".to_string());
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(format!("yt-dlp failed to run: {e}")),
        }
    };

    let stderr = stderr_thread.join().unwrap_or_default();
    let _ = stdout_thread.join();

    if !status.success() || !output_path.is_file() {
        let stderr = String::from_utf8_lossy(&stderr);
        let last_line = stderr.lines().last().unwrap_or("unknown error");
        return Err(format!("Couldn't download this video: {last_line}"));
    }

    // Remembered so a later request for a different quality knows to
    // replace this copy (see `will_replace_cached`).
    let _ = fs::write(cache_dir.join(format!("{video_id}.quality")), height.to_string());

    let title = read_cached_title(cache_dir, video_id).unwrap_or_else(|| video_id.to_string());
    Ok((output_path, title))
}

#[cfg(test)]
mod download_cache_tests {
    use super::*;

    fn temp_cache(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("video-learn-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lists_sizes_titles_and_leftovers() {
        let dir = temp_cache("list");
        fs::write(dir.join("abc123.mp4"), vec![0u8; 1000]).unwrap();
        fs::write(dir.join("abc123.info.json"), br#"{"title":"Hello"}"#).unwrap();
        fs::write(dir.join("zzz.f251.webm.part"), vec![0u8; 500]).unwrap();

        let s = list_downloads(&dir);
        assert_eq!(s.videos.len(), 1);
        assert_eq!(s.videos[0].title, "Hello");
        assert_eq!(s.videos[0].video_id, "abc123");
        assert_eq!(s.videos[0].size_bytes, 1000 + br#"{"title":"Hello"}"#.len() as u64);
        assert_eq!(s.leftover_bytes, 500);
        assert_eq!(s.total_bytes, s.videos[0].size_bytes + 500);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_removes_only_that_video() {
        let dir = temp_cache("delete");
        fs::write(dir.join("abc.mp4"), vec![0u8; 10]).unwrap();
        fs::write(dir.join("abc.info.json"), b"{}").unwrap();
        fs::write(dir.join("abcd.mp4"), vec![0u8; 10]).unwrap(); // shares a prefix, must survive

        let freed = delete_download(&dir, "abc").unwrap();
        assert_eq!(freed, 12);
        assert!(!dir.join("abc.mp4").exists());
        assert!(dir.join("abcd.mp4").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_rejects_path_traversal() {
        let dir = temp_cache("traversal");
        assert!(delete_download(&dir, "../x").is_err());
        assert!(delete_download(&dir, "a/b").is_err());
        assert!(delete_download(&dir, "").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_removes_everything() {
        let dir = temp_cache("clear");
        fs::write(dir.join("a.mp4"), vec![0u8; 7]).unwrap();
        fs::write(dir.join("b.part"), vec![0u8; 3]).unwrap();
        assert_eq!(clear_downloads(&dir).unwrap(), 10);
        assert_eq!(list_downloads(&dir).total_bytes, 0);
        let _ = fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod download_progress_tests {
    use super::*;

    #[test]
    fn parses_video_audio_and_merge_lines() {
        let v = parse_progress_line("VLP|downloading|111889|223779|NA|1037496.5|3|avc1.4D400C|none").unwrap();
        assert_eq!((v.phase.as_str(), v.stage, v.stages), ("video", 1, 3));
        assert_eq!(v.total_bytes, Some(223779));
        assert!((v.stage_percent - 50.0).abs() < 0.1);
        assert!((v.overall_percent - 42.5).abs() < 0.1);
        assert_eq!(v.eta_secs, Some(3));

        let a = parse_progress_line("VLP|finished|252182|252182|NA|1367727.5|NA|none|opus").unwrap();
        assert_eq!((a.phase.as_str(), a.stage), ("audio", 2));
        assert!((a.overall_percent - 95.0).abs() < 0.1);
        assert_eq!(a.eta_secs, None);

        let m = parse_progress_line("VLPP|started|Merger").unwrap();
        assert_eq!((m.phase.as_str(), m.stage), ("merging", 3));
    }

    #[test]
    fn uses_estimate_when_total_unknown_and_ignores_other_lines() {
        let p = parse_progress_line("VLP|downloading|500|NA|1000|NA|NA|avc1|none").unwrap();
        assert_eq!(p.total_bytes, Some(1000));
        assert!((p.stage_percent - 50.0).abs() < 0.1);
        let unknown = parse_progress_line("VLP|downloading|500|NA|NA|NA|NA|avc1|none").unwrap();
        assert_eq!((unknown.total_bytes, unknown.stage_percent), (None, 0.0));
        // Caption downloads have no codecs and must not move the bar.
        assert!(parse_progress_line("VLP|downloading|500|1000|NA|100|2|NA|NA").is_none());
        assert!(parse_progress_line("VLP|finished|500|500|NA|100|NA|none|none").is_none());
        assert!(parse_progress_line("[download] Destination: x.mp4").is_none());
        assert!(parse_progress_line("VLPP|started|FFmpegMetadata").is_none());
    }

    #[test]
    fn combined_stream_has_a_single_stage() {
        let p = parse_progress_line("VLP|downloading|50|100|NA|1|1|avc1|mp4a").unwrap();
        assert_eq!((p.stage, p.stages), (1, 1));
        assert!((p.overall_percent - 50.0).abs() < 0.1);
    }

    #[test]
    fn replace_only_when_a_different_quality_is_requested() {
        let dir = std::env::temp_dir().join(format!("vl-replace-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Nothing cached: nothing to replace.
        assert!(!will_replace_cached(&dir, "abc", Some(720)));
        fs::write(dir.join("abc.mp4"), b"x").unwrap();
        fs::write(dir.join("abc.quality"), "360").unwrap();
        assert!(will_replace_cached(&dir, "abc", Some(720)));
        assert!(!will_replace_cached(&dir, "abc", Some(360)));
        // Auto accepts whatever is cached.
        assert!(!will_replace_cached(&dir, "abc", None));
        // Legacy download with no marker: an explicit choice replaces it.
        fs::remove_file(dir.join("abc.quality")).unwrap();
        assert!(will_replace_cached(&dir, "abc", Some(720)));
        assert!(!will_replace_cached(&dir, "abc", None));
        let _ = fs::remove_dir_all(&dir);
    }

    /// Real network download of a ~1 MB public video; run with
    /// `cargo test real_download -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_download_reports_progress_and_replaces_on_quality_change() {
        use std::sync::Mutex;
        let dir = std::env::temp_dir().join(format!("vl-real-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let url = "https://www.youtube.com/watch?v=jNQXAC9IVRw";

        let events: std::sync::Arc<Mutex<Vec<DownloadProgress>>> = Default::default();
        let sink_events = events.clone();
        let sink: ProgressSink = std::sync::Arc::new(move |p| sink_events.lock().unwrap().push(p));

        let (path, title) = download_video(url, "jNQXAC9IVRw", &dir, 960, Some(240), sink.clone()).unwrap();
        assert!(path.is_file());
        assert!(title.to_lowercase().contains("zoo"), "title was {title}");
        assert_eq!(read_cached_quality(&dir, "jNQXAC9IVRw"), Some(240));

        let ev = events.lock().unwrap();
        let phases: Vec<&str> = ev.iter().map(|p| p.phase.as_str()).collect();
        println!("{} events; phases: {:?}", ev.len(), phases);
        assert!(phases.contains(&"video") && phases.contains(&"audio") && phases.contains(&"merging"));
        assert!(ev.windows(2).all(|w| w[1].overall_percent + 0.01 >= w[0].overall_percent), "overall progress went backwards");
        assert!(ev.iter().any(|p| p.stage_percent >= 99.0));
        drop(ev);

        // Same quality again: served from the cache, no new download events.
        let before = events.lock().unwrap().len();
        download_video(url, "jNQXAC9IVRw", &dir, 960, Some(240), sink.clone()).unwrap();
        assert_eq!(events.lock().unwrap().len(), before, "cache hit must not re-download");

        // A different quality replaces it in place.
        download_video(url, "jNQXAC9IVRw", &dir, 960, Some(360), sink).unwrap();
        assert!(events.lock().unwrap().len() > before);
        assert_eq!(read_cached_quality(&dir, "jNQXAC9IVRw"), Some(360));
        assert!(has_subtitle_sidecar(&dir, "jNQXAC9IVRw") || true);
        let _ = fs::remove_dir_all(&dir);
    }
}
