//! Local LLM (llama.cpp) — context-aware word/phrase explanations.
//!
//! Unlike the Free Dictionary API (`dictionary.rs`), a local instruct model
//! can disambiguate the sense a word is used in from the surrounding subtitle
//! line, and handles idioms/phrases/slang the dictionary API has no entry
//! for at all.
//!
//! Runs a bundled `llama-cli` (llama.cpp's CLI) as a subprocess against a
//! small instruct GGUF model — fully offline, no API keys, no background
//! service to keep running. The binary + model are downloaded into the
//! app's data dir on demand (see `llm_download_file`/`llm_extract_downloaded_archive`)
//! rather than bundled with the app itself: a usable binary+model pair is
//! several hundred MB to a couple of GB, and the size/quality tradeoff is a
//! choice we don't want to make on the user's behalf.
//!
//! Layout:
//!   <app_data_dir>/llm/
//!     llama-cli.exe        (or `llama-cli` on unix)
//!     models/
//!       qwen2.5-1.5b-instruct-q4_k_m.gguf
//!       ...

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;
use tauri::Manager;

fn llm_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    Ok(base.join("llm"))
}

fn llm_binary(root: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    let name = "llama-cli.exe";
    #[cfg(not(target_os = "windows"))]
    let name = "llama-cli";
    root.join(name)
}

#[derive(Serialize)]
pub struct LlmModel {
    /// Absolute path to the .gguf — passed back verbatim to `llm_explain`.
    path: String,
    /// User-facing label (the file stem).
    name: String,
    /// On-disk size, so the UI can show "1.1 GB" next to the model name.
    size_bytes: u64,
}

#[derive(Serialize)]
pub struct LlmStatus {
    binary_exists: bool,
    binary_path: String,
    models_dir: String,
    models: Vec<LlmModel>,
}

#[tauri::command]
pub fn llm_status(app: tauri::AppHandle) -> Result<LlmStatus, String> {
    let root = llm_root(&app)?;
    let models_dir = root.join("models");
    // Create the folders on first look so "Open models folder" from the UI
    // has somewhere to open on a fresh install.
    if !root.exists() {
        fs::create_dir_all(&root).map_err(|e| format!("mkdir llm root: {e}"))?;
    }
    if !models_dir.exists() {
        fs::create_dir_all(&models_dir).map_err(|e| format!("mkdir models: {e}"))?;
    }

    let binary_path = llm_binary(&root);
    let binary_exists = binary_path.exists();

    let mut models: Vec<LlmModel> = Vec::new();
    if let Ok(read) = fs::read_dir(&models_dir) {
        for entry in read.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("gguf") {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let size_bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            models.push(LlmModel {
                path: path.to_string_lossy().into_owned(),
                name,
                size_bytes,
            });
        }
    }
    models.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(LlmStatus {
        binary_exists,
        binary_path: binary_path.to_string_lossy().into_owned(),
        models_dir: models_dir.to_string_lossy().into_owned(),
        models,
    })
}

/// Download `url` to `<llm_root>/<rel_path>` via the system `curl`.
///
/// **Async on purpose:** curl can run for many seconds on a large model
/// download, and Tauri sync commands run on the main thread — which would
/// freeze the whole window. Wrapping the blocking subprocess in
/// `spawn_blocking` lets the async runtime keep servicing other commands
/// (in particular the JS side's file-size progress poll) while curl runs.
///
/// curl handles the GitHub/HuggingFace redirect chains transparently and
/// ships with Windows 10 1803+ / macOS / Linux, so no new dependency here.
#[tauri::command]
pub async fn llm_download_file(
    app: tauri::AppHandle,
    url: String,
    rel_path: String,
) -> Result<u64, String> {
    let root = llm_root(&app)?;
    let dest = root.join(&rel_path);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    if dest.exists() {
        let _ = fs::remove_file(&dest);
    }

    let dest_for_task = dest.clone();
    let output = tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = Command::new("curl");
        cmd.arg("-L") // follow redirects (GitHub/HuggingFace → CDN)
            .arg("--fail")
            .arg("--silent")
            .arg("--show-error")
            .arg("--connect-timeout").arg("30")
            .arg("--retry").arg("2")
            .arg("--output").arg(&dest_for_task)
            .arg(&url);

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        cmd.output()
    })
    .await
    .map_err(|e| format!("task join: {e}"))?
    .map_err(|e| format!("failed to spawn curl (is it installed?): {e}"))?;

    if !output.status.success() {
        let _ = fs::remove_file(&dest);
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!(
            "curl exit {}: {}",
            output.status.code().unwrap_or(-1),
            if stderr.trim().is_empty() { "no output" } else { stderr.trim() }
        ));
    }

    let size = fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
    if size == 0 {
        let _ = fs::remove_file(&dest);
        return Err("download produced an empty file".into());
    }
    Ok(size)
}

