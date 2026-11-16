mod dictionary;
mod llm;
mod mkv_subtitles;
mod subtitles;
mod vocabulary;
mod youtube;

use std::collections::HashMap;
use std::os::raw::{c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use image::codecs::jpeg::JpegEncoder;
use image::ExtendedColorType;
use mkv_subtitles::EmbeddedSubtitleTrack;
use serde::Serialize;
use subtitles::SubtitleCue;
use tauri::ipc::{Channel, Response};
use tauri::Manager;
use vlc::MediaPlayerAudioEx;

/// Every raw frame payload goes through Tauri's slower fetch-based IPC path
/// (payloads over 1KB skip the fast inline-eval path entirely). Raw RGBA
/// frames at full resolution/framerate saturate that path badly, so frames
/// are JPEG-compressed before sending (roughly 20-40x smaller than raw RGBA)
/// which is what actually lets playback stay smooth without flooding the
/// webview. Resolution cap and frame-rate cap are a secondary safety margin.
///
/// The cap is user-selectable (see `set_video_quality`) rather than a fixed
/// constant, since downscaling to fit a fixed cap means playback no longer
/// matches the source's native resolution — some users would rather trade
/// smoothness for sharpness. There's no "uncapped/original" option: encoding
/// a 4K frame every ~33ms is exactly the kind of load that caused the
/// stuttering this pipeline was built to avoid, so the highest preset is
/// capped at 1920 rather than left unbounded.
const DEFAULT_DECODE_DIMENSION: u32 = 960;
const TARGET_FRAME_INTERVAL: Duration = Duration::from_millis(33); // ~30fps
const JPEG_QUALITY: u8 = 75;

fn scaled_dimensions(width: u32, height: u32, max_dimension: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= max_dimension || longest == 0 {
        return (width, height);
    }
    let scale = max_dimension as f64 / longest as f64;
    (
        ((width as f64 * scale).round() as u32).max(2),
        ((height as f64 * scale).round() as u32).max(2),
    )
}

/// Computes the actual buffer dimensions to decode into: corrects for the
/// source's sample aspect ratio (anamorphic encodes store non-square pixels,
/// so the coded width/height alone isn't the display aspect ratio), applies
/// the quality cap, then applies a user aspect-ratio override if set.
fn compute_dimensions(
    coded_width: u32,
    coded_height: u32,
    sar_num: u32,
    sar_den: u32,
    max_dimension: u32,
    aspect_override: Option<(u32, u32)>,
) -> (u32, u32) {
    let sar_num = sar_num.max(1);
    let sar_den = sar_den.max(1);
    let display_width = ((coded_width as u64 * sar_num as u64) / sar_den as u64).max(1) as u32;

    let (base_width, base_height) = scaled_dimensions(display_width, coded_height, max_dimension);

    match aspect_override {
        Some((num, den)) if num > 0 && den > 0 => {
            let height = ((base_width as u64 * den as u64) / num as u64).max(1) as u32;
            (base_width, height)
        }
        _ => (base_width, base_height),
    }
}

/// Receives decoded RGBA frames from libVLC's video callbacks and forwards
/// them to the frontend over an IPC channel. libVLC calls lock/unlock/display
/// sequentially on its own decode thread, so the buffer only needs protecting
/// against a resize happening concurrently with a file switch.
struct FrameSink {
    buffer: Mutex<Vec<u8>>,
    channel: Mutex<Option<Channel<Response>>>,
    width: AtomicU32,
    height: AtomicU32,
    last_sent: Mutex<Instant>,
}

impl FrameSink {
    fn new() -> Self {
        Self {
            buffer: Mutex::new(Vec::new()),
            channel: Mutex::new(None),
            width: AtomicU32::new(0),
            height: AtomicU32::new(0),
            last_sent: Mutex::new(Instant::now()),
        }
    }

    fn resize(&self, width: u32, height: u32) {
        let size = width as usize * height as usize * 4;
        *self.buffer.lock().unwrap() = vec![0u8; size];
        self.width.store(width, Ordering::Relaxed);
        self.height.store(height, Ordering::Relaxed);
    }
}

unsafe extern "C" fn video_lock(opaque: *mut c_void, planes: *mut c_void) -> *mut c_void {
    let sink = unsafe { &*(opaque as *const FrameSink) };
    let mut buf = sink.buffer.lock().unwrap();
    let ptr = buf.as_mut_ptr();
    unsafe { *(planes as *mut *mut c_void) = ptr as *mut c_void };
    ptr as *mut c_void
}

unsafe extern "C" fn video_unlock(_opaque: *mut c_void, _picture: *mut c_void, _planes: *const *mut c_void) {
}

fn rgba_to_rgb(rgba: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for px in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&px[..3]);
    }
    rgb
}

