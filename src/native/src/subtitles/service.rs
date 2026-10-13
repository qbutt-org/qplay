use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::watch;

use super::cache::SubtitleCacheError;
use super::{
    SubtitleCache, SubtitleDocument, SubtitleError, SubtitleFormat, SubtitleTranslationOptions,
    TranslatedSubtitleBatch, pending_batch, translate_batch, window_bounds,
};

#[derive(Debug, thiserror::Error)]
pub enum SubtitleServiceError {
    #[error(transparent)]
    Cache(#[from] SubtitleCacheError),
    #[error(transparent)]
    Document(#[from] SubtitleError),
    #[error(transparent)]
    Codex(#[from] qbutt_codex::CodexError),
    #[error("The subtitle translation job belongs to an old player generation")]
    Stale,
    #[error("The subtitle translation service is unavailable")]
    Unavailable,
    #[error("The translated subtitle could not be prepared for playback")]
    Playback(#[source] std::io::Error),
}

/// One qplay owner serializes cache admission and guards player generations.
/// Call methods on a worker; filesystem access must not block the Qt thread.
pub struct SubtitleService {
    cache: Mutex<SubtitleCache>,
    generation: AtomicU64,
    output_dir: PathBuf,
}

impl SubtitleService {
    pub fn new(data_dir: &Path) -> Result<Self, SubtitleServiceError> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| SubtitleServiceError::Unavailable)?
            .as_nanos();
        Ok(Self {
            cache: Mutex::new(SubtitleCache::new(data_dir)?),
            generation: AtomicU64::new(0),
            output_dir: data_dir
                .join("subtitle-session")
                .join(format!("{}-{nonce}", std::process::id())),
        })
    }

    pub fn output_path(&self, generation: u64, format: SubtitleFormat) -> PathBuf {
        let extension = match format {
            SubtitleFormat::Ass => "ass",
            SubtitleFormat::Ssa => "ssa",
            SubtitleFormat::Srt => "srt",
            SubtitleFormat::Vtt => "vtt",
        };
        self.output_dir.join(format!("{generation}.{extension}"))
    }

    pub fn owns_output_path(&self, path: &Path) -> bool {
        path.parent() == Some(self.output_dir.as_path())
    }

    /// Qt only changes the generation atomically. Cache and file I/O stay on
    /// workers, and every accepted cue is checked against this generation.
    pub fn begin_generation(&self) -> u64 {
        self.generation
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1)
    }

    /// Return only accepted text for the current read-ahead window.
    pub fn cached_window(
        &self,
        document: &SubtitleDocument,
        position_ms: i64,
        options: &SubtitleTranslationOptions,
        generation: u64,
    ) -> Result<HashMap<String, String>, SubtitleServiceError> {
        let (start, end) = window_bounds(position_ms)?;
        if self.generation.load(Ordering::SeqCst) != generation {
            return Err(SubtitleServiceError::Stale);
        }
        let cache = self
            .cache
            .lock()
            .map_err(|_| SubtitleServiceError::Unavailable)?;
        if self.generation.load(Ordering::SeqCst) != generation {
            return Err(SubtitleServiceError::Stale);
        }
        let mut accepted = HashMap::new();
        for fragment in document
            .fragments()
            .iter()
            .filter(|fragment| fragment.end_ms > start && fragment.start_ms < end)
        {
            if let Some(text) = cache.read(&options.key(fragment))? {
                accepted.insert(fragment.id.clone(), text);
            }
        }
        Ok(accepted)
    }

    /// Write a new generation's file only once, before mpv is told its path.
    /// This leaves every file mpv can read immutable.
    pub fn publish_window(
        &self,
        document: &SubtitleDocument,
        accepted: &HashMap<String, String>,
        path: &Path,
        generation: u64,
    ) -> Result<(), SubtitleServiceError> {
        if path != self.output_path(generation, document.format) {
            return Err(SubtitleServiceError::Unavailable);
        }
        let rendered = document.render(accepted)?;
        if self.generation.load(Ordering::SeqCst) != generation {
            return Err(SubtitleServiceError::Stale);
        }
        fs::create_dir_all(path.parent().ok_or(SubtitleServiceError::Unavailable)?)
            .map_err(SubtitleServiceError::Playback)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(SubtitleServiceError::Playback)?;
        file.write_all(rendered.as_bytes())
            .map_err(SubtitleServiceError::Playback)?;
        file.sync_all().map_err(SubtitleServiceError::Playback)?;
        if self.generation.load(Ordering::SeqCst) != generation {
            return Err(SubtitleServiceError::Stale);
        }
        Ok(())
    }

    /// Translate the next bounded uncached batch. `None` means every cue in
    /// this window is already accepted.
    pub async fn translate_window(
        &self,
        document: &SubtitleDocument,
        position_ms: i64,
        options: &SubtitleTranslationOptions,
        generation: u64,
        cancelled: watch::Receiver<bool>,
    ) -> Result<Option<TranslatedSubtitleBatch>, SubtitleServiceError> {
        let batch = {
            let cache = self
                .cache
                .lock()
                .map_err(|_| SubtitleServiceError::Unavailable)?;
            if self.generation.load(Ordering::SeqCst) != generation {
                return Err(SubtitleServiceError::Stale);
            }
            let (start, end) = window_bounds(position_ms)?;
            let mut accepted = HashMap::new();
            for fragment in document.fragments().iter().filter(|fragment| {
                fragment.end_ms > start.saturating_sub(45_000) && fragment.start_ms < end
            }) {
                if let Some(text) = cache.read(&options.key(fragment))? {
                    accepted.insert(fragment.id.clone(), text);
                }
            }
            pending_batch(document, position_ms, &accepted)?
        };
        if self.generation.load(Ordering::SeqCst) != generation {
            return Err(SubtitleServiceError::Stale);
        }
        if batch.fragments.is_empty() {
            return Ok(None);
        }
        let result = translate_batch(&batch, options, cancelled.clone()).await?;
        if *cancelled.borrow() {
            return Err(qbutt_codex::CodexError::Cancelled.into());
        }
        {
            let mut cache = self
                .cache
                .lock()
                .map_err(|_| SubtitleServiceError::Unavailable)?;
            for (key, text) in &result.accepted {
                // This check admits one cue. Cancellation after admission
                // cannot make the GUI wait for the atomic file publication.
                if self.generation.load(Ordering::SeqCst) != generation {
                    return Err(SubtitleServiceError::Stale);
                }
                cache.accept(key, text)?;
            }
        }
        Ok(Some(result))
    }
}

impl Drop for SubtitleService {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.output_dir);
    }
}
