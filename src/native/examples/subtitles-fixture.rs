//! Generated integration scenario. No user media or account data is read.

use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use qplay_native::subtitles::{
    MpvEmbeddedTrack, PlaybackMedia, SubtitleCache, SubtitleDocument, SubtitleFormat,
    SubtitleIdentity, SubtitleService, SubtitleServiceError, SubtitleSourceError,
    SubtitleTranslationOptions, VerifiedMediaTools, digest_local_media, extract_local_embedded,
    pending_batch, read_local_external, verify_embedded_track,
};
use tokio::sync::watch;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    if std::env::var_os("QPLAY_SUBTITLE_LIVE_CODEX").is_some() {
        return live_codex_fixture().await;
    }
    if let Ok(evidence) = std::env::var("QPLAY_SUBTITLE_CACHE_EVIDENCE") {
        return cached_codex_fixture(Path::new(&evidence)).await;
    }

    let header = "[Script Info]\r\nScriptType: v4.00+\r\n\r\n[Events]\r\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\r\n";
    let cue = "Dialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,{\\an8}Hello\\N{\\i1}world{\\i0}!";
    let later = "Dialogue: 0,0:01:01.00,0:01:03.00,Default,,0,0,0,,See you soon.";
    let whole = format!("{header}{cue}\r\n{later}\r\n");
    let document = SubtitleDocument::parse(&whole, SubtitleFormat::Ass)?;
    assert_eq!(document.fragments().len(), 4);
    assert_eq!(document.render(&HashMap::new())?, whole);
    let first = &document.fragments()[0];
    let translated = HashMap::from([(first.id.clone(), "Привет".into())]);
    assert_eq!(
        document.render(&translated)?,
        whole.replacen("Hello\\N", "Привет\\N", 1)
    );
    let changed_header = SubtitleDocument::parse(
        &format!("{header}Title: another window\r\n{cue}\r\n"),
        SubtitleFormat::Ass,
    )?;
    assert_eq!(first.id, changed_header.fragments()[0].id);
    let drawing = SubtitleDocument::parse(
        &format!(
            "{header}Dialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,A{{\\p1}}m 0 0 l 10 10{{\\p0}}B\r\n"
        ),
        SubtitleFormat::Ass,
    )?;
    assert_eq!(
        drawing
            .fragments()
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B"]
    );
    let srt = "1\r\n00:00:01,000 --> 00:00:03,000\r\n<i>Hello</i> world\r\n\r\n";
    let srt_document = SubtitleDocument::parse(srt, SubtitleFormat::Srt)?;
    let accepted = HashMap::from([(srt_document.fragments()[0].id.clone(), "Привет".into())]);
    assert_eq!(
        srt_document.render(&accepted)?,
        "1\r\n00:00:01,000 --> 00:00:03,000\r\n<i>Привет</i> world\r\n\r\n"
    );
    let vtt = "WEBVTT\n\nSTYLE\n::cue { color: white; }\n\n00:01.000 --> 00:03.000 line:10%\n<v Alice>Hello &amp; goodbye</v>\n";
    let vtt_document = SubtitleDocument::parse(vtt, SubtitleFormat::Vtt)?;
    assert_eq!(vtt_document.fragments()[0].text, "Hello & goodbye");
    let accepted = HashMap::from([(vtt_document.fragments()[0].id.clone(), "Аня <Борис>".into())]);
    assert!(
        vtt_document
            .render(&accepted)?
            .contains("<v Alice>Аня &lt;Борис&gt;</v>")
    );
    assert!(SubtitleDocument::parse("bad\0text", SubtitleFormat::Srt).is_err());
    assert!(
        SubtitleDocument::parse(&"x".repeat(8 * 1024 * 1024 + 1), SubtitleFormat::Srt).is_err()
    );
    assert!(
        SubtitleDocument::parse(
            "1\n00:00:03,000 --> 00:00:01,000\nwrong\n",
            SubtitleFormat::Srt
        )
        .is_err()
    );

    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let profile = std::env::temp_dir().join(format!("qplay-subtitles-{nonce}"));
    let result = (|| -> Result<()> {
        let identity = SubtitleIdentity {
            media_digest: "a".repeat(64),
            stream_index: Some(3),
            source_digest: "b".repeat(64),
        };
        let options = SubtitleTranslationOptions {
            identity,
            target_language: "ru".into(),
            model: "gpt-6.1-sol".into(),
            effort: "low".into(),
        };
        let mut cache = SubtitleCache::new(&profile)?;
        let key = options.key(first);
        cache.accept(&key, "Привет")?;
        assert_eq!(cache.read(&key)?, Some("Привет".into()));
        let other_model = qplay_native::subtitles::SubtitleCacheKey::new(
            key.identity.clone(),
            key.fragment_id.clone(),
            "ru".into(),
            "another-model".into(),
            "low".into(),
            0,
        );
        let other_target = qplay_native::subtitles::SubtitleCacheKey::new(
            key.identity.clone(),
            key.fragment_id.clone(),
            "en".into(),
            "gpt-6.1-sol".into(),
            "low".into(),
            0,
        );
        assert!(cache.read(&other_model)?.is_none());
        assert!(cache.read(&other_target)?.is_none());
        let service = SubtitleService::new(&profile)?;
        let generation = service.begin_generation();
        assert_eq!(
            service.cached_window(&changed_header, 1_000, &options, generation)?,
            HashMap::from([(first.id.clone(), "Привет".into())])
        );
        let output = service.output_path(generation, SubtitleFormat::Ass);
        service.publish_window(&changed_header, &translated, &output, generation)?;
        assert_eq!(
            std::fs::read_to_string(&output)?,
            changed_header.render(&translated)?
        );
        assert!(
            service
                .publish_window(&changed_header, &translated, &output, generation)
                .is_err()
        );
        let batch = pending_batch(&changed_header, 1_000, &translated)?;
        assert!(
            !batch
                .fragments
                .iter()
                .any(|fragment| fragment.id == first.id)
        );
        assert_eq!(
            pending_batch(&document, 61_000, &HashMap::new())?
                .fragments
                .len(),
            1
        );
        let new_generation = service.begin_generation();
        assert!(matches!(
            service.cached_window(&document, 1_000, &options, generation),
            Err(SubtitleServiceError::Stale)
        ));
        assert!(matches!(
            service.publish_window(&document, &translated, &output, generation),
            Err(SubtitleServiceError::Stale)
        ));
        assert!(new_generation > generation);
        let shard = std::fs::read_dir(profile.join("subtitle-cache/v1"))?
            .next()
            .ok_or("missing shard")??
            .path();
        let entry = std::fs::read_dir(shard)?
            .next()
            .ok_or("missing cue")??
            .path();
        std::fs::write(entry, b"\"\"")?;
        assert!(cache.read(&key).is_err());
        Ok(())
    })();
    std::fs::remove_dir_all(&profile)?;
    result?;

    let service = SubtitleService::new(&std::env::temp_dir())?;
    let generation = service.begin_generation();
    let options = SubtitleTranslationOptions {
        identity: SubtitleIdentity {
            media_digest: "c".repeat(64),
            stream_index: None,
            source_digest: "d".repeat(64),
        },
        target_language: "ru".into(),
        model: "gpt-6.1-sol".into(),
        effort: "low".into(),
    };
    let (_sender, cancelled) = watch::channel(true);
    assert!(matches!(
        service
            .translate_window(&srt_document, 1_000, &options, generation, cancelled)
            .await,
        Err(SubtitleServiceError::Codex(
            qbutt_codex::CodexError::Cancelled
        ))
    ));

    if let Ok(library) = std::env::var("QPLAY_SUBTITLE_FIXTURE_LIBRARY") {
        media_fixture(Path::new(&library)).await?;
    }
    println!("subtitle-fixture PASS: exact render, bounds, cache, seek, cancel, corruption");
    Ok(())
}