unsafe extern "C" fn video_display(opaque: *mut c_void, _picture: *mut c_void) {
    let sink = unsafe { &*(opaque as *const FrameSink) };

    {
        let mut last_sent = sink.last_sent.lock().unwrap();
        if last_sent.elapsed() < TARGET_FRAME_INTERVAL {
            return;
        }
        *last_sent = Instant::now();
    }

    let width = sink.width.load(Ordering::Relaxed);
    let height = sink.height.load(Ordering::Relaxed);
    if width == 0 || height == 0 {
        return;
    }

    // No subscriber (before the page mounts, or after a reload): skip the
    // copy and JPEG encode entirely.
    if sink.channel.lock().unwrap().is_none() {
        return;
    }

    let rgba = sink.buffer.lock().unwrap().clone();
    let rgb = rgba_to_rgb(&rgba);

    let mut jpeg_bytes = Vec::new();
    let encoded = JpegEncoder::new_with_quality(&mut jpeg_bytes, JPEG_QUALITY)
        .encode(&rgb, width, height, ExtendedColorType::Rgb8)
        .is_ok();

    if encoded {
        if let Some(channel) = sink.channel.lock().unwrap().as_ref() {
            let _ = channel.send(Response::new(jpeg_bytes));
        }
    }
}

/// Points libVLC's decoder at our RGBA buffer for the given resolution.
/// Must be called (via `MediaPlayer::raw()`) after `video_lock`/`unlock`/
/// `display` are registered and before playback starts for that media.
fn set_video_format(media_player: &vlc::MediaPlayer, width: u32, height: u32) {
    let pitch = width * 4;
    unsafe {
        vlc::sys::libvlc_video_set_format(
            media_player.raw(),
            c"RGBA".as_ptr() as *const c_char,
            width,
            height,
            pitch,
        );
    }
}

struct Player {
    instance: vlc::Instance,
    media_player: vlc::MediaPlayer,
}

