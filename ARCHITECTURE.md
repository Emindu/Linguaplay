# Linguaplay — Architecture

Linguaplay is a Tauri 2 desktop app: a SvelteKit/Svelte 5 frontend (TypeScript,
compiled to static assets, `ssr=false`) driving a Rust backend over Tauri's IPC
bridge. The backend wraps libVLC for playback and shells out to a few
external tools (`yt-dlp` with `ffmpeg`, `llama-cli`, `curl`, `tar`) for features libVLC and
the JS runtime can't do alone.

## 1. System context

```mermaid
flowchart TB
    user(("User"))

    subgraph app["Linguaplay (Tauri app)"]
        direction TB
        frontend["Frontend<br/>SvelteKit + Svelte 5<br/>(WebView)"]
        backend["Backend<br/>Rust (Tauri core)"]
        frontend <-->|"Tauri IPC<br/>(invoke / Channel)"| backend
    end

    subgraph external["External processes & services"]
        direction TB
        libvlc["libVLC<br/>(in-process library)"]
        ytdlp["yt-dlp<br/>(subprocess)"]
        llama["llama-cli<br/>(subprocess)"]
        curl["curl / tar<br/>(subprocess)"]
        dictapi["Free Dictionary API<br/>(HTTPS)"]
        wiki["Wikipedia REST API<br/>(HTTPS)"]
        youtube["YouTube<br/>(video/captions/iframe)"]
        ghhf["GitHub / Hugging Face<br/>(binary & model downloads)"]
    end

    subgraph storage["Local storage"]
        direction TB
        sqlite[("vocabulary.sqlite3")]
        ytcache[("App data dir:<br/>youtube_cache/ (id.mp4, .en.vtt,<br/>.info.json, .quality)")]
        appdata[("App data dir:<br/>llm/llama-cli + models/*.gguf")]
        localstorage[("Browser localStorage:<br/>recents, theme, font,<br/>LLM model choice")]
        files[("Local video/subtitle files<br/>(user's filesystem)")]
    end

    user -->|"clicks, keyboard,<br/>file picker"| frontend
    backend --> libvlc
    backend -->|"spawn"| ytdlp
    backend -->|"spawn"| llama
    backend -->|"spawn"| curl
    backend -->|"reqwest"| dictapi
    backend -->|"reqwest"| wiki
    ytdlp -->|"resolve stream/captions"| youtube
    curl -->|"download binary/model"| ghhf
    frontend -.->|"iframe embed mode only"| youtube
    backend --> sqlite
    ytdlp --> ytcache
    curl --> appdata
    llama --> appdata
    frontend --> localstorage
    backend -->|"libvlc_media_new_path"| files
```

**Three ways a YouTube link can play** (details in section 5): a direct
stream resolved by `yt-dlp` and decoded through libVLC; a full download into
the app-data cache when streaming is blocked; or "embed mode", which drops
YouTube's own `<iframe>` player into the page. Only the first two give
click-to-define and custom subtitles, because only they put the frames and
captions under the app's control.

## 2. Frontend structure

