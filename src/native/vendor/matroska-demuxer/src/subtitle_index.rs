//! Exact subtitle block discovery when a track has no complete Cue index.

use std::io::{Read, Seek, SeekFrom};

use crate::ebml::{next_element, parse_element_header, parse_variable_u64, ElementData};
use crate::{DemuxError, ElementId, Result, SubtitleCue, MAX_COLLECTION_ENTRIES};

struct Cluster {
    position: u64,
    data: u64,
    end: u64,
    timestamp: Option<u64>,
    unknown_size: bool,
}

struct Block {
    offset: u64,
    size: u64,
    duration: Option<u64>,
}

pub(crate) fn scan<R: Read + Seek>(
    file: &mut R,
    first_cluster: u64,
    track: u64,
) -> Result<Vec<SubtitleCue>> {
    let file_end = file.seek(SeekFrom::End(0))?;
    file.seek(SeekFrom::Start(first_cluster))?;
    let mut cues = Vec::new();
    let mut cluster: Option<Cluster> = None;
    let mut scanned = 0_usize;

    while file.stream_position()? < file_end {
        if scanned >= MAX_COLLECTION_ENTRIES {
            return Err(DemuxError::ResourceLimitExceeded {
                declared: u64::try_from(scanned)? + 1,
                limit: u64::try_from(MAX_COLLECTION_ENTRIES)?,
            });
        }
        scanned += 1;
        let position = file.stream_position()?;
        if cluster
            .as_ref()
            .is_some_and(|cluster| position == cluster.end)
        {
            cluster = None;
        }
        let (id, size) = parse_element_header(file, None)?;
        let data = file.stream_position()?;
        let end = if id == ElementId::Cluster && size == u64::MAX {
            file_end
        } else {
            data.checked_add(size)
                .filter(|end| *end <= file_end)
                .ok_or(DemuxError::InvalidEbmlDataSize)?
        };
        if id == ElementId::Cluster {
            if cluster
                .as_ref()
                .is_some_and(|cluster| !cluster.unknown_size)
            {
                return Err(DemuxError::InvalidEbmlDataSize);
            }
            cluster = Some(Cluster {
                position,
                data,
                end,
                timestamp: None,
                unknown_size: size == u64::MAX,
            });
            continue;
        }
        let Some(cluster) = &mut cluster else {
            file.seek(SeekFrom::Start(end))?;
            continue;
        };
        if end > cluster.end {
            return Err(DemuxError::InvalidEbmlDataSize);
        }
        match id {
            ElementId::Timestamp => {
                if cluster.timestamp.is_some() {
                    return Err(DemuxError::InvalidBlockSize);
                }
                file.seek(SeekFrom::Start(position))?;
                let (_, ElementData::Unsigned(value)) = next_element(file)? else {
                    return Err(DemuxError::UnexpectedDataType);
                };
                cluster.timestamp = Some(value);
            }
            ElementId::BlockGroup | ElementId::SimpleBlock => {
                let cluster_timestamp = cluster
                    .timestamp
                    .ok_or(DemuxError::ElementNotFound(ElementId::Timestamp))?;
                let block = if id == ElementId::BlockGroup {
                    block_group(file, end)?
                } else {
                    Block {
                        offset: data,
                        size,
                        duration: None,
                    }
                };
                file.seek(SeekFrom::Start(block.offset))?;
                let block_track = parse_variable_u64(file)?;
                let block_end = block
                    .offset
                    .checked_add(block.size)
                    .ok_or(DemuxError::InvalidBlockSize)?;
                if block_track == 0
                    || file
                        .stream_position()?
                        .checked_add(3)
                        .map_or(true, |header_end| header_end > block_end)
                {
                    return Err(DemuxError::InvalidBlockSize);
                }
                if block_track == track {
                    let mut relative = [0_u8; 2];
                    file.read_exact(&mut relative)?;
                    let timestamp = i128::from(cluster_timestamp)
                        + i128::from(i16::from_be_bytes([relative[0], relative[1]]));
                    let timestamp = u64::try_from(timestamp)?;
                    let duration = block
                        .duration
                        .filter(|duration| *duration > 0)
                        .ok_or(DemuxError::ElementNotFound(ElementId::BlockDuration))?;
                    cues.push(SubtitleCue {
                        timestamp,
                        duration,
                        cluster_position: cluster.position,
                        relative_position: position - cluster.data,
                        track,
                    });
                }
            }
            _ => {}
        }
        file.seek(SeekFrom::Start(end))?;
    }
    Ok(cues)
}

fn block_group<R: Read + Seek>(file: &mut R, end: u64) -> Result<Block> {
    let mut block = None;
    let mut duration = None;
    let mut children = 0;
    while file.stream_position()? < end {
        if children >= 64 {
            return Err(DemuxError::ResourceLimitExceeded {
                declared: 65,
                limit: 64,
            });
        }
        children += 1;
        let position = file.stream_position()?;
        let (id, size) = parse_element_header(file, None)?;
        let data = file.stream_position()?;
        let child_end = data
            .checked_add(size)
            .filter(|child_end| *child_end <= end)
            .ok_or(DemuxError::InvalidBlockSize)?;
        match id {
            ElementId::Block if size < 4 || block.is_some() => {
                return Err(DemuxError::InvalidBlockSize);
            }
            ElementId::Block => block = Some((data, size)),
            ElementId::BlockDuration => {
                file.seek(SeekFrom::Start(position))?;
                let (_, ElementData::Unsigned(value)) = next_element(file)? else {
                    return Err(DemuxError::UnexpectedDataType);
                };
                if duration.replace(value).is_some() {
                    return Err(DemuxError::InvalidBlockSize);
                }
            }
            _ => {}
        }
        file.seek(SeekFrom::Start(child_end))?;
    }
    let (offset, size) = block.ok_or(DemuxError::ElementNotFound(ElementId::Block))?;
    Ok(Block {
        offset,
        size,
        duration,
    })
}