async fn media_fixture(library: &Path) -> Result<()> {
    let tools = VerifiedMediaTools::from_library(library)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let directory = std::env::temp_dir().join(format!("qplay-subtitle-media-{nonce}"));
    std::fs::create_dir(&directory)?;
    let result = async {
        let external = directory.join("original.srt");
        let media = directory.join("generated.mkv");
        std::fs::write(
            &external,
            "1\n00:00:01,000 --> 00:00:03,000\nHello from generated video\n\n",
        )?;
        let (document, digest) = read_local_external(&external, SubtitleFormat::Srt).await?;
        assert_eq!(document.fragments()[0].text, "Hello from generated video");
        assert_eq!(digest.len(), 64);
        let ffmpeg = library
            .parent()
            .ok_or("missing runtime directory")?
            .join("ffmpeg.exe");
        let status = tokio::process::Command::new(ffmpeg)
            .args([
                "-nostdin",
                "-hide_banner",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=16x16:r=1",
                "-i",
            ])
            .arg(&external)
            .args(["-t", "5", "-c:v", "mpeg4", "-c:s", "srt", "-y"])
            .arg(&media)
            .status()
            .await?;
        assert!(status.success());
        let (_sender, cancelled) = watch::channel(false);
        assert_eq!(digest_local_media(&media, cancelled).await?.len(), 64);
        let identity_file = directory.join("identity.bin");
        std::fs::write(&identity_file, b"first media generation")?;
        let playback = PlaybackMedia::new(identity_file.to_string_lossy().into_owned(), Vec::new(), None);
        let (_sender, cancelled) = watch::channel(true);
        assert!(matches!(
            playback.get(cancelled).await,
            Err(SubtitleSourceError::Cancelled)
        ));
        let (_sender, cancelled) = watch::channel(false);
        let (first_digest, shared_digest) = tokio::join!(
            playback.get(cancelled.clone()),
            playback.get(cancelled)
        );
        let first_digest = first_digest?;
        assert_eq!(shared_digest?, first_digest);
        std::fs::write(&identity_file, b"reopened media generation")?;
        let (_sender, cancelled) = watch::channel(false);
        assert_eq!(playback.get(cancelled).await?, first_digest);
        let reopened = PlaybackMedia::new(identity_file.to_string_lossy().into_owned(), Vec::new(), None);
        let (_sender, cancelled) = watch::channel(false);
        assert_ne!(reopened.get(cancelled).await?, first_digest);
        let selected = MpvEmbeddedTrack {
            mpv_id: 2,
            ff_index: Some(1),
            src_id: None,
            codec: "subrip".into(),
            current_demuxer: "lavf".into(),
            origin_ms: 0,
        };
        let (_sender, cancelled) = watch::channel(false);
        let track = verify_embedded_track(&media, &tools, &selected, cancelled).await?;
        let (_sender, cancelled) = watch::channel(false);
        let embedded = extract_local_embedded(&media, &tools, &track, 0, 4_000, cancelled).await?;
        assert_eq!(embedded.format, SubtitleFormat::Srt);
        assert_eq!(embedded.fragments()[0].text, "Hello from generated video");
        let (_sender, cancelled) = watch::channel(false);
        let overlap =
            extract_local_embedded(&media, &tools, &track, 1_000, 4_000, cancelled).await?;
        assert_eq!(embedded.fragments()[0].id, overlap.fragments()[0].id);
        let incorrect = MpvEmbeddedTrack {
            ff_index: Some(0),
            ..selected
        };
        let (_sender, cancelled) = watch::channel(false);
        assert!(matches!(
            verify_embedded_track(&media, &tools, &incorrect, cancelled).await,
            Err(SubtitleSourceError::TrackMapping)
        ));

        let second = directory.join("second.srt");
        let reordered = directory.join("reordered.mkv");
        std::fs::write(
            &second,
            "1\n00:00:01,000 --> 00:00:03,000\nAnother subtitle track\n\n",
        )?;
        let status = tokio::process::Command::new(library.parent().unwrap().join("ffmpeg.exe"))
            .args([
                "-nostdin",
                "-hide_banner",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=16x16:r=1",
                "-i",
            ])
            .arg(&external)
            .arg("-i")
            .arg(&second)
            .args([
                "-t",
                "5",
                "-map",
                "0:v",
                "-map",
                "2:s",
                "-map",
                "1:s",
                "-c:v",
                "mpeg4",
                "-c:s",
                "srt",
                "-metadata:s:s:0",
                "title=Same",
                "-metadata:s:s:1",
                "title=Same",
                "-metadata:s:s:0",
                "language=eng",
                "-metadata:s:s:1",
                "language=eng",
                "-disposition:s:0",
                "0",
                "-disposition:s:1",
                "default",
                "-y",
            ])
            .arg(&reordered)
            .status()
            .await?;
        assert!(status.success());
        let selected = MpvEmbeddedTrack {
            mpv_id: 1,
            ff_index: Some(2),
            src_id: Some(2),
            codec: "subrip".into(),
            current_demuxer: "mkv".into(),
            origin_ms: 0,
        };
        let (_sender, cancelled) = watch::channel(false);
        let track = verify_embedded_track(&reordered, &tools, &selected, cancelled)
            .await
            .map_err(|error| format!("Map MKV track 2: {error}"))?;
        let (_sender, cancelled) = watch::channel(false);
        let document = extract_local_embedded(&reordered, &tools, &track, 0, 4_000, cancelled)
            .await
            .map_err(|error| format!("Extract MKV track 2: {error}"))?;
        assert_eq!(document.fragments()[0].text, "Another subtitle track");
        let selected = MpvEmbeddedTrack {
            mpv_id: 2,
            src_id: Some(3),
            ..selected
        };
        let (_sender, cancelled) = watch::channel(false);
        let track = verify_embedded_track(&reordered, &tools, &selected, cancelled)
            .await
            .map_err(|error| format!("Map MKV track 3: {error}"))?;
        let (_sender, cancelled) = watch::channel(false);
        let document = extract_local_embedded(&reordered, &tools, &track, 0, 4_000, cancelled)
            .await
            .map_err(|error| format!("Extract MKV track 3: {error}"))?;
        assert_eq!(document.fragments()[0].text, "Hello from generated video");

        let identical = directory.join("identical.mkv");
        let status = tokio::process::Command::new(library.parent().unwrap().join("ffmpeg.exe"))
            .args([
                "-nostdin",
                "-hide_banner",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=16x16:r=1",
                "-i",
            ])
            .arg(&external)
            .args([
                "-t", "5", "-map", "0:v", "-map", "1:s", "-map", "1:s", "-c:v", "mpeg4", "-c:s",
                "srt", "-y",
            ])
            .arg(&identical)
            .status()
            .await?;
        assert!(status.success());
        let (_sender, cancelled) = watch::channel(false);
        assert!(matches!(
            verify_embedded_track(&identical, &tools, &selected, cancelled).await,
            Err(SubtitleSourceError::TrackMapping)
        ));

        let styled = directory.join("styled.ass");
        let styled_media = directory.join("styled.mkv");
        std::fs::write(&styled, "[Script Info]\nScriptType: v4.00+\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,{\\an8}Styled text\n")?;
        let status = tokio::process::Command::new(library.parent().unwrap().join("ffmpeg.exe"))
            .args(["-nostdin", "-hide_banner", "-v", "error", "-f", "lavfi", "-i", "color=c=blue:s=16x16:r=1", "-i"])
            .arg(&styled)
            .args(["-t", "5", "-map", "0:v", "-map", "1:s", "-c:v", "mpeg4", "-c:s", "ass", "-y"])
            .arg(&styled_media)
            .status()
            .await?;
        assert!(status.success());
        let selected = MpvEmbeddedTrack { mpv_id: 1, ff_index: Some(1), src_id: Some(2), codec: "ass".into(), current_demuxer: "mkv".into(), origin_ms: 0 };
        let (_sender, cancelled) = watch::channel(false);
        let track = verify_embedded_track(&styled_media, &tools, &selected, cancelled).await?;
        let (_sender, cancelled) = watch::channel(false);
        let document = extract_local_embedded(&styled_media, &tools, &track, 0, 4_000, cancelled).await?;
        assert_eq!(document.format, SubtitleFormat::Ass);
        assert!(document.original().contains("{\\an8}Styled text"));
        let bitmap = MpvEmbeddedTrack { codec: "hdmv_pgs_subtitle".into(), ..selected };
        let (_sender, cancelled) = watch::channel(false);
        assert!(matches!(verify_embedded_track(&styled_media, &tools, &bitmap, cancelled).await, Err(SubtitleSourceError::Bitmap)));
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    std::fs::remove_dir_all(directory)?;
    result?;
    println!("subtitle-media PASS: generated local media and verified qplay tools");
    Ok(())
}

async fn live_codex_fixture() -> Result<()> {
    let media = std::path::PathBuf::from(std::env::var("QPLAY_SUBTITLE_LIVE_MEDIA")?);
    let evidence = std::path::PathBuf::from(std::env::var("QPLAY_SUBTITLE_LIVE_EVIDENCE")?);
    let data = evidence.join("profile/data");
    std::fs::create_dir_all(&evidence)?;
    let source = evidence.join("original.ass");
    std::fs::write(
        &source,
        r"[Script Info]
ScriptType: v4.00+
PlayResX: 1280
PlayResY: 720

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,36,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,2,1,2,30,30,36,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:01.00,0:00:20.00,Default,Alice,0,0,0,,{\an8}Hello, Alice.\N{\i1}Bob is waiting at the gate.{\i0}
Dialogue: 0,0:01:01.00,0:01:15.00,Default,Bob,0,0,0,,Alice, please keep Bob safe.
",
    )?;
    let (document, source_digest) = read_local_external(&source, SubtitleFormat::Ass).await?;
    let (_sender, cancelled) = watch::channel(false);
    let media_digest = digest_local_media(&media, cancelled.clone()).await?;
    let (model, effort) =
        qbutt_codex::resolve_selection(qbutt_codex::ModelModality::Text, None, None).await?;
    let options = SubtitleTranslationOptions {
        identity: SubtitleIdentity {
            media_digest,
            stream_index: None,
            source_digest,
        },
        target_language: "ru".into(),
        model,
        effort,
    };
    let service = SubtitleService::new(&data)?;
    let generation = service.begin_generation();
    assert!(
        service
            .cached_window(&document, 2_000, &options, generation)?
            .is_empty()
    );
    let translated = service
        .translate_window(&document, 2_000, &options, generation, cancelled)
        .await?
        .ok_or("The fresh fixture did not run inference")?;
    service.remember_selection(&translated.accepted[0].0, None, None, generation)?;
    let accepted = service.cached_window(&document, 2_000, &options, generation)?;
    assert_eq!(accepted.len(), document.fragments().len());
    let output = service.output_path(generation, SubtitleFormat::Ass);
    service.publish_window(&document, &accepted, &output, generation)?;
    let rendered = std::fs::read_to_string(&output)?;
    let rendered_document = SubtitleDocument::parse(&rendered, SubtitleFormat::Ass)?;
    assert_eq!(
        rendered_document.fragments().len(),
        document.fragments().len()
    );

    let mut original_envelope = HashMap::new();
    let mut translated_envelope = HashMap::new();
    for (index, (original, translated)) in document
        .fragments()
        .iter()
        .zip(rendered_document.fragments())
        .enumerate()
    {
        assert_eq!(
            (original.start_ms, original.end_ms),
            (translated.start_ms, translated.end_ms)
        );
        assert!(
            translated
                .text
                .chars()
                .any(|character| ('А'..='я').contains(&character))
        );
        let marker = format!("fragment-{index}");
        original_envelope.insert(original.id.clone(), marker.clone());
        translated_envelope.insert(translated.id.clone(), marker);
    }
    assert_eq!(
        document.render(&original_envelope)?,
        rendered_document.render(&translated_envelope)?
    );
    assert!(
        accepted
            .values()
            .any(|text| text.contains("Боб") || text.contains("Bob"))
    );
    assert!(
        accepted
            .values()
            .any(|text| text.contains("Алис") || text.contains("Элис") || text.contains("Alice"))
    );
    std::fs::write(evidence.join("translated.ass"), &rendered)?;
    drop(service);

    let reopened = SubtitleService::new(&data)?;
    let generation = reopened.begin_generation();
    let (_sender, cancelled) = watch::channel(true);
    for position_ms in [2_000, 62_000] {
        let cached = reopened.cached_window(&document, position_ms, &options, generation)?;
        assert!(!cached.is_empty());
        assert!(
            pending_batch(&document, position_ms, &cached)?
                .fragments
                .is_empty()
        );
        assert!(
            reopened
                .translate_window(
                    &document,
                    position_ms,
                    &options,
                    generation,
                    cancelled.clone()
                )
                .await?
                .is_none()
        );
        for (id, text) in cached {
            assert_eq!(accepted.get(&id), Some(&text));
        }
    }
    let receipt = serde_json::json!({
        "media": media,
        "mediaSha256": options.identity.media_digest,
        "originalSha256": options.identity.source_digest,
        "translatedSha256": format!("{:x}", Sha256::digest(rendered.as_bytes())),
        "model": options.model,
        "effort": options.effort,
        "targetLanguage": options.target_language,
        "accepted": translated.accepted.iter().map(|(key, text)| serde_json::json!({
            "text": text,
            "key": key,
        })).collect::<Vec<_>>(),
        "exactAssEnvelopeAndTiming": true,
        "cacheAfterReopenAndSeek": true,
        "noInferenceOnCacheHit": true,
    });
    std::fs::write(
        evidence.join("live-subtitles.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    println!(
        "subtitle-codex PASS: generated ASS translated to Russian; exact style/timing, names and reopen/seek cache"
    );
    Ok(())
}

async fn cached_codex_fixture(evidence: &Path) -> Result<()> {
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(evidence.join("live-subtitles.json"))?)?;
    let first = &receipt["accepted"][0];
    let key: qplay_native::subtitles::SubtitleCacheKey =
        serde_json::from_value(first["key"].clone())?;
    let first_text = first["text"].as_str().ok_or("Missing accepted text")?;
    let (document, source_digest) =
        read_local_external(&evidence.join("original.ass"), SubtitleFormat::Ass).await?;
    assert_eq!(source_digest, key.identity.source_digest);
    let service = SubtitleService::new(&evidence.join("profile/data"))?;
    let generation = service.begin_generation();
    service.remember_selection(&key, None, None, generation)?;
    let options = service
        .cached_options(&key.identity, "ru", None, None, generation)?
        .ok_or("Accepted automatic choice was not restored")?;
    assert_eq!((&options.model, &options.effort), (&key.model, &key.effort));
    assert!(
        service
            .cached_options(&key.identity, "en", None, None, generation)?
            .is_none()
    );
    let mut other_media = key.identity.clone();
    other_media.media_digest = "f".repeat(64);
    assert!(
        service
            .cached_options(&other_media, "ru", None, None, generation)?
            .is_none()
    );
    assert!(
        service
            .cached_options(&key.identity, "ru", Some("other-model"), None, generation)?
            .is_none()
    );
    let other_options = service
        .cached_options(
            &key.identity,
            "ru",
            Some("other-model"),
            Some("low"),
            generation,
        )?
        .ok_or("Explicit choices must support local lookup")?;
    assert!(
        service
            .cached_window(&document, 2_000, &other_options, generation)?
            .is_empty()
    );
    let (_sender, cancelled) = watch::channel(true);
    for position_ms in [2_000, 62_000] {
        assert!(
            service
                .translate_window(
                    &document,
                    position_ms,
                    &options,
                    generation,
                    cancelled.clone()
                )
                .await?
                .is_none()
        );
    }
    let accepted = service.cached_window(&document, 2_000, &options, generation)?;
    assert_eq!(
        document.render(&accepted)?,
        std::fs::read_to_string(evidence.join("translated.ass"))?
    );
    service.begin_generation();
    assert!(matches!(
        service.cached_options(&key.identity, "ru", None, None, generation),
        Err(SubtitleServiceError::Stale)
    ));
    assert!(matches!(
        service.remember_selection(&key, None, None, generation),
        Err(SubtitleServiceError::Stale)
    ));
    drop(service);

    let partial_data = evidence.join("partial-profile/data");
    let mut cache = SubtitleCache::new(&partial_data)?;
    cache.accept(&key, first_text)?;
    let service = SubtitleService::new(&partial_data)?;
    let generation = service.begin_generation();
    assert!(
        service
            .cached_options(&key.identity, "ru", None, None, generation)?
            .is_none()
    );
    service.remember_selection(&key, None, None, generation)?;
    let options = service
        .cached_options(&key.identity, "ru", None, None, generation)?
        .ok_or("Partial automatic choice was not restored")?;
    let accepted = service.cached_window(&document, 2_000, &options, generation)?;
    assert_eq!(accepted.len(), 1);
    assert_eq!(
        pending_batch(&document, 2_000, &accepted)?.fragments.len(),
        2
    );
    let (_sender, cancelled) = watch::channel(false);
    assert!(matches!(
        service
            .translate_window(&document, 2_000, &options, generation, cancelled)
            .await,
        Err(SubtitleServiceError::Codex(
            qbutt_codex::CodexError::Unavailable
        ))
    ));
    assert_eq!(
        service.cached_window(&document, 2_000, &options, generation)?,
        accepted
    );
    let output = service.output_path(generation, SubtitleFormat::Ass);
    service.publish_window(&document, &accepted, &output, generation)?;
    let rendered = std::fs::read_to_string(output)?;
    assert!(rendered.contains(first_text) && rendered.contains("Bob is waiting at the gate."));
    std::fs::write(evidence.join("partial-translated.ass"), rendered)?;
    std::fs::write(
        evidence.join("offline-cache.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "automaticChoice": options.model,
            "effort": options.effort,
            "exactIdentityAndTarget": true,
            "explicitChoiceLocalLookup": true,
            "cacheOnlyReopenAndSeek": true,
            "staleSelectionRejected": true,
            "partialCacheRetainedAfterUnavailableCodex": true,
            "partialAcceptedFragments": accepted.len(),
            "partialOriginalFragments": 2,
        }))?,
    )?;
    println!(
        "subtitle-cache PASS: offline Auto/explicit, identity, reopen/seek, stale and partial cache"
    );
    Ok(())
}
