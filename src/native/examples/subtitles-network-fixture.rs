//! Generated HTTP acquisition scenario, served by the native fixture runner.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use qplay_native::subtitles::{
    PlaybackMedia, SubtitleCache, SubtitleCacheKey, SubtitleFormat, SubtitleIdentity,
    SubtitleSourceError,
};
use tokio::sync::watch;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().collect();
    let base = arguments.get(1).ok_or("missing fixture origin")?;
    let foreign = arguments.get(2).ok_or("missing second fixture origin")?;
    let media = format!("{base}/movie.mkv");
    let headers = vec![
        "Authorization: fixture-authorization".into(),
        "Cookie: fixture-cookie".into(),
        "User-Agent: qplay-subtitle-fixture".into(),
    ];
    let playback = PlaybackMedia::new(media.clone(), headers.clone(), None);

    for (path, format) in [
        ("original.ass", SubtitleFormat::Ass),
        ("original.srt?fixture=signed", SubtitleFormat::Srt),
        ("redirect.ass", SubtitleFormat::Ass),
    ] {
        let (_sender, cancelled) = watch::channel(false);
        let (document, digest) = playback
            .read_external(&format!("{base}/{path}"), format, cancelled)
            .await?;
        assert_eq!(digest.len(), 64);
        assert_eq!(document.render(&HashMap::new())?, document.original());
        assert_eq!(document.fragments()[0].text, "Hello from the network");
        if format == SubtitleFormat::Ass {
            assert!(document.original().contains("Style: Default,Arial,20"));
            assert!(document.original().contains("{\\an8}"));
        }
    }

    let (_sender, cancelled) = watch::channel(false);
    let (document, _) = playback
        .read_external(
            &format!("{base}/foreign.srt"),
            SubtitleFormat::Srt,
            cancelled,
        )
        .await?;
    assert_eq!(document.fragments()[0].text, "Hello from the network");

    let (_sender, cancelled) = watch::channel(false);
    playback
        .read_external(
            &format!("{foreign}/original.srt"),
            SubtitleFormat::Srt,
            cancelled,
        )
        .await?;

    let proxy = PlaybackMedia::new(
        "http://subtitle-fixture.invalid/movie.mkv".into(),
        headers.clone(),
        Some(base.clone()),
    );
    let (_sender, cancelled) = watch::channel(false);
    let (document, _) = proxy
        .read_external(
            "http://subtitle-fixture.invalid/proxy.srt",
            SubtitleFormat::Srt,
            cancelled,
        )
        .await?;
    assert_eq!(document.fragments()[0].text, "Hello from the network");

    for path in ["oversize.srt", "streamed-oversize.srt"] {
        let (_sender, cancelled) = watch::channel(false);
        assert!(matches!(
            playback
                .read_external(&format!("{base}/{path}"), SubtitleFormat::Srt, cancelled,)
                .await,
            Err(SubtitleSourceError::TooLarge)
        ));
    }

    let (_sender, cancelled) = watch::channel(false);
    let invalid = PlaybackMedia::new(
        media.clone(),
        vec!["Authorization: bad\r\nInjected: header".into()],
        None,
    );
    assert!(
        invalid
            .read_external(
                &format!("{base}/header.srt"),
                SubtitleFormat::Srt,
                cancelled,
            )
            .await
            .is_err()
    );

    let (cancel, cancelled) = watch::channel(false);
    let started = Instant::now();
    let slow_source = format!("{base}/slow.srt");
    let (result, ()) = tokio::join!(
        playback.read_external(&slow_source, SubtitleFormat::Srt, cancelled,),
        async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel.send(true).expect("acquisition receiver disappeared");
        }
    );
    assert!(matches!(result, Err(SubtitleSourceError::Cancelled)));
    assert!(started.elapsed() < Duration::from_secs(2));

    let (_sender, cancelled) = watch::channel(false);
    let first_digest = playback.get(cancelled.clone()).await?;
    assert_eq!(playback.get(cancelled.clone()).await?, first_digest);
    let reopened = PlaybackMedia::new(media.clone(), headers.clone(), None);
    assert_ne!(reopened.get(cancelled.clone()).await?, first_digest);
    let torrent = |token: &str, name: &str| {
        PlaybackMedia::new(
            format!("{base}/media/{token}/{name}"),
            headers.clone(),
            None,
        )
    };
    let first = torrent("0123456789abcdef0123456789abcdef", "same.mkv");
    let (first_digest, shared_digest) =
        tokio::join!(first.get(cancelled.clone()), first.get(cancelled.clone()));
    let first_digest = first_digest?;
    assert_eq!(shared_digest?, first_digest);
    let reopened = torrent("fedcba9876543210fedcba9876543210", "same.mkv");
    assert_eq!(reopened.get(cancelled.clone()).await?, first_digest);
    let changed = torrent("0123456789abcdef0123456789abcdef", "changed.mkv");
    assert_ne!(changed.get(cancelled.clone()).await?, first_digest);
    let resized = torrent("0123456789abcdef0123456789abcdef", "resized.mkv");
    assert_ne!(resized.get(cancelled.clone()).await?, first_digest);
    let weak = torrent("0123456789abcdef0123456789abcdef", "weak.mkv");
    let weak_digest = weak.get(cancelled.clone()).await?;
    let weak = torrent("0123456789abcdef0123456789abcdef", "weak.mkv");
    assert_ne!(weak.get(cancelled.clone()).await?, weak_digest);

    let (document, source_digest) = first
        .read_external(
            &format!("{base}/original.ass"),
            SubtitleFormat::Ass,
            cancelled.clone(),
        )
        .await?;
    let profile =
        std::env::temp_dir().join(format!("qplay-http-subtitle-cache-{}", std::process::id()));
    let result = (|| -> Result<()> {
        let key = SubtitleCacheKey::new(
            SubtitleIdentity {
                media_digest: first_digest,
                stream_index: None,
                source_digest,
            },
            document.fragments()[0].id.clone(),
            "ru".into(),
            "fixture-model".into(),
            "fixture-effort".into(),
            0,
        );
        let mut cache = SubtitleCache::new(&profile)?;
        cache.accept(&key, "Перевод из cache")?;
        let cache = SubtitleCache::new(&profile)?;
        assert_eq!(cache.read(&key)?, Some("Перевод из cache".into()));
        assert!(
            document
                .original()
                .contains("{\\an8}Hello from the network")
        );
        Ok(())
    })();
    std::fs::remove_dir_all(profile)?;
    result?;
    if let Some(profile) = arguments.get(3) {
        let native = torrent("0123456789abcdef0123456789abcdef", "native.mp4");
        let (_sender, cancelled) = watch::channel(false);
        let media_digest = native.get(cancelled.clone()).await?;
        let (document, source_digest) = native
            .read_external(
                &format!("{base}/native.ass?fixture=signed"),
                SubtitleFormat::Ass,
                cancelled,
            )
            .await?;
        let models = qbutt_codex::available_models(qbutt_codex::ModelModality::Text).await?;
        let model = models.first().ok_or("empty native fixture model catalog")?;
        let effort = model
            .efforts
            .first()
            .ok_or("empty native fixture effort catalog")?;
        let key = SubtitleCacheKey::new(
            SubtitleIdentity {
                media_digest,
                stream_index: None,
                source_digest,
            },
            document.fragments()[0].id.clone(),
            "ru".into(),
            model.id.clone(),
            effort.clone(),
            0,
        );
        let mut cache = SubtitleCache::new(Path::new(profile))?;
        cache.accept(&key, "Cached network translation")?;
    }
    println!(
        "subtitle-network PASS: exact ASS/SRT, scoped auth and redirects, bounds, cancellation, session/ETag identity and cache"
    );
    Ok(())
}