```mermaid
flowchart TB
    layout["+layout.svelte<br/>(loads theme.css, applies saved theme<br/>before first paint)"]

    subgraph routes["src/routes/"]
        player["/  (+page.svelte)<br/>video player, sidebar/popup<br/>dictionary+AI panel, AI setup modal"]
        vocab["/vocabulary (+page.svelte)<br/>word list, spaced-repetition<br/>review session"]
        library["/library (+page.svelte)<br/>recently-opened list,<br/>resume playback"]
    end

    subgraph lib["src/lib/ (shared modules)"]
        playerStore["player.svelte.ts<br/>persistent singleton store:<br/>playback state, YouTube/local<br/>loading, LLM setup, word popup"]
        recents["recents.ts<br/>localStorage recent-items list"]
        themeSettings["themeSettings.ts<br/>theme/appearance catalog<br/>+ persistence"]
        themeCss["theme.css<br/>design tokens, 16 theme presets,<br/>shared component classes"]
        themeSwitcher["ThemeSwitcher.svelte<br/>shared theme-picker component"]
        appHeader["AppHeader.svelte<br/>shared top bar + navigation"]
        stylePanel["SubtitleStylePanel.svelte + subtitleStyle.ts<br/>subtitle look: colors, weight,<br/>position, presets, live preview"]
        fmt["format.ts<br/>byte / ETA formatting"]
    end

    layout --> player
    layout --> vocab
    layout --> library
    layout --> themeCss

    player --> playerStore
    vocab --> playerStore
    library --> playerStore

    player --> recents
    library --> recents

    player --> appHeader
    player --> stylePanel
    player --> fmt
    library --> fmt
    player --> themeSwitcher
    vocab --> themeSwitcher
    library --> themeSwitcher
    themeSwitcher --> themeSettings

    playerStore -->|"invoke() / Channel"| ipc(["Tauri IPC"])
```

`player.svelte.ts` is a **module-level singleton** (not component state), so
it survives SvelteKit's destroy/recreate cycle when navigating between
routes — video playback, controls, and LLM state stay alive whether you're
looking at the player, the vocabulary list, or the library.

## 3. Backend module structure

```mermaid
flowchart LR
    subgraph src_tauri["src-tauri/src/"]
        libRs["lib.rs<br/>AppState, Tauri commands,<br/>libVLC setup, frame pipeline"]
        dictionaryRs["dictionary.rs<br/>Free Dictionary API + Wikipedia<br/>fallback lookup"]
        subtitlesRs["subtitles.rs<br/>SRT/VTT parsing"]
        mkvRs["mkv_subtitles.rs<br/>embedded MKV subtitle<br/>track demuxing"]
        vocabularyRs["vocabulary.rs<br/>SQLite CRUD + SM-2<br/>spaced-repetition scheduler"]
        youtubeRs["youtube.rs<br/>yt-dlp wrapper: stream resolution,<br/>captions, cached downloads,<br/>progress parsing, eviction"]
        llmRs["llm.rs<br/>llama-cli wrapper: install,<br/>download, explain, examples"]
    end

    libRs --> dictionaryRs
    libRs --> subtitlesRs
    libRs --> mkvRs
    libRs --> vocabularyRs
    libRs --> youtubeRs
    libRs --> llmRs
    youtubeRs --> subtitlesRs
```

### Registered Tauri commands (by feature area)

| Area | Commands |
|---|---|
| Local file playback | `load_file`, `load_subtitles`, `list_embedded_subtitle_tracks`, `load_embedded_subtitle_track` |
| YouTube (stream) | `load_youtube`, `list_youtube_captions`, `load_youtube_caption` |
| YouTube downloads | `download_youtube_video` (with a progress `Channel`), `list_youtube_downloads`, `delete_youtube_download`, `clear_youtube_downloads` |
| Playback control | `play`, `pause`, `toggle_play`, `seek_ms`, `set_volume`, `set_playback_rate`, `get_status`, `subscribe_video_frames` |
| Quality / aspect | `set_video_quality`, `set_aspect_ratio` |
| Dictionary | `define_word` |
| Vocabulary & review | `add_vocabulary_word`, `list_vocabulary`, `delete_vocabulary_word`, `list_due_vocabulary`, `review_vocabulary_word` |
| Local LLM | `llm::llm_status`, `llm::llm_download_file`, `llm::llm_file_size`, `llm::llm_extract_downloaded_archive`, `llm::llm_explain`, `llm::llm_word_examples` |

## 4. Video frame pipeline

Playback is not a native `<video>` element — libVLC decodes off-screen and
frames are pushed to a `<canvas>` over Tauri IPC.

