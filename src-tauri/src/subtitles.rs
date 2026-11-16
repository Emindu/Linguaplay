use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct SubtitleCue {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn parse_timestamp(s: &str) -> Option<i64> {
    let s = s.trim();
    let (main, ms) = s.split_once(|c| c == ',' || c == '.')?;
    let ms: i64 = ms.trim().parse().ok()?;
    let parts: Vec<&str> = main.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [h, m, s] => (h.parse::<i64>().ok()?, m.parse::<i64>().ok()?, s.parse::<i64>().ok()?),
        [m, s] => (0i64, m.parse::<i64>().ok()?, s.parse::<i64>().ok()?),
        _ => return None,
    };
    Some((h * 3600 + m * 60 + sec) * 1000 + ms)
}

fn parse_time_range(line: &str) -> Option<(i64, i64)> {
    let (start, end) = line.split_once("-->")?;
    let end_timestamp = end.split_whitespace().next()?;
    Some((parse_timestamp(start)?, parse_timestamp(end_timestamp)?))
}

/// Parses SRT or WebVTT subtitle text into cues. Both formats share the same
/// "index/id (optional) + time range + text lines + blank line" block
/// structure, so one pass handles both; VTT-only extras (WEBVTT header,
/// NOTE/STYLE blocks) are simply skipped as lines with no time range.
pub fn parse(content: &str) -> Vec<SubtitleCue> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut cues = Vec::new();
    let mut lines = content.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !trimmed.contains("-->") {
            continue;
        }
        if let Some((start_ms, end_ms)) = parse_time_range(trimmed) {
            let mut text_lines = Vec::new();
            while let Some(next) = lines.peek() {
                if next.trim().is_empty() {
                    break;
                }
                text_lines.push(strip_tags(next.trim()));
                lines.next();
            }
            if !text_lines.is_empty() {
                cues.push(SubtitleCue {
                    start_ms,
                    end_ms,
                    text: text_lines.join(" "),
                });
            }
        }
    }

    cues
}

/// Looks for a same-name .srt/.vtt file next to the given video file.
pub fn find_sibling(video_path: &str) -> Option<PathBuf> {
    let path = Path::new(video_path);
    let stem = path.file_stem()?;
    let dir = path.parent()?;
    for ext in ["srt", "vtt"] {
        let candidate = dir.join(format!("{}.{}", stem.to_string_lossy(), ext));
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // Language-tagged sidecars like `video.en.vtt` / `video.en-US.srt` (the
    // convention yt-dlp and most subtitle tools use). English is preferred,
    // then whichever sorts first so the pick is stable.
    let prefix = format!("{}.", stem.to_string_lossy());
    let mut tagged: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            name.starts_with(&prefix.to_lowercase()) && (name.ends_with(".srt") || name.ends_with(".vtt"))
        })
        .collect();
    tagged.sort_by_key(|p| {
        let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        (!name[prefix.len()..].starts_with("en"), name)
    });
    tagged.into_iter().next()
}

pub fn load_from_path(path: &Path) -> Vec<SubtitleCue> {
    std::fs::read_to_string(path)
        .map(|content| parse(&content))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_srt() {
        let srt = "1\n00:00:01,000 --> 00:00:04,500\nHello world\n\n2\n00:00:05,000 --> 00:00:07,000\nSecond line\n";
        let cues = parse(srt);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start_ms, 1000);
        assert_eq!(cues[0].end_ms, 4500);
        assert_eq!(cues[0].text, "Hello world");
        assert_eq!(cues[1].start_ms, 5000);
    }

    #[test]
    fn parses_vtt_with_tags_and_settings() {
        let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:04.500 align:start line:90%\n<i>Hello</i> world\n";
        let cues = parse(vtt);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].start_ms, 1000);
        assert_eq!(cues[0].end_ms, 4500);
        assert_eq!(cues[0].text, "Hello world");
    }

    #[test]
    fn parses_hour_component() {
        let srt = "1\n01:02:03,250 --> 01:02:04,000\nLine\n";
        let cues = parse(srt);
        assert_eq!(cues[0].start_ms, (3600 + 120 + 3) * 1000 + 250);
    }

    #[test]
    fn multiline_cue_text_is_joined() {
        let srt = "1\n00:00:00,000 --> 00:00:01,000\nLine one\nLine two\n";
        let cues = parse(srt);
        assert_eq!(cues[0].text, "Line one Line two");
    }
}

#[cfg(test)]
mod sibling_tests {
    use super::*;

    #[test]
    fn finds_language_tagged_sidecar_and_prefers_english() {
        let dir = std::env::temp_dir().join(format!("vl-sibling-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("clip.mp4"), b"").unwrap();
        std::fs::write(dir.join("clip.de.vtt"), b"").unwrap();
        std::fs::write(dir.join("clip.en.vtt"), b"").unwrap();
        let found = find_sibling(dir.join("clip.mp4").to_str().unwrap()).unwrap();
        assert!(found.ends_with("clip.en.vtt"));
        // An exact `clip.srt` still wins over tagged ones.
        std::fs::write(dir.join("clip.srt"), b"").unwrap();
        let found = find_sibling(dir.join("clip.mp4").to_str().unwrap()).unwrap();
        assert!(found.ends_with("clip.srt"));
        // Unrelated videos' captions are never picked up.
        assert!(find_sibling(dir.join("other.mp4").to_str().unwrap()).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_real_youtube_auto_captions() {
        let Ok(path) = std::env::var("YT_VTT_SAMPLE") else { return };
        let cues = load_from_path(Path::new(&path));
        assert!(cues.len() > 100, "only {} cues", cues.len());
        let mut dups = 0;
        for w in cues.windows(2) {
            if w[0].text == w[1].text {
                dups += 1;
            }
        }
        println!("cues={} consecutive_duplicates={} first={:?}", cues.len(), dups, cues.first().map(|c| c.text.clone()));
        assert!(dups < cues.len() / 20);
    }
}
