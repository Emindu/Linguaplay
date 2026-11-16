# Language Learning Player — Build Plan

A Rust/Tauri desktop app: pause-and-define subtitle words, personal vocabulary bank, local-LLM context-aware meanings, spaced-repetition review. Built on libVLC.

---

## Phase 0 — Environment Setup (Day 1)

1. Install Rust toolchain (`rustup`)
2. Install VLC itself (needed for libVLC shared libraries) — Tauri app will link against it
3. Install Node.js (for the Tauri frontend tooling)
4. Install Tauri CLI: `cargo install create-tauri-app --locked`
5. Scaffold project: `cargo create-tauri-app` → choose frontend framework (Svelte recommended — lightweight, easy reactive UI for word-click handling)
6. Verify `vlc-rs` compiles and links against your local libVLC install (this is usually the first real friction point — confirm early before building anything else)
7. Add `reqwest` for the Free Dictionary API call (used in Phase 3)

**Goal of this phase:** Tauri + vlc-rs compile and run a "hello world" before you wire anything together. **No LLM setup yet** — `llama-cpp-2` and GGUF model download are deferred to Phase 5, once the basic dictionary + vocab features work end-to-end.

---

## Phase 1 — Basic Video Playback (Days 2–4)

1. Wire `vlc-rs` into the Tauri Rust backend: open a video file, play/pause/seek
2. Expose Tauri commands (`play`, `pause`, `seek`, `load_file`) callable from the frontend
3. Build minimal frontend: file picker + video controls (play/pause/seek bar)
4. Render actual video frames — decide whether to use libVLC's native window embedding or pipe frames to a canvas (native embedding is simpler; start there)

**Milestone:** you can open a video file and play/pause it from the Tauri UI.

---

## Phase 2 — Subtitle Parsing & Rendering (Days 5–8)

1. Write/integrate `.srt` and `.vtt` parser → structured data: `{ start_time, end_time, text }` per line
2. Sync subtitle display to video playback time (poll current position from `vlc-rs`, show matching subtitle line)
3. Render subtitle text in the frontend as individual clickable `<span>` elements per word (not one text blob) — this is what enables word-level click detection
4. Style the overlay to sit on top of the video

**Milestone:** subtitles appear in sync with video, and each word is individually clickable (test with a `console.log` on click before building the popup).

---

## Phase 3 — Pause-and-Define Core Feature (Days 9–12)

1. On word click: call `pause()` via Tauri command, capture `{ word, full_sentence, timestamp, video_source }`
2. Build a static/offline dictionary fallback first (bundle a compact dataset like a Wiktionary/WordNet extract) — gives instant results while LLM path is being built
3. Show a definition popup near the clicked word with the static definition
4. Add "Resume" button to un-pause

**Milestone:** click any subtitle word → video pauses → popup shows a definition → resume works.

---

## Phase 4 — Vocabulary Storage (Days 13–15)

1. Design SQLite schema via `rusqlite`:
   ```sql
   CREATE TABLE vocabulary (
     id INTEGER PRIMARY KEY,
     word TEXT NOT NULL,
     definition TEXT,
     context_sentence TEXT,
     source_video TEXT,
     timestamp_ms INTEGER,
     date_added TEXT,
     ease_factor REAL DEFAULT 2.5,
     interval_days INTEGER DEFAULT 0,
     next_review_date TEXT,
     review_count INTEGER DEFAULT 0
   );
   ```
2. Add "+ Add to Vocabulary" button in the definition popup → writes a row
3. Build a simple vocabulary list screen (frontend route) showing all saved words, sortable/searchable

**Milestone:** words saved from the popup persist and show up in a vocabulary list.

---

## Phase 5 — Local LLM Context-Aware Meaning (Days 16–21)

1. Install `llama-cpp-2`, download one small GGUF model (e.g. Qwen2.5-3B-Instruct-Q4_K_M.gguf)
2. Load the model once at app startup (keep it resident in memory — reloading per-query is slow)
3. Write the prompt template:
   > "Define '{word}' as used in this sentence: '{sentence}'. Give a short, learner-friendly definition (1 sentence) and one example sentence."
4. Add a Tauri command `get_contextual_meaning(word, sentence)` → runs inference → returns structured text
5. Update the popup: show static dictionary definition instantly, then replace/augment with LLM contextual definition once inference completes (async, non-blocking)
6. Add basic caching (word + sentence hash → cached LLM response) so repeated lookups don't re-run inference
7. Optionally: let saved vocabulary entries be re-annotated with the contextual LLM definition after the fact

**Milestone:** popup shows a definition that reflects the word's actual meaning *in that sentence*, not just a generic dictionary entry.

---

## Phase 6 — Review System / Spaced Repetition (Days 22–26)

1. Implement SM-2 algorithm (same as Anki) in Rust: given a review grade (again/hard/good/easy), update `ease_factor`, `interval_days`, `next_review_date`
2. Build review screen: fetch words due today (`next_review_date <= now`), show sentence with word blanked out, user recalls meaning, self-grades
3. Wire grading buttons → update SM-2 fields in SQLite
4. Add a small dashboard: words due today, streak, total vocab count

**Milestone:** full loop works — watch video → learn word → review it later with spaced repetition.

---

## Phase 7 — Polish & Packaging (Days 27–30)

1. Error handling: missing subtitle file, LLM load failure, corrupt video, etc. — graceful fallbacks everywhere
2. Settings screen: choose LLM model, subtitle language, review reminder frequency
3. Tauri bundling for Windows/macOS/Linux (`tauri build`) — test that libVLC dependency is either bundled or clearly documented as a prerequisite
4. Write README with setup instructions (VLC install, model download)
5. Basic onboarding flow (first-run: pick video, load model, done)

**Milestone:** installable app on all three platforms with a README a stranger could follow.

---

## Phase 8 — Open Source Release

1. Pick a license (GPLv2/LGPL-compatible if you use vlc-rs/libVLC — check current vlc-rs license terms before release)
2. Clean repo structure, CONTRIBUTING.md, issue templates
3. Publish on GitHub, write a short demo GIF/video for the README
4. Optional: post in VideoLAN forums/communities as a companion tool (not a VLC core patch) — this avoids the slower upstream review process while still crediting/building on VLC

---

## Suggested order of "hardest problem first" de-risking

If you want to validate feasibility before committing to the full plan, do these two spikes first, in isolation:
1. **vlc-rs + libVLC linking** on your actual target OS — dependency/linking issues are the most common blocker
2. **Word-level clickable subtitle rendering** in your chosen frontend — confirms the UX centerpiece works before building everything around it

(The `llama-cpp-2` inference-speed spike is deferred until Phase 5, once the core dictionary/vocab loop is proven.)

---

## Summary Timeline

| Phase | Focus | Duration |
|---|---|---|
| 0 | Environment setup (no LLM yet) | 1 day |
| 1 | Video playback | 3 days |
| 2 | Subtitle rendering | 4 days |
| 3 | Pause-and-define (basic dictionary) | 4 days |
| 4 | Vocabulary storage | 3 days |
| 5 | Local LLM context meaning | 6 days |
| 6 | Review/spaced repetition | 5 days |
| 7 | Polish & packaging | 4 days |
| 8 | Open source release | ongoing |

**Total: ~4–5 weeks** for a solo builder working steadily, longer if this is part-time.
