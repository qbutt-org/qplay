//! Generated HTTP Matroska integration, run by tests/native/subtitles-matroska.ts.

use std::path::Path;
use std::sync::Arc;

use qplay_native::subtitles::{
    MpvEmbeddedTrack, PlaybackMedia, SubtitleCache, SubtitleCacheKey, extract_indexed_matroska,
};
use tokio::sync::watch;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 4 && args.len() != 6 && args.len() != 7 {
        eprintln!(
            "Usage: subtitles-matroska-fixture <URL> <TrackNumber> <Expected text> <ass|ssa|subrip|webvtt> [start-ms end-ms [profile]]"
        );
        std::process::exit(2);
    }
    let track = args[1].parse::<i64>().expect("TrackNumber");
    let media = Arc::new(PlaybackMedia::new(
        args[0].clone(),
        vec![
            "Authorization: fixture-authorization".into(),
            "Cookie: fixture-cookie".into(),
        ],
        None,
    ));
    let (_cancel_sender, cancelled) = watch::channel(false);
    let digest = media.get(cancelled.clone()).await.expect("media identity");
    let selected = MpvEmbeddedTrack {
        mpv_id: 1,
        ff_index: None,
        src_id: Some(track),
        codec: args[3].clone(),
        current_demuxer: "mkv".into(),
        origin_ms: 0,
    };
    let start = args
        .get(4)
        .map_or(12_000, |value| value.parse().expect("start"));
    let end = args
        .get(5)
        .map_or(19_000, |value| value.parse().expect("end"));
    let (document, identity) =
        extract_indexed_matroska(media, selected, start, end, digest, cancelled)
            .await
            .expect("indexed subtitle extraction");
    assert!(document.original().contains(&args[2]));
    match args[3].as_str() {
        "ass" => assert!(document.original().contains("Style: Default,Arial,20")),
        "ssa" => {
            assert!(document.original().contains("Style: Default,Arial,20"));
            assert!(document.original().contains("Format: Marked, Start, End"));
        }
        "webvtt" => {
            assert!(
                document
                    .original()
                    .contains("::cue(.bright) { color: yellow; }")
            );
            assert!(
                document
                    .original()
                    .contains("line:20% position:40% align:start")
            );
            assert!(document.original().contains("\nfirst\n"));
            assert!(document.original().contains("<00:00:02.000>after"));
            assert!(!document.original().contains("<00:00:01.000>after"));
        }
        "subrip" => {}
        _ => panic!("unsupported fixture codec"),
    }
    assert_eq!(
        document.fragments().len(),
        if args[3] == "webvtt" { 3 } else { 2 }
    );
    if let Some(profile) = args.get(6) {
        let models = qbutt_codex::available_models(qbutt_codex::ModelModality::Text)
            .await
            .expect("native model catalog");
        let model = models.first().expect("nonempty native model catalog");
        let effort = model
            .efforts
            .first()
            .expect("nonempty native effort catalog");
        let mut cache = SubtitleCache::new(Path::new(profile)).expect("native subtitle cache");
        for fragment in document.fragments() {
            let key = SubtitleCacheKey::new(
                identity.clone(),
                fragment.id.clone(),
                "ru".into(),
                model.id.clone(),
                effort.clone(),
                0,
            );
            cache
                .accept(&key, "Cached embedded translation")
                .expect("accepted embedded translation");
        }
    }
    println!(
        "{}",
        serde_json::json!({
            "status": "indexed-pass",
            "fragments": document.fragments().len(),
            "mediaDigest": identity.media_digest,
            "originalBytes": document.original().len(),
        })
    );
}
