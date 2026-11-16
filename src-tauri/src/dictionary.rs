use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone, Debug)]
pub struct DefinitionMeaning {
    pub part_of_speech: String,
    /// Capped to 2 per meaning — matches the "more definitions at a glance"
    /// breadth of the sibling epub-reader app's dictionary popover rather
    /// than dumping every sense the API has.
    pub definitions: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct WordDefinition {
    pub word: String,
    pub phonetic: Option<String>,
    /// A real recorded pronunciation clip, when the dictionary API has one
    /// for this word — not every entry includes audio.
    pub audio_url: Option<String>,
    /// Capped to 2 meanings (part-of-speech groups). Empty when this result
    /// came from the Wikipedia fallback instead (see `summary`).
    pub meanings: Vec<DefinitionMeaning>,
    /// Set only for the Wikipedia fallback path, when the word isn't in the
    /// dictionary at all (proper nouns, technical terms, etc.).
    pub summary: Option<String>,
}

#[derive(Deserialize)]
struct ApiEntry {
    word: String,
    phonetic: Option<String>,
    #[serde(default)]
    phonetics: Vec<ApiPhonetic>,
    meanings: Vec<ApiMeaning>,
}

#[derive(Deserialize)]
struct ApiPhonetic {
    text: Option<String>,
    audio: Option<String>,
}

#[derive(Deserialize)]
struct ApiMeaning {
    #[serde(rename = "partOfSpeech")]
    part_of_speech: String,
    definitions: Vec<ApiDefinition>,
}

#[derive(Deserialize)]
struct ApiDefinition {
    definition: String,
}

#[derive(Deserialize)]
struct WikiSummary {
    title: String,
    extract: Option<String>,
}

/// Strips leading/trailing punctuation a subtitle word might carry (e.g. a
/// trailing period or wrapping quotes) while keeping internal apostrophes
/// so contractions like "don't" survive.
pub fn clean_word(raw: &str) -> String {
    raw.trim()
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

pub async fn lookup(client: &reqwest::Client, word: &str) -> Result<WordDefinition, String> {
    let url = format!("https://api.dictionaryapi.dev/api/v2/entries/en/{word}");

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|_| "Couldn't reach the dictionary service. Check your connection.".to_string())?;

    // Proper nouns, technical terms, and anything else absent from the
    // dictionary fall back to a Wikipedia summary rather than a dead end.
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return lookup_wikipedia(client, word).await;
    }
    if !response.status().is_success() {
        return Err("Couldn't reach the dictionary service. Check your connection.".to_string());
    }

    let entries: Vec<ApiEntry> = response
        .json()
        .await
        .map_err(|_| "Couldn't read the dictionary response.".to_string())?;

    let Some(entry) = entries.first() else {
        return lookup_wikipedia(client, word).await;
    };

    let meanings: Vec<DefinitionMeaning> = entry
        .meanings
        .iter()
        .take(2)
        .map(|m| DefinitionMeaning {
            part_of_speech: m.part_of_speech.clone(),
            definitions: m.definitions.iter().take(2).map(|d| d.definition.clone()).collect(),
        })
        .collect();

    // Audio isn't always on the first entry — check every returned entry's
    // phonetics list for the first non-empty clip.
    let audio_url = entries
        .iter()
        .flat_map(|e| e.phonetics.iter())
        .find_map(|p| p.audio.clone().filter(|a| !a.is_empty()));

    let phonetic = entry
        .phonetic
        .clone()
        .or_else(|| entry.phonetics.iter().find_map(|p| p.text.clone()));

    Ok(WordDefinition {
        word: entry.word.clone(),
        phonetic,
        audio_url,
        meanings,
        summary: None,
    })
}

async fn lookup_wikipedia(client: &reqwest::Client, word: &str) -> Result<WordDefinition, String> {
    let title = word.replace(' ', "_");
    let url = format!("https://en.wikipedia.org/api/rest_v1/page/summary/{title}");

    let response = client
        .get(&url)
        .header("User-Agent", "video-learn (github.com)")
        .send()
        .await
        .map_err(|_| format!("No definition found for \"{word}\"."))?;

    if !response.status().is_success() {
        return Err(format!("No definition found for \"{word}\"."));
    }

    let summary: WikiSummary = response
        .json()
        .await
        .map_err(|_| format!("No definition found for \"{word}\"."))?;

    let extract = summary
        .extract
        .filter(|e| !e.trim().is_empty())
        .ok_or_else(|| format!("No definition found for \"{word}\"."))?;

    Ok(WordDefinition {
        word: summary.title,
        phonetic: None,
        audio_url: None,
        meanings: Vec::new(),
        summary: Some(extract),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_punctuation_keeps_apostrophes() {
        assert_eq!(clean_word("\"Hello,\""), "hello");
        assert_eq!(clean_word("don't."), "don't");
        assert_eq!(clean_word("  World!  "), "world");
    }
}
