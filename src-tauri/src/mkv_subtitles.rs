use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use matroska_demuxer::{Frame, MatroskaFile, TrackType};
use serde::Serialize;

use crate::subtitles::SubtitleCue;

const SUPPORTED_CODECS: [&str; 3] = ["S_TEXT/UTF8", "S_TEXT/ASS", "S_TEXT/SSA"];
const DEFAULT_CUE_DURATION_MS: i64 = 2000;

#[derive(Serialize, Clone)]
pub struct EmbeddedSubtitleTrack {
    pub id: u64,
    pub language: Option<String>,
    pub name: Option<String>,
}

fn open(path: &Path) -> Option<MatroskaFile<BufReader<File>>> {
    let file = File::open(path).ok()?;
    MatroskaFile::open(BufReader::new(file)).ok()
}

/// Lists text-based subtitle tracks embedded in a Matroska (.mkv/.webm) file.
/// Bitmap subtitle formats (PGS, VobSub) aren't listed since they can't be
/// turned into clickable text without OCR.
pub fn list_tracks(path: &Path) -> Vec<EmbeddedSubtitleTrack> {
    let Some(mkv) = open(path) else {
        return Vec::new();
    };

    mkv.tracks()
        .iter()
        .filter(|t| {
            t.track_type() == TrackType::Subtitle && SUPPORTED_CODECS.contains(&t.codec_id())
        })
        .map(|t| EmbeddedSubtitleTrack {
            id: t.track_number().get(),
            language: t.language().map(str::to_string),
            name: t.name().map(str::to_string),
        })
        .collect()
}

fn extract_ass_text(line: &str) -> &str {
    // Dialogue payload: ReadOrder,Layer,Style,Name,MarginL,MarginR,MarginV,Effect,Text
    line.splitn(9, ',').nth(8).unwrap_or(line)
}

fn strip_ass_overrides(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0u32;
    for c in s.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Extracts cues for every text-based subtitle track in a single pass.
///
/// The underlying demuxer's frame iterator reads every block's full payload
/// into memory regardless of which track it belongs to (video/audio frames
/// included) — there's no cheaper "peek track, skip payload" API available.
/// For a large file that one linear scan is unavoidably expensive (can be
/// several seconds to tens of seconds), so this collects cues for *all*
/// subtitle tracks at once rather than doing a separate full scan per track:
/// the first subtitle selection for a file pays the cost once, and switching
/// between tracks afterward is then instant from the caller's cache.
pub fn extract_all_cues(path: &Path) -> HashMap<u64, Vec<SubtitleCue>> {
    let Some(mut mkv) = open(path) else {
        return HashMap::new();
    };

    let scale_ns = mkv.info().timestamp_scale().get();
    let track_is_ass: HashMap<u64, bool> = mkv
        .tracks()
        .iter()
        .filter(|t| {
            t.track_type() == TrackType::Subtitle && SUPPORTED_CODECS.contains(&t.codec_id())
        })
        .map(|t| {
            (
                t.track_number().get(),
                matches!(t.codec_id(), "S_TEXT/ASS" | "S_TEXT/SSA"),
            )
        })
        .collect();

    let mut result: HashMap<u64, Vec<SubtitleCue>> = HashMap::new();
    let mut frame = Frame::default();
    let mut scanned: u64 = 0;
    while mkv.next_frame(&mut frame).unwrap_or(false) {
        scanned += 1;
        if scanned % 4000 == 0 {
            // Give the OS scheduler a breath during this long, CPU/IO-heavy
            // scan so it doesn't starve other threads in the process.
            std::thread::yield_now();
        }

        let Some(&is_ass) = track_is_ass.get(&frame.track) else {
            continue;
        };

        let start_ms = (frame.timestamp * scale_ns / 1_000_000) as i64;
        let duration_ms = frame
            .duration
            .map(|d| (d * scale_ns / 1_000_000) as i64)
            .unwrap_or(DEFAULT_CUE_DURATION_MS);

        let raw = String::from_utf8_lossy(&frame.data);
        let raw = if is_ass { extract_ass_text(&raw) } else { &raw };
        let text = strip_ass_overrides(raw)
            .replace("\\N", " ")
            .replace("\\n", " ");
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");

        if !text.is_empty() {
            result.entry(frame.track).or_default().push(SubtitleCue {
                start_ms,
                end_ms: start_ms + duration_ms,
                text,
            });
        }
    }

    result
}