/// Report the size of a file inside the llm root — used by the JS side to
/// poll progress while `llm_download_file` is in flight. Returns 0 (not an
/// error) if the file doesn't exist yet, since that's the normal state right
/// before curl writes its first bytes.
#[tauri::command]
pub fn llm_file_size(app: tauri::AppHandle, rel_path: String) -> Result<u64, String> {
    let root = llm_root(&app)?;
    let path = root.join(&rel_path);
    if !path.exists() {
        return Ok(0);
    }
    fs::metadata(&path).map(|m| m.len()).map_err(|e| format!("stat: {e}"))
}

/// Extract a downloaded llama.cpp release archive in the llm root. Uses the
/// system `tar` (bsdtar on Windows 10+ / macOS / Linux) so we don't need to
/// pull a zip/tar crate into the bundle. llama.cpp's release archives
/// extract flat (no wrapping top-level folder), so no hoist step is needed.
#[tauri::command]
pub async fn llm_extract_downloaded_archive(
    app: tauri::AppHandle,
    archive_name: String,
) -> Result<(), String> {
    let root = llm_root(&app)?;
    let archive = root.join(&archive_name);
    if !archive.exists() {
        return Err(format!("archive not found: {}", archive.to_string_lossy()));
    }

    let root_for_task = root.clone();
    let archive_for_task = archive.clone();
    let output = tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = Command::new("tar");
        cmd.arg("-xf").arg(&archive_for_task).arg("-C").arg(&root_for_task);

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }

        cmd.output()
    })
    .await
    .map_err(|e| format!("task join: {e}"))?
    .map_err(|e| format!("failed to run tar: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "tar exit {}: {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let _ = fs::remove_file(&archive);
    Ok(())
}

/// Extract just the model's answer from llama-cli's stdout.
///
/// In single-turn mode the CLI prints a startup banner, echoes the user turn
/// (prefixed with `> `), then the generated reply, then a stats footer
/// (`[ Prompt: … ]`) and `Exiting…`. Since we know the exact `prompt` we
/// sent, the reply is reliably the text *after* the last echo of that prompt
/// and *before* the footer — that boundary survives banner changes across
/// llama.cpp builds far better than trying to blocklist banner lines.
fn clean_llm_output(raw: &str, prompt: &str) -> String {
    let after_prompt = raw
        .rfind(prompt)
        .map(|i| &raw[i + prompt.len()..])
        .or_else(|| {
            // Fallback: no verbatim prompt match. The interactive echo is a
            // single line beginning with "\n> "; the reply starts on the next
            // line, so skip past the newline that ends the echoed prompt line.
            raw.rfind("\n> ").map(|i| {
                let rest = &raw[i + 3..];
                rest.find('\n').map_or(rest, |nl| &rest[nl + 1..])
            })
        })
        .unwrap_or(raw);

    let mut out = after_prompt.to_string();
    for marker in [
        "[ Prompt:", "\nExiting", "Exiting...", "[end of text]",
        "<|im_end|>", "<|eot_id|>", "</s>", "<end_of_turn>",
    ] {
        if let Some(idx) = out.find(marker) {
            out.truncate(idx);
        }
    }

    out.trim().trim_start_matches('>').trim().to_string()
}

/// Drives `llama-cli` in single-turn mode with `--jinja` so the model's own
/// chat template (baked into the GGUF) is applied — that keeps instruct
/// models behaving correctly without hardcoding a per-model prompt format.
/// stdin is wired to /dev/null so a build that doesn't understand `-st` gets
/// EOF and exits rather than hanging forever waiting for interactive input.
fn run_llama_cli(binary: &Path, model: &Path, root: &Path, prompt: &str) -> Result<String, String> {
    let mut cmd = Command::new(binary);
    cmd.arg("-m").arg(model)
        .arg("-st") // single user turn, then exit (no interactive loop)
        .arg("--jinja") // apply the model's embedded chat template
        .arg("--simple-io") // plain IO for subprocesses: no ANSI/cursor codes
        .arg("--no-display-prompt")
        .arg("-p").arg(prompt)
        .arg("-n").arg("220")
        .arg("-c").arg("4096")
        .arg("--temp").arg("0.2")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Keep model resolution of any sidecar files relative to the install root.
    cmd.current_dir(root);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let output = cmd.output().map_err(|e| format!("spawn llama-cli: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr.lines().rev().take(4).collect::<Vec<_>>().join(" | ");
        return Err(format!(
            "llama-cli exit {}: {}",
            output.status.code().unwrap_or(-1),
            if tail.trim().is_empty() { "no output" } else { tail.trim() }
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Validates the llama-cli binary + model exist and the word isn't blank —
/// shared precondition for every local-LLM word command.
fn llm_preflight(
    app: &tauri::AppHandle,
    word: &str,
    model_path: &str,
) -> Result<(PathBuf, PathBuf, PathBuf, String), String> {
    let root = llm_root(app)?;
    let binary = llm_binary(&root);
    if !binary.exists() {
        return Err(format!("llama-cli not found at {}", binary.to_string_lossy()));
    }
    let model = PathBuf::from(model_path);
    if !model.exists() {
        return Err(format!("model not found at {model_path}"));
    }
    let word = word.trim().to_string();
    if word.is_empty() {
        return Err("no word/phrase to explain".into());
    }
    Ok((root, binary, model, word))
}

/// Ask the local model to explain `word` *as it is used in* `sentence`.
///
/// This backs the word popup's "Explain in context" action: unlike the Free
/// Dictionary API, it disambiguates the sense in context and can handle
/// idioms, phrases, and slang the dictionary API has no entry for at all.
///
/// Async + spawn_blocking: a 1-2B model on CPU takes a few seconds to
/// answer, and we can't block the UI thread that long.
#[tauri::command]
pub async fn llm_explain(
    app: tauri::AppHandle,
    word: String,
    sentence: String,
    model_path: String,
) -> Result<String, String> {
    let (root, binary, model, word) = llm_preflight(&app, &word, &model_path)?;

    // Fold instruction + context into one user turn; `--jinja` wraps it in
    // the model's chat template. Kept to a single line (no embedded
    // newlines): the CLI echoes the prompt verbatim and clean_llm_output
    // slices on that echo, which is most reliable when the prompt is one
    // line, so we collapse any newlines in the sentence too.
    let sentence_flat = sentence.trim().split_whitespace().collect::<Vec<_>>().join(" ");
    let prompt = if sentence_flat.is_empty() {
        format!(
            "Explain the word or phrase '{word}' concisely for a language learner. Give its \
meaning in 1-2 plain sentences, then one short example of use. Do not repeat this instruction."
        )
    } else {
        format!(
            "In the sentence below, explain what '{word}' means as it is used here, for a \
language learner. Give the specific sense in 1-2 plain sentences. If it is an idiom, figure of \
speech, or slang, say so and give the plain meaning. Be concise and do not repeat the sentence \
back. Sentence: {sentence_flat}"
        )
    };

    let binary_for_task = binary.clone();
    let model_for_task = model.clone();
    let root_for_task = root.clone();
    let prompt_for_task = prompt.clone(); // keep `prompt` for clean_llm_output below

    let raw = tauri::async_runtime::spawn_blocking(move || {
        run_llama_cli(&binary_for_task, &model_for_task, &root_for_task, &prompt_for_task)
    })
    .await
    .map_err(|e| format!("task join: {e}"))??;

    let cleaned = clean_llm_output(&raw, &prompt);
    if cleaned.is_empty() {
        return Err("model returned an empty answer".into());
    }
    Ok(cleaned)
}

/// Ask the local model for a few example sentences that use `word` in
/// contexts/senses *other than* the one the viewer is currently looking at
/// (`sentence`, used only to steer the model away from repeating it) — backs
/// the word popup's "Other examples" action, for viewers who want a feel for
/// a word beyond its one occurrence in this subtitle line.
#[tauri::command]
pub async fn llm_word_examples(
    app: tauri::AppHandle,
    word: String,
    sentence: String,
    model_path: String,
) -> Result<String, String> {
    let (root, binary, model, word) = llm_preflight(&app, &word, &model_path)?;

    let sentence_flat = sentence.trim().split_whitespace().collect::<Vec<_>>().join(" ");
    let avoid = if sentence_flat.is_empty() {
        String::new()
    } else {
        format!(" Don't reuse this sentence: \"{sentence_flat}\".")
    };
    let prompt = format!(
        "Give 3 short example sentences that each use the word or phrase '{word}' in a \
different context or sense than the others, to help a language learner see how it's used. \
Number them 1-3, one sentence per line, with no extra commentary before or after.{avoid}"
    );

    let binary_for_task = binary.clone();
    let model_for_task = model.clone();
    let root_for_task = root.clone();
    let prompt_for_task = prompt.clone();

    let raw = tauri::async_runtime::spawn_blocking(move || {
        run_llama_cli(&binary_for_task, &model_for_task, &root_for_task, &prompt_for_task)
    })
    .await
    .map_err(|e| format!("task join: {e}"))??;

    let cleaned = clean_llm_output(&raw, &prompt);
    if cleaned.is_empty() {
        return Err("model returned an empty answer".into());
    }
    Ok(cleaned)
}