// vlc-rs wraps raw libVLC pointers and doesn't mark them Send/Sync itself.
// All access goes through the Mutex below, and libVLC's player-level API is
// safe to call from a single thread at a time, so this is sound.
unsafe impl Send for Player {}
unsafe impl Sync for Player {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MediaKind {
    LocalFile,
    YouTube,
}

struct AppState {
    player: Mutex<Player>,
    frame_sink: Arc<FrameSink>,
    // Caches the result of the one expensive full-file scan for embedded MKV
    // subtitle tracks, keyed by file path, so switching between tracks for
    // the same file after the first selection is instant.
    subtitle_cache: Mutex<Option<(String, HashMap<u64, Vec<SubtitleCue>>)>>,
    // Caches the last-listed YouTube caption tracks (with their direct URLs)
    // keyed by video URL, so selecting one just needs a lookup.
    youtube_caption_cache: Mutex<Option<(String, Vec<youtube::CaptionTrack>)>>,
    http_client: reqwest::Client,
    dictionary_cache: Mutex<HashMap<String, dictionary::WordDefinition>>,
    db: Mutex<rusqlite::Connection>,
    current_path: Mutex<Option<String>>,
    current_media_kind: Mutex<MediaKind>,
    decode_quality: AtomicU32,
    // None = auto (use the source's own aspect ratio); Some((num, den)) =
    // force a specific ratio regardless of source.
    aspect_override: Mutex<Option<(u32, u32)>>,
    // Carries the user's chosen speed across quality/aspect reloads and
    // fresh loads of a new video — libVLC resets rate to 1.0 on every new
    // input, so it must be explicitly reapplied wherever media is opened.
    playback_rate: Mutex<f32>,
}

#[derive(Serialize)]
struct PlaybackStatus {
    is_playing: bool,
    position_ms: i64,
    duration_ms: i64,
    width: u32,
    height: u32,
    volume: i32,
}

#[derive(Serialize)]
struct LoadFileResponse {
    duration_ms: i64,
    width: u32,
    height: u32,
    subtitles: Vec<SubtitleCue>,
    title: Option<String>,
}

/// Opens `path` as the player's current media at the given decode-quality
/// cap and aspect-ratio override. Shared between `load_file` (fresh open)
/// and `reload_current_media` (same file, new settings, playback position
/// preserved by the caller).
fn add_media_option(media: &vlc::Media, option: &str) {
    if let Ok(c_option) = std::ffi::CString::new(option) {
        unsafe {
            vlc::sys::libvlc_media_add_option(media.raw(), c_option.as_ptr());
        }
    }
}

fn media_for_local_file(player: &Player, path: &str) -> Result<vlc::Media, String> {
    let media = vlc::Media::new_path(&player.instance, path)
        .ok_or_else(|| format!("could not open media: {path}"))?;
    // The app draws subtitles itself (clickable words over the frame), from
    // the sidecar file it finds via `subtitles::find_sibling`. libVLC would
    // otherwise also auto-load that same `<video>.*.vtt/srt` file and burn
    // it into the video frames, giving two overlapping captions.
    add_media_option(&media, ":no-sub-autodetect-file");
    // Same for subtitle tracks embedded in the container (the app extracts
    // and overlays those itself too).
    add_media_option(&media, ":no-spu");
    Ok(media)
}

fn media_for_youtube(player: &Player, resolved: &youtube::ResolvedStream) -> Result<vlc::Media, String> {
    let media = vlc::Media::new_location(&player.instance, &resolved.url)
        .ok_or_else(|| "could not open YouTube stream".to_string())?;
    if let Some(ua) = &resolved.user_agent {
        add_media_option(&media, &format!(":http-user-agent={ua}"));
    }
    if let Some(referer) = &resolved.referer {
        add_media_option(&media, &format!(":http-referrer={referer}"));
    }
    // Forces software decoding for this media only (local files are
    // unaffected). Hardware decode paths are the other half of the crash
    // `resolve_stream`'s mp4-only restriction guards against — belt and
    // suspenders, since a stream that merely *labels* itself H.264 could
    // still land on a flaky hardware decoder on some systems.
    add_media_option(&media, ":avcodec-hw=none");
    Ok(media)
}

/// Dimensions/aspect info needed to size the decode buffer, gathered
/// however the caller's source type requires (libVLC's `parse()` for local
/// files, yt-dlp's own reported format info for YouTube — see the comment
/// on `ResolvedStream` for why the latter can't just reuse `parse()`).
struct SourceDimensions {
    coded_width: u32,
    coded_height: u32,
    sar_num: u32,
    sar_den: u32,
}

fn dimensions_from_local_file(media: &vlc::Media) -> Result<SourceDimensions, String> {
    media.parse();
    media
        .tracks()
        .into_iter()
        .flatten()
        .find_map(|t| match t.type_specific_data {
            vlc::MediaTrackUnion::Video(v) => Some(SourceDimensions {
                coded_width: v.width,
                coded_height: v.height,
                sar_num: v.sar_num,
                sar_den: v.sar_den,
            }),
            _ => None,
        })
        .ok_or_else(|| "no video track found in file".to_string())
}

fn open_media_at_quality(
    player: &Player,
    frame_sink: &FrameSink,
    media: vlc::Media,
    dims: SourceDimensions,
    max_dimension: u32,
    aspect_override: Option<(u32, u32)>,
    rate: f32,
) -> Result<(u32, u32), String> {
    let (width, height) = compute_dimensions(
        dims.coded_width,
        dims.coded_height,
        dims.sar_num,
        dims.sar_den,
        max_dimension,
        aspect_override,
    );
    frame_sink.resize(width, height);
    set_video_format(&player.media_player, width, height);

    player.media_player.set_media(&media);
    player
        .media_player
        .play()
        .map_err(|_| "failed to start playback".to_string())?;
    // Best-effort: a fresh input defaults to 1.0 regardless, so a failure
    // here just leaves it there rather than breaking playback.
    let _ = player.media_player.set_rate(rate);

    Ok((width, height))
}

#[tauri::command]
fn load_file(path: String, state: tauri::State<AppState>) -> Result<LoadFileResponse, String> {
    let player = state.player.lock().unwrap();
    let max_dimension = state.decode_quality.load(Ordering::Relaxed);
    let aspect_override = *state.aspect_override.lock().unwrap();
    let media = media_for_local_file(&player, &path)?;
    let dims = dimensions_from_local_file(&media)?;
    let duration_ms = media.duration().unwrap_or(0);
    let rate = *state.playback_rate.lock().unwrap();
    let (width, height) =
        open_media_at_quality(&player, &state.frame_sink, media, dims, max_dimension, aspect_override, rate)?;

    let subtitles = subtitles::find_sibling(&path)
        .map(|p| subtitles::load_from_path(&p))
        .unwrap_or_default();
    *state.subtitle_cache.lock().unwrap() = None;
    *state.current_path.lock().unwrap() = Some(path);
    *state.current_media_kind.lock().unwrap() = MediaKind::LocalFile;

    Ok(LoadFileResponse {
        duration_ms,
        width,
        height,
        subtitles,
        title: None,
    })
}

#[derive(Serialize, Debug)]
struct QualityChangeResponse {
    width: u32,
    height: u32,
}

/// Reopens the currently-loaded file with the current quality-cap and
/// aspect-ratio settings, preserving playback position and play/pause
/// state. Changing libVLC's video format mid-stream isn't reliable once a
/// decode pipeline is already negotiated, so this does a fresh
/// open-and-seek-back rather than trying to reconfigure the running
/// pipeline (a brief reload flicker, but correct every time).
async fn reload_current_media(state: &AppState) -> Result<(u32, u32), String> {
    let path = state
        .current_path
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "no video loaded".to_string())?;
    let max_dimension = state.decode_quality.load(Ordering::Relaxed);
    let aspect_override = *state.aspect_override.lock().unwrap();
    let media_kind = *state.current_media_kind.lock().unwrap();

