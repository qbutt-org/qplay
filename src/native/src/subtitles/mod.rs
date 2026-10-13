//! Native timed-text translation owned by qplay.

mod cache;
mod document;
mod media;
mod network;
mod service;
mod source;
mod translation;

pub use cache::{SubtitleCache, SubtitleCacheKey, SubtitleIdentity};
pub use document::{SubtitleDocument, SubtitleError, SubtitleFormat, SubtitleFragment};
pub use media::PlaybackMedia;
pub use service::{SubtitleService, SubtitleServiceError};
pub use source::{
    EmbeddedTextTrack, MpvEmbeddedTrack, SubtitleSourceError, VerifiedMediaTools,
    digest_local_media, extract_local_embedded, read_local_external, verify_embedded_track,
};
pub use translation::{
    SubtitleBatch, SubtitleTranslationOptions, TranslatedSubtitleBatch, pending_batch,
    translate_batch, window_bounds,
};