```mermaid
sequenceDiagram
    participant VLC as libVLC (video_lock/unlock/display callbacks)
    participant Sink as FrameSink (Rust, shared Arc)
    participant Enc as JPEG encoder (image crate)
    participant Chan as Tauri Channel<Response>
    participant JS as player.svelte.ts
    participant Canvas as &lt;canvas&gt; (createImageBitmap)

    VLC->>Sink: raw RGBA frame (video_lock/unlock)
    Sink->>Enc: encode frame as JPEG
    Enc->>Chan: raw bytes (not JSON)
    Chan->>JS: onmessage(ArrayBuffer)
    alt previous frame still decoding
        JS-->>JS: drop frame (no backlog)
    else ready
        JS->>Canvas: createImageBitmap + drawImage
    end
```

**Subtitles are not rendered by libVLC.** Each media is opened with
`:no-sub-autodetect-file` and `:no-spu`, so libVLC neither auto-loads a
sibling `.vtt`/`.srt` nor draws embedded subtitle tracks into the frames. The
app parses the cues itself (`subtitles.rs`, `mkv_subtitles.rs`) and overlays
them in HTML so the words are clickable; without those options a downloaded
video showed two stacked captions.

On every page load the backend clears the frame channel and pauses
(`Builder::on_page_load`), so a webview reload can never leave libVLC pushing
frames to a dead channel.

Key perf notes baked into the build: `[profile.dev.package."*"] opt-level = 3`
keeps JPEG encoding fast even in `cargo build`/`tauri dev` (unoptimized
encoding was ~150ms/frame vs ~7-9ms optimized), and the frontend drops any
frame that arrives while the previous one is still decoding rather than
queuing, so a slow moment never builds a backlog.

## 5. YouTube loading — stream, download, or embed

There are three ways a YouTube link can play. The app tries the richest one
first and degrades only when it has to.

```mermaid
flowchart TD
    start(["User pastes a YouTube URL"]) --> check{"'Use YouTube's own player'<br/>(persisted choice)"}

    check -->|"on"| iframe["Extract video ID client-side<br/>(regex, no backend call)"]
    iframe --> embed["&lt;iframe youtube-nocookie.com/embed/ID&gt;<br/>YouTube's own player"]
    embed --> limited["No tap-to-define (captions are drawn<br/>inside the iframe).<br/>'Use tap-to-define player' button<br/>switches to the path below"]

    check -->|"off (study mode)"| cached{"Already in<br/>youtube_cache?"}
    cached -->|"yes"| local
    cached -->|"no"| resolve["youtube::resolve_stream()<br/>yt-dlp -j, run BEFORE locking<br/>the player mutex"]
    resolve -->|"ok"| media["vlc::Media::new_location()<br/>+ user-agent/referrer options"]
    resolve -->|"fails / can't stream"| dl["download_youtube_video()<br/>(section 5.1)"]
    dl --> local["Play the cached .mp4 like<br/>any local file + its .en.vtt"]
    dl -->|"also fails"| fallback["Fall back to embed mode<br/>with the real error message"]
    media --> decode["libVLC frame pipeline (section 4)"]
    local --> decode
    decode --> full["Full features: tap-to-define,<br/>custom subtitles, transcript,<br/>quality / aspect picker"]
```

Captions for non-embed videos are fetched separately
(`list_youtube_captions` / `load_youtube_caption`), preferring manual
subtitle tracks and falling back to auto-generated ones. For a downloaded
video the English captions are saved next to the file as `<id>.en.vtt`, and
`subtitles::find_sibling` picks them up exactly like a local `.srt`.

### 5.1 Download cache, quality and progress