    // YouTube's direct stream URLs are short-lived and tied to the specific
    // format they were resolved for, so a quality/aspect change has to
    // re-resolve via yt-dlp rather than just reopening the same URL.
    // spawn_blocking (same fix and reasoning as load_youtube) keeps that
    // subprocess wait off the main thread so the app doesn't freeze.
    let resolved_youtube = match media_kind {
        MediaKind::YouTube => {
            let resolve_path = path.clone();
            Some(
                tauri::async_runtime::spawn_blocking(move || youtube::resolve_stream(&resolve_path, max_dimension))
                    .await
                    .map_err(|e| format!("internal error resolving stream: {e}"))??,
            )
        }
        MediaKind::LocalFile => None,
    };

    let player = state.player.lock().unwrap();
    let was_playing = player.media_player.is_playing();
    let position_ms = player.media_player.get_time().unwrap_or(0);

    let (media, dims) = if let Some(resolved) = resolved_youtube {
        let dims = SourceDimensions {
            coded_width: resolved.width,
            coded_height: resolved.height,
            sar_num: 1,
            sar_den: 1,
        };
        (media_for_youtube(&player, &resolved)?, dims)
    } else {
        let media = media_for_local_file(&player, &path)?;
        let dims = dimensions_from_local_file(&media)?;
        (media, dims)
    };
    let rate = *state.playback_rate.lock().unwrap();
    let (width, height) =
        open_media_at_quality(&player, &state.frame_sink, media, dims, max_dimension, aspect_override, rate)?;

    // A seek issued immediately after play() on freshly-set media is silently
    // dropped by libVLC — it hasn't finished its Opening/Buffering transition
    // yet. Wait (briefly, bounded) for it to actually reach Playing first.
    let deadline = Instant::now() + Duration::from_secs(2);
    while player.media_player.state() != vlc::State::Playing && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(15));
    }

    player.media_player.set_time(position_ms);
    if !was_playing {
        player.media_player.set_pause(true);
    }

    Ok((width, height))
}

#[tauri::command]
async fn set_video_quality(
    max_dimension: u32,
    state: tauri::State<'_, AppState>,
) -> Result<QualityChangeResponse, String> {
    state.decode_quality.store(max_dimension, Ordering::Relaxed);
    let (width, height) = reload_current_media(&state).await?;
    Ok(QualityChangeResponse { width, height })
}

/// `num`/`den` of 0 means "Auto" (use the source's own aspect ratio).
#[tauri::command]
async fn set_aspect_ratio(
    num: u32,
    den: u32,
    state: tauri::State<'_, AppState>,
) -> Result<QualityChangeResponse, String> {
    *state.aspect_override.lock().unwrap() = if num == 0 || den == 0 {
        None
    } else {
        Some((num, den))
    };
    let (width, height) = reload_current_media(&state).await?;
    Ok(QualityChangeResponse { width, height })
}

#[tauri::command]
fn load_subtitles(path: String) -> Vec<SubtitleCue> {
    subtitles::load_from_path(&PathBuf::from(path))
}

