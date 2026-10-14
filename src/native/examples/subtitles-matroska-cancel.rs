//! Cancellation while an indexed Matroska HTTP page is pending.

use std::sync::Arc;
use std::time::Duration;

use qplay_native::subtitles::{
    MpvEmbeddedTrack, PlaybackMedia, SubtitleSourceError, extract_network_matroska,
};
use tokio::sync::watch;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let url = std::env::args().nth(1).expect("fixture URL");
    let media = Arc::new(PlaybackMedia::new(
        url,
        vec![
            "Authorization: fixture-authorization".into(),
            "Cookie: fixture-cookie".into(),
        ],
        None,
    ));
    let (sender, cancelled) = watch::channel(false);
    let digest = media.get(cancelled.clone()).await.expect("media identity");
    let task = tokio::spawn(extract_network_matroska(
        media,
        MpvEmbeddedTrack {
            mpv_id: 1,
            ff_index: None,
            src_id: Some(2),
            codec: "ass".into(),
            current_demuxer: "mkv".into(),
            origin_ms: 0,
        },
        12_000,
        19_000,
        digest,
        cancelled,
    ));
    tokio::time::sleep(Duration::from_millis(100)).await;
    if task.is_finished() {
        match task.await.expect("reader task") {
            Ok(_) => panic!("reader completed before cancellation"),
            Err(error) => panic!("reader failed before cancellation: {error}"),
        }
    }
    sender.send(true).expect("pending cancellation receiver");
    assert!(matches!(
        task.await.expect("reader task"),
        Err(SubtitleSourceError::Cancelled)
    ));
    println!("indexed cancellation PASS");
}