```mermaid
sequenceDiagram
    participant UI as player.svelte.ts
    participant Cmd as download_youtube_video (lib.rs)
    participant YT as youtube::download_video
    participant Y as yt-dlp (+ ffmpeg)
    participant Disk as youtube_cache/

    UI->>Cmd: url, video_id, quality_height, Channel
    Cmd->>Cmd: validate id; stop libVLC if the cached file<br/>is about to be replaced (Windows file lock)
    Cmd->>YT: spawn_blocking
    YT->>Disk: cache hit with same quality marker?
    alt hit
        Disk-->>YT: path (no events sent)
    else miss or different quality
        YT->>Y: H.264 video + m4a audio capped at height,<br/>--progress-template, --force-overwrites
        loop stdout lines
            Y-->>YT: VLP|downloading|bytes|total|speed|eta|vcodec|acodec
            YT-->>UI: DownloadProgress via Channel (throttled ~100 ms)
        end
        Y->>Disk: id.mp4, id.info.json, id.en.vtt
        YT->>Disk: id.quality (requested height)
    end
    YT->>Disk: evict oldest past 3 GB
    Cmd-->>UI: { path, title }
```

- **Progress:** yt-dlp's progress hook is printed in a fixed template and
  parsed in Rust. Video and audio are separate transfers (plus a merge), so
  the percentage shown is a weighted estimate across the stages, and the merge
  step is shown as indeterminate. Caption downloads report through the same
  hook and are ignored (they carry no codecs).
- **Quality:** the picker maps to a max height (Auto, 360–1080). The chosen
  height is stored in `<id>.quality`; asking for a different one re-downloads
  and replaces the file, asking for Auto reuses whatever is cached.
- **Management:** the Library lists cached downloads with their size and
  quality (`list_youtube_downloads`) and can delete one
  (`delete_youtube_download`, with a path-traversal-safe id check) or all
  (`clear_youtube_downloads`). Library entries and resume positions survive
  deleting the file; it is simply downloaded again on next open.

## 6. Word lookup & context-aware AI explanation

```mermaid
sequenceDiagram
    participant U as User (clicks a subtitle word)
    participant Store as player.svelte.ts
    participant Rust as Rust backend
    participant Dict as Free Dictionary API
    participant Wiki as Wikipedia
    participant Llama as llama-cli (subprocess)

    U->>Store: onWordActivate(word, sentence)
    Store->>Rust: pause()
    Store->>Rust: define_word(word)
    Rust->>Dict: GET /api/v2/entries/en/{word}
    alt found
        Dict-->>Rust: meanings, phonetic, audio_url
    else 404
        Rust->>Wiki: GET /page/summary/{word}
        Wiki-->>Rust: summary extract
    end
    Rust-->>Store: WordDefinition
    Store-->>U: popup/sidebar shows dictionary result

    opt user clicks "Explain in context" (if local LLM installed)
        Store->>Rust: llm_explain(word, sentence, modelPath)
        Rust->>Llama: spawn: -m model -st --jinja -p "prompt" ...
        Llama-->>Rust: stdout (echoed prompt + reply)
        Rust-->>Rust: clean_llm_output() slices reply<br/>from the echoed-prompt boundary
        Rust-->>Store: explanation text
    end
```

The same `wordPanel` UI (and the same backend commands) is reused in three
places: the player's sidebar (windowed mode), the fullscreen popup, and the
vocabulary review card's back panel.

## 7. Vocabulary & spaced repetition

```mermaid
flowchart LR
    add(["Add to Vocabulary<br/>(from word popup)"]) --> db[("vocabulary.sqlite3<br/>word, definition, context_sentence,<br/>ease_factor, interval_days,<br/>next_review_date, review_count")]

    db --> due["list_due_vocabulary<br/>(next_review_date IS NULL<br/>OR <= now)"]
    due --> session["Review session<br/>(vocabulary/+page.svelte)"]

    session -->|"Again (1) / Good (3) / Easy (5)"| sm2["sm2_update()<br/>SM-2 algorithm:<br/>ease/interval/due-date math"]
    sm2 --> db

    session -.->|"reuses"| aiExplain["llm_explain / llm_word_examples<br/>(same as word popup)"]
```

A lapse (rating "Again") doesn't get scheduled days out — it's reinserted a
few cards ahead in the same session queue, mirroring how physical flashcard
drills handle a miss.

## 8. Local LLM provisioning

