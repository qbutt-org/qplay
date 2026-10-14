//! Generated HTTP Matroska integration, run by tests/native/subtitles-matroska.ts.

use std::path::Path;
use std::sync::Arc;

use qplay_native::subtitles::{
    MpvEmbeddedTrack, PlaybackMedia, SubtitleCache, SubtitleCacheKey, extract_network_matroska,
};
use tokio::sync::watch;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 4 && args.len() != 6 && args.len() != 7 && args.len() != 8 {
        eprintln!(
            "Usage: subtitles-matroska-fixture <URL> <TrackNumber> <Expected text> <ass|ssa|subrip|webvtt> [start-ms end-ms [profile [seed|offline]]]"
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
    let result = extract_network_matroska(media, selected, start, end, digest, cancelled).await;
    if args[2] == "--reject" {
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("malformed or changed source was accepted"),
        };
        println!(
            "{}",
            serde_json::json!({ "status": "rejected", "error": error.to_string() })
        );
        return;
    }
    let (document, identity) = result.expect("network subtitle extraction");
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
    if let Some(mode) = args.get(7) {
        use qplay_native::subtitles::{
            SubtitleService, SubtitleServiceError, SubtitleTranslationOptions,
        };
        let profile = Path::new(&args[6]);
        let options = SubtitleTranslationOptions {
            identity: identity.clone(),
            target_language: "en".into(),
            model: "fixture-model".into(),
            effort: "low".into(),
        };
        if mode == "seed" {
            let mut cache = SubtitleCache::new(profile).expect("fixture cache");
            for fragment in document.fragments() {
                cache
                    .accept(&options.key(fragment), "Accepted offline English")
                    .expect("cache seed");
            }
        } else {
            assert_eq!(mode, "offline");
        }
        let service = SubtitleService::new(profile).expect("single service owner");
        let generation = service.begin_generation();
        let options = service
            .cached_options(
                &identity,
                "en",
                Some("fixture-model"),
                Some("low"),
                generation,
            )
            .expect("offline options")
            .expect("explicit cache selection");
        let accepted = service
            .cached_window(&document, start, &options, generation)
            .expect("offline window");
        assert_eq!(accepted.len(), document.fragments().len());
        let output = service.output_path(generation, document.format);
        service
            .publish_window(&document, &accepted, &output, generation)
            .expect("offline publish");
        let rendered = std::fs::read_to_string(output).expect("published text");
        let translated =
            qplay_native::subtitles::SubtitleDocument::parse(&rendered, document.format)
                .expect("translated envelope");
        for (original, translated) in document.fragments().iter().zip(translated.fragments()) {
            assert_eq!(
                (original.start_ms, original.end_ms),
                (translated.start_ms, translated.end_ms)
            );
        }
        assert!(rendered.contains("Accepted offline English"));
        assert!(rendered.contains("{\\an8}"));
        assert!(rendered.contains("Style: Default,Arial,20"));
        assert_eq!(
            document
                .render(&std::collections::HashMap::new())
                .expect("Original"),
            document.original()
        );
        service.begin_generation();
        let stale = service.output_path(generation, document.format);
        assert!(matches!(
            service.publish_window(&document, &accepted, &stale, generation),
            Err(SubtitleServiceError::Stale)
        ));
    } else if let Some(profile) = args.get(6) {
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
            "status": "network-pass",
            "sourceDigest": identity.source_digest,
            "fragments": document.fragments().len(),
            "mediaDigest": identity.media_digest,
            "originalBytes": document.original().len(),
        })
    );
}
