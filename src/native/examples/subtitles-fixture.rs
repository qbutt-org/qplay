//! Generated integration scenario. No user media or account data is read.

use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use qplay_native::subtitles::{
    MpvEmbeddedTrack, SubtitleCache, SubtitleDocument, SubtitleFormat, SubtitleIdentity,
    SubtitleService, SubtitleServiceError, SubtitleSourceError, SubtitleTranslationOptions,
    VerifiedMediaTools, digest_local_media, extract_local_embedded, pending_batch,
    read_local_external, verify_embedded_track,
};
use tokio::sync::watch;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
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
        let generation = service.begin_generation()?;
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
        let new_generation = service.begin_generation()?;
        assert!(matches!(
            service.cached_window(&document, 1_000, &options, generation),
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
    let generation = service.begin_generation()?;
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
    if std::env::var_os("QPLAY_SUBTITLE_LIVE_CODEX").is_some() {
        live_codex_fixture().await?;
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
    let (model, effort) =
        qbutt_codex::resolve_selection(qbutt_codex::ModelModality::Text, None, None).await?;
    let document = SubtitleDocument::parse(
        "1\n00:00:01,000 --> 00:00:03,000\nHello, Alice.\n",
        SubtitleFormat::Srt,
    )?;
    let batch = pending_batch(&document, 1_000, &HashMap::new())?;
    let options = SubtitleTranslationOptions {
        identity: SubtitleIdentity {
            media_digest: "c".repeat(64),
            stream_index: None,
            source_digest: "d".repeat(64),
        },
        target_language: "ru".into(),
        model,
        effort,
    };
    let (_sender, cancelled) = watch::channel(false);
    let translated = qplay_native::subtitles::translate_batch(&batch, &options, cancelled).await?;
    let accepted: HashMap<_, _> = translated
        .accepted
        .into_iter()
        .map(|(key, text)| (key.fragment_id, text))
        .collect();
    assert_ne!(document.render(&accepted)?, document.original());
    println!("subtitle-codex PASS: one generated cue translated");
    Ok(())
}