```mermaid
flowchart TD
    subgraph setup["AI Setup modal (any page)"]
        installBin["Install engine<br/>(resolves latest llama.cpp<br/>release for this OS via GitHub API)"]
        installModel["Install a model<br/>(curated Qwen2.5 catalog,<br/>or a custom .gguf URL)"]
    end

    installBin -->|"llm_download_file<br/>(curl, progress polled via llm_file_size)"| archive["_llm_download.zip/tar.gz<br/>in app data dir"]
    archive -->|"llm_extract_downloaded_archive<br/>(tar)"| binDir["llm/llama-cli(.exe) + DLLs"]

    installModel -->|"llm_download_file"| modelFile["llm/models/*.gguf"]

    binDir --> status["llm_status<br/>(scans dirs, reports readiness)"]
    modelFile --> status
    status --> ready{"binary_exists &&<br/>models.length > 0"}
    ready -->|"yes"| enabled["'Explain in context' /<br/>'Other examples' buttons<br/>appear in word panel"]
```

## 9. Theming

```mermaid
flowchart LR
    layoutScript["+layout.svelte<br/>(runs before first paint)"] --> loadSettings["loadThemeSettings()<br/>(localStorage, first-run falls<br/>back to OS prefers-color-scheme)"]
    loadSettings --> applyTheme["applyTheme()<br/>sets html[data-theme][data-appearance]"]
    applyTheme --> css["theme.css<br/>html[data-theme=X][data-appearance=Y]<br/>{ --color-bg, --color-accent, ... }"]

    switcher["ThemeSwitcher.svelte<br/>(shared component, all 3 pages)"] -->|"pick theme / light-dark"| saveSettings["saveThemeSettings()"]
    saveSettings --> loadSettings
    switcher --> applyTheme
```

Theme and appearance (light/dark) are two independent, fully user-controlled
axes — picking a theme doesn't change light/dark, and neither re-syncs to the
OS after the first launch. The video player's own chrome stays permanently
dark regardless of the chosen app theme (`--player-*` tokens scoped to
`.player-card`, matching how YouTube/Netflix/VLC treat their controls as
part of the video, not the surrounding UI).

## 9.1 Subtitle styling

The on-video subtitle look is a set of independent settings in the
`player.svelte.ts` store: size, font, weight, text color, background color
and opacity, outline, and position (top / middle / bottom), plus four presets
(Default, High contrast, Yellow, Dark on light). `subtitleStyle.ts` turns them
into CSS custom properties (`--subtitle-color`, `--subtitle-bg-rgb`, ...) that
both the real overlay and the preview in `SubtitleStylePanel.svelte` use, so
the preview cannot drift from what is drawn. The outline color flips to white
for black text. Each setting is validated against its option list on load, and
an absent value means "default" (not zero).

## 10. Local storage summary

| Store | Contents | Written by |
|---|---|---|
| `vocabulary.sqlite3` (Rust/SQLite) | Vocabulary entries + SM-2 scheduling fields | `vocabulary.rs` |
| App data dir `youtube_cache/` | Downloaded YouTube videos: `<id>.mp4`, `<id>.en.vtt`, `<id>.info.json`, `<id>.quality` (3 GB cap, oldest evicted first) | `youtube.rs` (via `yt-dlp`) |
| App data dir `llm/` | `llama-cli` binary + `models/*.gguf` | `llm.rs` (via `curl`/`tar`) |
| `localStorage: videoLearnRecents` | Recently opened local files/YouTube links, resume position | `recents.ts` |
| `localStorage: videoLearnTheme` | Selected theme id + appearance | `themeSettings.ts` |
| `localStorage: llmModelPath`, `playbackRate`, `videoLearnPauseAfterLine` | Active LLM model, playback speed, pause-after-line toggle | `player.svelte.ts` |
| `localStorage: subtitle*` | Subtitle size, font, weight, colors, background opacity, outline, position | `player.svelte.ts` |
| `localStorage: youtubeDownloadQualityHeight`, `videoLearnYoutubeEmbedMode` | Download quality choice; "use YouTube's own player" | `player.svelte.ts` |
