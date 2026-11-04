//! Real FFprobe integration, driven by tests/native/content-probe.ts.

#[allow(
    dead_code,
    reason = "Reuse the production classifier without exposing content-choice internals"
)]
#[path = "../src/gpu/content.rs"]
mod content;
#[path = "../src/gpu/probe.rs"]
mod probe;
#[path = "../src/gpu/process.rs"]
mod process;
#[path = "../src/gpu/timing.rs"]
mod timing;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use qplay_native::framegen;
use serde_json::json;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    assert!(
        args.len() == 4 || args.len() == 5,
        "content-probe <ffprobe> <source> <duration> <file|network|auth|cookies|cancel|resolve|failure> [selection.json]"
    );
    let ffprobe = Path::new(&args[0]);
    let duration = args[2].parse::<f64>().expect("duration");
    let mode = args[3].as_str();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancellation = (mode == "cancel").then(|| {
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            let cancelled_at = Instant::now();
            cancel.store(true, Ordering::Release);
            cancelled_at
        })
    });
    let headers = if mode == "auth" {
        vec![
            "Authorization: fixture-private-authorization".into(),
            "Cookie: fixture-private-cookie".into(),
        ]
    } else {
        Vec::new()
    };
    let selected = if mode == "resolve" {
        let selection: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&args[4]).expect("selection file"))
                .expect("selection JSON");
        Some(
            timing::resolve_network_source(
                selection["streamFilename"]
                    .as_str()
                    .expect("stream filename"),
                selection["trackTitle"].as_str().expect("track title"),
                0,
                Some(&selection["extractorResult"].to_string()),
            )
            .expect("selected video URL"),
        )
    } else {
        None
    };
    let ca_file = std::env::var_os("QPLAY_TEST_CA_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            ffprobe
                .parent()
                .expect("bundled engine")
                .join("profile/certs/cacert.pem")
        });
    let source = timing::NetworkSource {
        url: selected
            .as_ref()
            .map_or(args[1].as_str(), |source| source.url.as_str()),
        video_stream: selected
            .as_ref()
            .map_or("0", |source| source.video_stream.as_str()),
        ca_file: &ca_file,
        headers: &headers,
        proxy: None,
        user_agent: Some("qplay-style-fixture: 'quoted', [value]"),
        referrer: None,
        request_size: Some(4 * 1024 * 1024),
        cookies_in_use: mode == "cookies",
    };
    let started = Instant::now();
    let classification = if mode == "file" {
        probe::classify_file(ffprobe, Path::new(&args[1]), duration, 0, &cancel)
    } else {
        probe::classify_network(ffprobe, source, duration, &cancel)
    };
    if let Some(cancellation) = cancellation {
        let cancelled_at = cancellation.join().expect("cancel timer");
        assert!(
            classification.is_err(),
            "Cancelled style analysis succeeded"
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "Cancellation did not drain promptly"
        );
        println!(
            "{}",
            json!({"status":"cancelled","elapsedMs":started.elapsed().as_millis(),"drainMs":cancelled_at.elapsed().as_millis()})
        );
        return;
    }
    let timing = if mode == "file" {
        timing::probe_local_timing(ffprobe, Path::new(&args[1]), duration, 0, &cancel)
    } else {
        timing::probe_network_timing(ffprobe, source, &cancel)
    };
    if matches!(mode, "auth" | "cookies") {
        assert!(
            classification
                .expect_err("Protected style probe succeeded")
                .contains("protected stream")
        );
        assert!(
            timing
                .expect_err("Protected timing probe succeeded")
                .contains("protected stream")
        );
        println!(
            "{}",
            json!({"status":"protected","elapsedMs":started.elapsed().as_millis()})
        );
        return;
    }
    if mode == "failure" {
        let errors = [
            classification.expect_err("Invalid style source succeeded"),
            timing.expect_err("Invalid timing source succeeded"),
        ];
        for error in &errors {
            assert!(
                !error.contains("fixture-private-query"),
                "Probe error exposed private URL query"
            );
        }
        println!("{}", json!({"status":"failed","errors":errors}));
        return;
    }
    let classification = classification.expect("content classification");
    let timing = timing.expect("source timing");
    println!(
        "{}",
        json!({"status":"classified","kind":format!("{:?}",classification.kind),
        "confidence":classification.confidence,"fps":timing.source_rate.fps(),"variable":timing.variable,
        "elapsedMs":started.elapsed().as_millis()})
    );
}