/// Separate from `load_file` on purpose: for files without a Matroska
/// SeekHead this has to linearly scan the container, which can take a
/// noticeable moment on large files. Keeping it out of `load_file` means
/// playback always starts immediately; the subtitle picker just populates
/// a beat later.
#[tauri::command]
fn list_embedded_subtitle_tracks(path: String) -> Vec<EmbeddedSubtitleTrack> {
    let path = Path::new(&path);
    let is_matroska = matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()),
        Some(ext) if ext == "mkv" || ext == "webm"
    );
    if is_matroska {
        mkv_subtitles::list_tracks(path)
    } else {
        Vec::new()
    }
}

#[tauri::command]
fn load_embedded_subtitle_track(
    path: String,
    track_id: u64,
    state: tauri::State<AppState>,
) -> Vec<SubtitleCue> {
    let mut cache = state.subtitle_cache.lock().unwrap();
    let needs_scan = !matches!(cache.as_ref(), Some((cached_path, _)) if *cached_path == path);
    if needs_scan {
        let tracks = mkv_subtitles::extract_all_cues(Path::new(&path));
        *cache = Some((path, tracks));
    }
    cache
        .as_ref()
        .and_then(|(_, tracks)| tracks.get(&track_id))
        .cloned()
        .unwrap_or_default()
}

#[tauri::command]
fn subscribe_video_frames(channel: Channel<Response>, state: tauri::State<AppState>) {
    *state.frame_sink.channel.lock().unwrap() = Some(channel);
}

#[tauri::command]
async fn load_youtube(url: String, state: tauri::State<'_, AppState>) -> Result<LoadFileResponse, String> {
    let max_dimension = state.decode_quality.load(Ordering::Relaxed);
    let aspect_override = *state.aspect_override.lock().unwrap();

    // Shells out to yt-dlp, which can take anywhere from a couple of
    // seconds to much longer (network conditions, YouTube's bot-detection
    // challenges). Non-async Tauri commands run on the main thread, so
    // calling this inline used to freeze the entire window — no repaints,
    // no input, get_status stuck too — for the whole wait. spawn_blocking
    // moves the actual blocking subprocess call off both the main thread
    // and the async runtime's regular worker threads.
    let resolve_url = url.clone();
    let resolved = tauri::async_runtime::spawn_blocking(move || youtube::resolve_stream(&resolve_url, max_dimension))
        .await
        .map_err(|e| format!("internal error resolving stream: {e}"))??;

    // Checked before ever touching the player: a stream that's going to
    // get cut off by YouTube's CDN partway through should never start
    // playing in the first place, so the frontend can cleanly fall back to
    // Embed mode instead of leaving a stalled video on screen.
    if !youtube::probe_stream_reliability(&state.http_client, &resolved).await {
        return Err(youtube::RESTRICTED_STREAM_ERROR.to_string());
    }

    let title = resolved.title.clone();
    let duration_ms = resolved.duration_ms;
    let dims = SourceDimensions {
        coded_width: resolved.width,
        coded_height: resolved.height,
        sar_num: 1,
        sar_den: 1,
    };

    let player = state.player.lock().unwrap();
    let media = media_for_youtube(&player, &resolved)?;
    let rate = *state.playback_rate.lock().unwrap();
    let (width, height) =
        open_media_at_quality(&player, &state.frame_sink, media, dims, max_dimension, aspect_override, rate)?;

    *state.subtitle_cache.lock().unwrap() = None;
    *state.youtube_caption_cache.lock().unwrap() = None;
    *state.current_path.lock().unwrap() = Some(url);
    *state.current_media_kind.lock().unwrap() = MediaKind::YouTube;

    Ok(LoadFileResponse {
        duration_ms,
        width,
        height,
        subtitles: Vec::new(),
        title: if title.is_empty() { None } else { Some(title) },
    })
}

#[tauri::command]
async fn list_youtube_captions(state: tauri::State<'_, AppState>) -> Result<Vec<youtube::CaptionTrack>, String> {
    let url = state
        .current_path
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "no video loaded".to_string())?;
    // Same yt-dlp subprocess concern as load_youtube — see its comment.
    let list_url = url.clone();
    let tracks = tauri::async_runtime::spawn_blocking(move || youtube::list_captions(&list_url))
        .await
        .map_err(|e| format!("internal error listing captions: {e}"))??;
    *state.youtube_caption_cache.lock().unwrap() = Some((url, tracks.clone()));
    Ok(tracks)
}

#[derive(Serialize)]
struct DownloadedVideoResponse {
    path: String,
    title: String,
}

