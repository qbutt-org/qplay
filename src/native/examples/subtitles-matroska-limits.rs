//! Generated file integration for bounded Matroska acquisition allocations.

use std::fs::File;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use matroska_demuxer::{DemuxError, Frame, MatroskaFile};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> Result<()> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let directory = std::env::temp_dir().join(format!("qplay-matroska-limits-{nonce}"));
    std::fs::create_dir(&directory)?;
    let result = (|| -> Result<()> {
        let path = directory.join("metadata.mkv");
        let mut file = File::create(&path)?;
        file.write_all(&[0x1a, 0x45, 0xdf, 0xa3])?;
        file.write_all(&size(8 * 1024 * 1024 + 1))?;
        let length = file.metadata()?.len() + 8 * 1024 * 1024 + 1;
        file.set_len(length)?;
        drop(file);
        assert!(matches!(
            MatroskaFile::open(File::open(&path)?),
            Err(DemuxError::ResourceLimitExceeded {
                limit: 8_388_608,
                ..
            })
        ));

        let path = directory.join("tracks.mkv");
        let (prefix, length) = generated_container(513, 4);
        let mut file = File::create(&path)?;
        file.write_all(&prefix)?;
        file.set_len(length)?;
        drop(file);
        let error = MatroskaFile::open(File::open(&path)?)
            .err()
            .ok_or("oversized track collection was accepted")?;
        assert!(
            matches!(error, DemuxError::ResourceLimitExceeded { limit: 512, .. }),
            "{error:?}"
        );

        let path = directory.join("frame.mkv");
        let (prefix, length) = generated_container(1, 32 * 1024 * 1024 + 5);
        let mut file = File::create(&path)?;
        file.write_all(&prefix)?;
        file.set_len(length)?;
        drop(file);
        let mut matroska = MatroskaFile::open(File::open(&path)?)?;
        let mut frame = Frame::default();
        assert!(matches!(
            matroska.next_frame(&mut frame),
            Err(DemuxError::ResourceLimitExceeded {
                limit: 33_554_432,
                ..
            })
        ));
        assert_eq!(frame.data.capacity(), 0);

        Ok(())
    })();
    std::fs::remove_dir_all(directory)?;
    result?;
    println!(
        "subtitle-matroska-limits PASS: metadata, track collections and frame pre-allocation bounds"
    );
    Ok(())
}

fn generated_container(tracks: u64, frame_size: u64) -> (Vec<u8>, u64) {
    let header = element(
        &[0x1a, 0x45, 0xdf, 0xa3],
        &[
            element(&[0x42, 0x82], b"matroska"),
            element(&[0x42, 0x87], &[4]),
            element(&[0x42, 0x85], &[2]),
        ]
        .concat(),
    );
    let info = element(
        &[0x15, 0x49, 0xa9, 0x66],
        &[
            element(&[0x4d, 0x80], b"qplay-fixture"),
            element(&[0x57, 0x41], b"qplay-fixture"),
        ]
        .concat(),
    );
    let tracks = element(
        &[0x16, 0x54, 0xae, 0x6b],
        &(1..=tracks)
            .flat_map(|track| {
                element(
                    &[0xae],
                    &[
                        element(&[0xd7], &track.to_be_bytes()),
                        element(&[0x73, 0xc5], &track.to_be_bytes()),
                        element(&[0x83], &[17]),
                        element(&[0x86], b"S_TEXT/UTF8"),
                    ]
                    .concat(),
                )
            })
            .collect::<Vec<_>>(),
    );
    let timestamp = element(&[0xe7], &[0]);
    let block_header = [vec![0xa3], size(frame_size)].concat();
    let cluster_size = u64::try_from(timestamp.len() + block_header.len()).unwrap() + frame_size;
    let cluster_header = [vec![0x1f, 0x43, 0xb6, 0x75], size(cluster_size)].concat();
    let segment_size =
        u64::try_from(info.len() + tracks.len() + cluster_header.len()).unwrap() + cluster_size;
    let prefix = [
        header,
        vec![0x18, 0x53, 0x80, 0x67],
        size(segment_size),
        info,
        tracks,
        cluster_header,
        timestamp,
        block_header,
        vec![0x81, 0, 0, 0],
    ]
    .concat();
    let length = u64::try_from(prefix.len()).unwrap() + frame_size - 4;
    (prefix, length)
}

fn element(id: &[u8], data: &[u8]) -> Vec<u8> {
    [
        id,
        size(u64::try_from(data.len()).unwrap()).as_slice(),
        data,
    ]
    .concat()
}

fn size(value: u64) -> Vec<u8> {
    for width in 1..=8 {
        if value < (1_u64 << (width * 7)) - 1 {
            let mut bytes = value.to_be_bytes()[8 - width..].to_vec();
            bytes[0] |= 1 << (8 - width);
            return bytes;
        }
    }
    panic!("generated EBML size is outside its fixture range");
}
