//! qplay-owned accepted text. File names are hashes of exact typed keys.
//! Media paths, URLs, cookies and subtitle source text never enter this store.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const PIPELINE_VERSION: u32 = 1;
const MAX_TEXT_BYTES: usize = 64 * 1024;
static NEXT_PENDING: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, thiserror::Error)]
pub enum SubtitleCacheError {
    #[error("The subtitle cache key or text is invalid")]
    Invalid,
    #[error("An accepted subtitle translation has changed or become corrupt")]
    Conflict,
    #[error("The subtitle cache could not be read or written")]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubtitleIdentity {
    pub media_digest: String,
    pub stream_index: Option<u32>,
    /// Full external subtitle digest, or stable embedded stream descriptor digest.
    pub source_digest: String,
}

impl SubtitleIdentity {
    fn valid(&self) -> bool {
        hex_digest(&self.media_digest)
            && hex_digest(&self.source_digest)
            && self.stream_index.is_none_or(|index| index <= 512)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubtitleCacheKey {
    pub identity: SubtitleIdentity,
    pub fragment_id: String,
    pub target_language: String,
    pub model: String,
    pub effort: String,
    pub context_revision: u64,
    pipeline: u32,
}

impl SubtitleCacheKey {
    pub fn new(
        identity: SubtitleIdentity,
        fragment_id: String,
        target_language: String,
        model: String,
        effort: String,
        context_revision: u64,
    ) -> Self {
        Self {
            identity,
            fragment_id,
            target_language,
            model,
            effort,
            context_revision,
            pipeline: PIPELINE_VERSION,
        }
    }

    pub fn validate(&self) -> Result<(), SubtitleCacheError> {
        let fragment = self.fragment_id.split_once(':');
        if !self.identity.valid()
            || !fragment.is_some_and(|(digest, index)| {
                hex_digest(digest)
                    && !index.is_empty()
                    && index.bytes().all(|byte| byte.is_ascii_digit())
            })
            || !matches!(self.target_language.as_str(), "ru" | "en")
            || !model_token(&self.model, 128)
            || !model_token(&self.effort, 32)
            || self.pipeline != PIPELINE_VERSION
        {
            return Err(SubtitleCacheError::Invalid);
        }
        Ok(())
    }

    fn digest(&self) -> Result<String, SubtitleCacheError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| SubtitleCacheError::Invalid)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

pub struct SubtitleCache {
    root: PathBuf,
}

impl SubtitleCache {
    /// `data_dir` is the qplay profile's data directory, never a media path.
    pub fn new(data_dir: &Path) -> Result<Self, SubtitleCacheError> {
        if !data_dir.is_absolute() {
            return Err(SubtitleCacheError::Invalid);
        }
        Ok(Self {
            root: data_dir.join("subtitle-cache/v1"),
        })
    }

    pub fn read(&self, key: &SubtitleCacheKey) -> Result<Option<String>, SubtitleCacheError> {
        let digest = key.digest()?;
        let path = self.path(&digest);
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if bytes.len() > MAX_TEXT_BYTES + 16 {
            return Err(SubtitleCacheError::Conflict);
        }
        let text: String =
            serde_json::from_slice(&bytes).map_err(|_| SubtitleCacheError::Conflict)?;
        if !valid_text(&text) {
            return Err(SubtitleCacheError::Conflict);
        }
        Ok(Some(text))
    }

    /// Called through the single subtitle service owner after generation validation.
    pub fn accept(&mut self, key: &SubtitleCacheKey, text: &str) -> Result<(), SubtitleCacheError> {
        if !valid_text(text) {
            return Err(SubtitleCacheError::Invalid);
        }
        let digest = key.digest()?;
        if let Some(existing) = self.read(key)? {
            return if existing == text {
                Ok(())
            } else {
                Err(SubtitleCacheError::Conflict)
            };
        }
        let path = self.path(&digest);
        let bytes = serde_json::to_vec(text).map_err(|_| SubtitleCacheError::Invalid)?;
        let result = self.write(&path, &bytes);
        if result.is_err() {
            if let Some(existing) = self.read(key)? {
                return if existing == text {
                    Ok(())
                } else {
                    Err(SubtitleCacheError::Conflict)
                };
            }
        }
        result
    }

    pub(super) fn resolved_selection(
        &self,
        identity: &SubtitleIdentity,
        target: &str,
        requested_model: Option<&str>,
        requested_effort: Option<&str>,
    ) -> Result<Option<(String, String)>, SubtitleCacheError> {
        let path = self.selection_path(identity, target, requested_model, requested_effort)?;
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if bytes.len() > 512 {
            return Err(SubtitleCacheError::Conflict);
        }
        let (model, effort): (String, String) =
            serde_json::from_slice(&bytes).map_err(|_| SubtitleCacheError::Conflict)?;
        if !model_token(&model, 128)
            || !model_token(&effort, 32)
            || requested_model.is_some_and(|requested| requested != model)
            || requested_effort.is_some_and(|requested| requested != effort)
        {
            return Err(SubtitleCacheError::Conflict);
        }
        Ok(Some((model, effort)))
    }

    pub(super) fn remember_selection(
        &mut self,
        accepted: &SubtitleCacheKey,
        requested_model: Option<&str>,
        requested_effort: Option<&str>,
    ) -> Result<(), SubtitleCacheError> {
        if self.read(accepted)?.is_none()
            || requested_model.is_some_and(|model| model != accepted.model)
            || requested_effort.is_some_and(|effort| effort != accepted.effort)
        {
            return Err(SubtitleCacheError::Invalid);
        }
        let path = self.selection_path(
            &accepted.identity,
            &accepted.target_language,
            requested_model,
            requested_effort,
        )?;
        let bytes = serde_json::to_vec(&(&accepted.model, &accepted.effort))
            .map_err(|_| SubtitleCacheError::Invalid)?;
        self.write(&path, &bytes)
    }

    fn selection_path(
        &self,
        identity: &SubtitleIdentity,
        target: &str,
        requested_model: Option<&str>,
        requested_effort: Option<&str>,
    ) -> Result<PathBuf, SubtitleCacheError> {
        if !identity.valid()
            || !matches!(target, "ru" | "en")
            || requested_model.is_some_and(|model| !model_token(model, 128))
            || requested_effort.is_some_and(|effort| !model_token(effort, 32))
        {
            return Err(SubtitleCacheError::Invalid);
        }
        let bytes = serde_json::to_vec(&(
            PIPELINE_VERSION,
            identity,
            target,
            requested_model,
            requested_effort,
        ))
        .map_err(|_| SubtitleCacheError::Invalid)?;
        let digest = format!("{:x}", Sha256::digest(bytes));
        Ok(self.root.join("selections").join(format!("{digest}.json")))
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), SubtitleCacheError> {
        fs::create_dir_all(path.parent().ok_or(SubtitleCacheError::Invalid)?)?;
        let pending = path.with_extension(format!(
            "{}.{}.tmp",
            std::process::id(),
            NEXT_PENDING.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&pending)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&pending, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&pending);
        }
        result.map_err(SubtitleCacheError::Io)
    }

    fn path(&self, digest: &str) -> PathBuf {
        self.root.join(&digest[..2]).join(format!("{digest}.json"))
    }
}

fn valid_text(text: &str) -> bool {
    !text.is_empty() && text.len() <= MAX_TEXT_BYTES && !text.chars().any(char::is_control)
}

fn hex_digest(value: &str) -> bool {
    value.len() == 64 && hex_string(value)
}

fn hex_string(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn model_token(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}