/// Fallback for when direct-stream playback (`load_youtube`) turns out to
/// be unplayable — downloads the video properly (separate audio/video
/// streams merged with ffmpeg, same as yt-dlp's normal download mode)
/// instead of streaming a single throttled URL. `video_id` is trusted from
/// the frontend's own `extractYoutubeVideoId` rather than re-parsed here,
/// since it's just used to name the cache file, not to fetch anything by
/// itself.
#[tauri::command]
async fn download_youtube_video(
    url: String,
    video_id: String,
    // A quality (max height, e.g. 720) the user picked; None = "Auto".
    quality_height: Option<u32>,
    on_progress: Channel<youtube::DownloadProgress>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<DownloadedVideoResponse, String> {
    if !youtube::is_safe_video_id(&video_id) {
        return Err("Invalid video id.".to_string());
    }
    let max_dimension = state.decode_quality.load(Ordering::Relaxed);
    let cache_dir = youtube_cache_dir(&app)?;

    // Choosing a different quality for an already-downloaded video replaces
    // its file, and Windows can't overwrite one libVLC has open.
    if youtube::will_replace_cached(&cache_dir, &video_id, quality_height) {
        stop_player_if_playing(&state, |p| {
            p.parent() == Some(cache_dir.as_path()) && p.file_stem().and_then(|s| s.to_str()) == Some(video_id.as_str())
        });
    }

    let sink: youtube::ProgressSink = std::sync::Arc::new(move |p| {
        let _ = on_progress.send(p);
    });
    let (path, title) = tauri::async_runtime::spawn_blocking(move || {
        youtube::download_video(&url, &video_id, &cache_dir, max_dimension, quality_height, sink)
    })
    .await
    .map_err(|e| format!("internal error downloading video: {e}"))??;

    Ok(DownloadedVideoResponse {
        path: path.to_string_lossy().to_string(),
        title,
    })
}

fn youtube_cache_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?
        .join("youtube_cache"))
}

/// Windows can't delete a file libVLC still has open, so if the video being
/// removed is the one currently loaded, stop the player first (the frontend
/// closes the video on its side).
fn stop_player_if_playing(state: &AppState, matches: impl Fn(&Path) -> bool) {
    let mut current = state.current_path.lock().unwrap();
    if current.as_deref().map(|p| matches(Path::new(p))).unwrap_or(false) {
        state.player.lock().unwrap().media_player.stop();
        *current = None;
    }
}

#[tauri::command]
fn list_youtube_downloads(app: tauri::AppHandle) -> Result<youtube::DownloadsSummary, String> {
    Ok(youtube::list_downloads(&youtube_cache_dir(&app)?))
}

#[tauri::command]
fn delete_youtube_download(
    video_id: String,
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<u64, String> {
    let cache_dir = youtube_cache_dir(&app)?;
    stop_player_if_playing(&state, |p| {
        p.parent() == Some(cache_dir.as_path())
            && p.file_stem().and_then(|s| s.to_str()) == Some(video_id.as_str())
    });
    youtube::delete_download(&cache_dir, &video_id)
}

#[tauri::command]
fn clear_youtube_downloads(app: tauri::AppHandle, state: tauri::State<AppState>) -> Result<u64, String> {
    let cache_dir = youtube_cache_dir(&app)?;
    stop_player_if_playing(&state, |p| p.parent() == Some(cache_dir.as_path()));
    youtube::clear_downloads(&cache_dir)
}

#[tauri::command]
async fn load_youtube_caption(
    track_id: u64,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<SubtitleCue>, String> {
    let caption_url = {
        let cache = state.youtube_caption_cache.lock().unwrap();
        cache
            .as_ref()
            .and_then(|(_, tracks)| tracks.iter().find(|t| t.id == track_id))
            .map(|t| t.url.clone())
            .ok_or_else(|| "subtitle track not found; try reopening the subtitle menu".to_string())?
    };
    youtube::download_captions(&state.http_client, &caption_url).await
}

#[tauri::command]
async fn define_word(
    word: String,
    state: tauri::State<'_, AppState>,
) -> Result<dictionary::WordDefinition, String> {
    let key = dictionary::clean_word(&word);
    if key.is_empty() {
        return Err("Not a word.".to_string());
    }

    if let Some(cached) = state.dictionary_cache.lock().unwrap().get(&key) {
        return Ok(cached.clone());
    }

    let definition = dictionary::lookup(&state.http_client, &key).await?;
    state
        .dictionary_cache
        .lock()
        .unwrap()
        .insert(key, definition.clone());
    Ok(definition)
}

#[tauri::command]
fn add_vocabulary_word(
    word: String,
    definition: Option<String>,
    context_sentence: Option<String>,
    source_video: Option<String>,
    timestamp_ms: Option<i64>,
    state: tauri::State<AppState>,
) -> Result<vocabulary::VocabularyEntry, String> {
    let conn = state.db.lock().unwrap();
    vocabulary::add_entry(
        &conn,
        &word,
        definition.as_deref(),
        context_sentence.as_deref(),
        source_video.as_deref(),
        timestamp_ms,
    )
}

#[tauri::command]
fn list_vocabulary(state: tauri::State<AppState>) -> Result<Vec<vocabulary::VocabularyEntry>, String> {
    let conn = state.db.lock().unwrap();
    vocabulary::list_entries(&conn)
}

#[tauri::command]
fn delete_vocabulary_word(id: i64, state: tauri::State<AppState>) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    vocabulary::delete_entry(&conn, id)
}

#[tauri::command]
fn list_due_vocabulary(state: tauri::State<AppState>) -> Result<Vec<vocabulary::VocabularyEntry>, String> {
    let conn = state.db.lock().unwrap();
    vocabulary::due_entries(&conn)
}

#[tauri::command]
fn review_vocabulary_word(
    id: i64,
    rating: i64,
    state: tauri::State<AppState>,
) -> Result<vocabulary::VocabularyEntry, String> {
    let conn = state.db.lock().unwrap();
    vocabulary::review_entry(&conn, id, rating)
}

#[tauri::command]
fn play(state: tauri::State<AppState>) -> Result<(), String> {
    let player = state.player.lock().unwrap();
    player
        .media_player
        .play()
        .map_err(|_| "failed to play".to_string())
}

#[tauri::command]
fn pause(state: tauri::State<AppState>) {
    let player = state.player.lock().unwrap();
    player.media_player.set_pause(true);
}

#[tauri::command]
fn toggle_play(state: tauri::State<AppState>) -> Result<bool, String> {
    let player = state.player.lock().unwrap();
    if player.media_player.is_playing() {
        player.media_player.set_pause(true);
        Ok(false)
    } else {
        player
            .media_player
            .play()
            .map_err(|_| "failed to play".to_string())?;
        Ok(true)
    }
}

#[tauri::command]
fn seek_ms(position_ms: i64, state: tauri::State<AppState>) {
    let player = state.player.lock().unwrap();
    player.media_player.set_time(position_ms);
}

#[tauri::command]
fn set_volume(volume: i32, state: tauri::State<AppState>) -> Result<(), String> {
    let player = state.player.lock().unwrap();
    player
        .media_player
        .set_volume(volume.clamp(0, 200))
        .map_err(|_| "failed to set volume".to_string())
}

#[tauri::command]
fn set_playback_rate(rate: f32, state: tauri::State<AppState>) -> Result<(), String> {
    let clamped = rate.clamp(0.25, 3.0);
    {
        let player = state.player.lock().unwrap();
        player
            .media_player
            .set_rate(clamped)
            .map_err(|_| "failed to set playback rate".to_string())?;
    }
    *state.playback_rate.lock().unwrap() = clamped;
    Ok(())
}

#[tauri::command]
fn get_status(state: tauri::State<AppState>) -> PlaybackStatus {
    let player = state.player.lock().unwrap();
    let duration_ms = player
        .media_player
        .get_media()
        .and_then(|m| m.duration())
        .unwrap_or(0);
    PlaybackStatus {
        is_playing: player.media_player.is_playing(),
        position_ms: player.media_player.get_time().unwrap_or(0),
        duration_ms,
        width: state.frame_sink.width.load(Ordering::Relaxed),
        height: state.frame_sink.height.load(Ordering::Relaxed),
        volume: player.media_player.get_volume(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // A full page load (including a dev hot-reload) destroys the JS side
        // of the frame channel and all UI state, but the Rust player would
        // otherwise keep decoding and sending frames to a callback id that no
        // longer exists (hundreds of "Couldn't find callback id" warnings,
        // then a crash). Drop the stale channel and pause so nothing runs
        // against a dead page; the new page re-subscribes on mount.
        .on_page_load(|webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Started) {
                if let Some(state) = webview.app_handle().try_state::<AppState>() {
                    *state.frame_sink.channel.lock().unwrap() = None;
                    state.player.lock().unwrap().media_player.set_pause(true);
                }
            }
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            load_file,
            load_youtube,
            set_video_quality,
            set_aspect_ratio,
            load_subtitles,
            load_embedded_subtitle_track,
            list_embedded_subtitle_tracks,
            list_youtube_captions,
            load_youtube_caption,
            download_youtube_video,
            list_youtube_downloads,
            delete_youtube_download,
            clear_youtube_downloads,
            subscribe_video_frames,
            define_word,
            add_vocabulary_word,
            list_vocabulary,
            delete_vocabulary_word,
            list_due_vocabulary,
            review_vocabulary_word,
            play,
            pause,
            toggle_play,
            seek_ms,
            set_volume,
            set_playback_rate,
            get_status,
            llm::llm_status,
            llm::llm_download_file,
            llm::llm_file_size,
            llm::llm_extract_downloaded_archive,
            llm::llm_explain,
            llm::llm_word_examples
        ])
        .setup(|app| {
            // A packaged release ships VLC's `plugins` folder next to the exe,
            // and libVLC finds a `plugins` folder beside its own DLL by
            // default, so nothing needs setting there. Only a development
            // build, which has no such folder, falls back to the VLC install
            // directory that build.rs found on the build machine.
            let bundled_plugins = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.join("plugins")))
                .is_some_and(|dir| dir.is_dir());
            if !bundled_plugins {
                if let Some(plugin_dir) = option_env!("VLC_PLUGIN_DIR") {
                    // SAFETY: called once during setup, before any other threads
                    // are spawned or read this process's environment.
                    unsafe { std::env::set_var("VLC_PLUGIN_PATH", plugin_dir) };
                }
            }

            let instance = vlc::Instance::new()
                .unwrap_or_else(|| panic!("failed to initialize libVLC: {:?}", vlc::errmsg()));
            let media_player =
                vlc::MediaPlayer::new(&instance).expect("failed to create libVLC player");

            let frame_sink = Arc::new(FrameSink::new());
            // Leaked on purpose: this pointer must stay valid for the life of
            // the process, matching the media_player's own lifetime.
            let opaque = Arc::into_raw(frame_sink.clone()) as *mut c_void;
            unsafe {
                vlc::sys::libvlc_video_set_callbacks(
                    media_player.raw(),
                    Some(video_lock),
                    Some(video_unlock),
                    Some(video_display),
                    opaque,
                );
            }

            let data_dir = app
                .path()
                .app_data_dir()
                .expect("could not resolve app data directory");
            let db = vocabulary::open(&data_dir).expect("failed to open vocabulary database");

            app.manage(AppState {
                player: Mutex::new(Player {
                    instance,
                    media_player,
                }),
                frame_sink,
                subtitle_cache: Mutex::new(None),
                youtube_caption_cache: Mutex::new(None),
                http_client: reqwest::Client::new(),
                dictionary_cache: Mutex::new(HashMap::new()),
                db: Mutex::new(db),
                current_path: Mutex::new(None),
                current_media_kind: Mutex::new(MediaKind::LocalFile),
                decode_quality: AtomicU32::new(DEFAULT_DECODE_DIMENSION),
                aspect_override: Mutex::new(None),
                playback_rate: Mutex::new(1.0),
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod dimension_tests {
    use super::compute_dimensions;

    #[test]
    fn square_pixels_no_override_just_caps_by_longest_side() {
        // 1920x800 (2.4:1), square pixels, capped to 960 longest side.
        let (w, h) = compute_dimensions(1920, 800, 1, 1, 960, None);
        assert_eq!((w, h), (960, 400));
    }

    #[test]
    fn anamorphic_sar_widens_display_width_before_capping() {
        // Coded 1440x1080 with SAR 4:3 -> display 1920x1080 (16:9), then capped.
        let (w, h) = compute_dimensions(1440, 1080, 4, 3, 960, None);
        assert_eq!((w, h), (960, 540));
    }

    #[test]
    fn zero_sar_is_treated_as_1_to_1() {
        let (w, h) = compute_dimensions(1920, 1080, 0, 0, 960, None);
        assert_eq!((w, h), (960, 540));
    }

    #[test]
    fn aspect_override_recomputes_height_from_capped_width() {
        // Source is 16:9-ish, but force a 4:3 override.
        let (w, h) = compute_dimensions(1920, 1080, 1, 1, 960, Some((4, 3)));
        assert_eq!(w, 960);
        assert_eq!(h, 720);
    }

    #[test]
    fn below_cap_dimensions_are_left_untouched() {
        let (w, h) = compute_dimensions(640, 360, 1, 1, 960, None);
        assert_eq!((w, h), (640, 360));
    }
}
