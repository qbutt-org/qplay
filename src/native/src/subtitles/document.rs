use std::collections::HashMap;
use std::ops::Range;

use sha2::{Digest, Sha256};
use subcue::{Format, Warning};

const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_CUES: usize = 50_000;
const MAX_FRAGMENTS: usize = 200_000;
const MAX_CUE_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubtitleFormat {
    Ass,
    Ssa,
    Srt,
    Vtt,
}

impl SubtitleFormat {
    fn parser_format(self) -> Format {
        match self {
            Self::Ass | Self::Ssa => Format::Ass,
            Self::Srt => Format::Srt,
            Self::Vtt => Format::Vtt,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SubtitleError {
    #[error("The text subtitle is invalid or unsupported")]
    Invalid,
    #[error("The text subtitle exceeds its size or cue limit")]
    TooLarge,
    #[error("The translated subtitle text is invalid")]
    InvalidTranslation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubtitleFragment {
    pub id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    source: Range<usize>,
}

pub struct SubtitleDocument {
    pub format: SubtitleFormat,
    original: String,
    fragments: Vec<SubtitleFragment>,
}

#[derive(Clone, Copy)]
struct Line<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

impl SubtitleDocument {
    pub fn parse(original: &str, format: SubtitleFormat) -> Result<Self, SubtitleError> {
        if original.len() > MAX_SOURCE_BYTES {
            return Err(SubtitleError::TooLarge);
        }
        if original.chars().any(|character| {
            character == '\0'
                || (character.is_control() && !matches!(character, '\r' | '\n' | '\t'))
        }) {
            return Err(SubtitleError::Invalid);
        }
        let lines = lines(original)?;
        let bodies = match format {
            SubtitleFormat::Ass | SubtitleFormat::Ssa => ass_bodies(&lines)?,
            SubtitleFormat::Srt | SubtitleFormat::Vtt => block_bodies(&lines, format)?,
        };
        if bodies.len() > MAX_CUES {
            return Err(SubtitleError::TooLarge);
        }
        let parsed = subcue::parse(original.as_bytes(), format.parser_format())
            .map_err(|_| SubtitleError::Invalid)?;
        if parsed
            .warnings
            .iter()
            .any(|warning| matches!(warning, Warning::EndBeforeStart { .. } | Warning::NoCues))
        {
            return Err(SubtitleError::Invalid);
        }

        if bodies.len() != parsed.cues.len() {
            return Err(SubtitleError::Invalid);
        }

        let mut fragments = Vec::new();
        let mut duplicates = HashMap::new();
        for (cue, body) in parsed.cues.iter().zip(bodies) {
            let raw = &original[body.clone()];
            let comparable = if matches!(format, SubtitleFormat::Ass | SubtitleFormat::Ssa) {
                raw.trim_start().to_owned()
            } else {
                raw.replace("\r\n", "\n").replace('\r', "\n")
            };
            if comparable != cue.text || body.len() > MAX_CUE_BYTES {
                return Err(SubtitleError::Invalid);
            }
            let occurrence = duplicates
                .entry((cue.start.0, cue.end.0, raw))
                .and_modify(|count| *count += 1)
                .or_insert(0_u32);
            let digest = Sha256::digest(
                format!(
                    "{}:{}:{}:{}:{:?}:{:?}:{:?}",
                    cue.start.0, cue.end.0, raw, occurrence, cue.id, cue.settings, cue.ass
                )
                .as_bytes(),
            );
            let cue_id = format!("{digest:x}");
            let ranges = text_ranges(raw, format)?;
            for (index, local) in ranges.into_iter().enumerate() {
                if fragments.len() >= MAX_FRAGMENTS {
                    return Err(SubtitleError::TooLarge);
                }
                let source = body.start + local.start..body.start + local.end;
                fragments.push(SubtitleFragment {
                    id: format!("{cue_id}:{index}"),
                    start_ms: cue.start.0,
                    end_ms: cue.end.0,
                    text: decode_entities(&original[source.clone()]),
                    source,
                });
            }
        }
        Ok(Self {
            format,
            original: original.to_owned(),
            fragments,
        })
    }

    pub fn original(&self) -> &str {
        &self.original
    }

    pub fn fragments(&self) -> &[SubtitleFragment] {
        &self.fragments
    }

    pub fn render(&self, accepted: &HashMap<String, String>) -> Result<String, SubtitleError> {
        let mut output = String::with_capacity(self.original.len());
        let mut cursor = 0;
        for fragment in &self.fragments {
            let Some(translation) = accepted.get(&fragment.id) else {
                continue;
            };
            let safe = safe_translation(translation, self.format)?;
            output.push_str(&self.original[cursor..fragment.source.start]);
            output.push_str(&safe);
            cursor = fragment.source.end;
            if output.len() > MAX_SOURCE_BYTES {
                return Err(SubtitleError::TooLarge);
            }
        }
        output.push_str(&self.original[cursor..]);
        if output.len() > MAX_SOURCE_BYTES {
            return Err(SubtitleError::TooLarge);
        }
        Ok(output)
    }
}

fn lines(source: &str) -> Result<Vec<Line<'_>>, SubtitleError> {
    let bytes = source.as_bytes();
    let mut result = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\r' || bytes[index] == b'\n' {
            if result.len() >= 250_000 {
                return Err(SubtitleError::TooLarge);
            }
            result.push(Line {
                text: &source[start..index],
                start,
                end: index,
            });
            if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                index += 1;
            }
            index += 1;
            start = index;
        } else {
            index += 1;
        }
    }
    if start < source.len() {
        if result.len() >= 250_000 {
            return Err(SubtitleError::TooLarge);
        }
        result.push(Line {
            text: &source[start..],
            start,
            end: source.len(),
        });
    }
    Ok(result)
}

fn ass_bodies(lines: &[Line<'_>]) -> Result<Vec<Range<usize>>, SubtitleError> {
    let mut events = false;
    let mut text_field = None;
    let mut field_count = 0;
    let mut bodies = Vec::new();
    for line in lines {
        let text = line.text.trim();
        if text.starts_with('[') && text.ends_with(']') {
            events = text.eq_ignore_ascii_case("[Events]");
            text_field = None;
            continue;
        }
        if !events {
            continue;
        }
        if let Some((name, fields)) = text.split_once(':') {
            if name.trim().eq_ignore_ascii_case("Format") {
                let fields: Vec<_> = fields.split(',').map(str::trim).collect();
                text_field = fields
                    .iter()
                    .position(|field| field.eq_ignore_ascii_case("Text"));
                field_count = fields.len();
                continue;
            }
            if !name.trim().eq_ignore_ascii_case("Dialogue") {
                continue;
            }
            let text_index = text_field.ok_or(SubtitleError::Invalid)?;
            let colon = line.text.find(':').ok_or(SubtitleError::Invalid)?;
            let mut left = colon + 1;
            let mut right = line.text.len();
            for _ in 0..text_index {
                let comma = line.text[left..].find(',').ok_or(SubtitleError::Invalid)?;
                left += comma + 1;
            }
            for _ in text_index + 1..field_count {
                let comma = line.text[..right]
                    .rfind(',')
                    .ok_or(SubtitleError::Invalid)?;
                if comma < left {
                    return Err(SubtitleError::Invalid);
                }
                right = comma;
            }
            if right < left {
                return Err(SubtitleError::Invalid);
            }
            bodies.push(line.start + left..line.start + right);
        }
    }
    Ok(bodies)
}

fn block_bodies(
    lines: &[Line<'_>],
    format: SubtitleFormat,
) -> Result<Vec<Range<usize>>, SubtitleError> {
    let mut bodies = Vec::new();
    let mut cursor = 0;
    if format == SubtitleFormat::Vtt {
        while cursor < lines.len() && !lines[cursor].text.is_empty() {
            cursor += 1;
        }
    }
    while cursor < lines.len() {
        if lines[cursor].text.is_empty() {
            cursor += 1;
            continue;
        }
        let first = cursor;
        while cursor < lines.len() && !lines[cursor].text.is_empty() {
            cursor += 1;
        }
        let block = &lines[first..cursor];
        if format == SubtitleFormat::Vtt
            && (block[0].text == "STYLE"
                || block[0].text == "REGION"
                || block[0].text == "NOTE"
                || block[0].text.starts_with("NOTE "))
        {
            continue;
        }
        let timing = if block[0].text.contains("-->") { 0 } else { 1 };
        if timing >= block.len() || !block[timing].text.contains("-->") {
            return Err(SubtitleError::Invalid);
        }
        let start = block
            .get(timing + 1)
            .map_or(block[timing].end, |line| line.start);
        bodies.push(start..block.last().expect("nonempty block").end);
    }
    Ok(bodies)
}

fn text_ranges(source: &str, format: SubtitleFormat) -> Result<Vec<Range<usize>>, SubtitleError> {
    let ass = matches!(format, SubtitleFormat::Ass | SubtitleFormat::Ssa);
    let mut result = Vec::new();
    let mut cursor = 0;
    let mut index = 0;
    let mut drawing = false;
    while index < source.len() {
        let rest = &source[index..];
        let was_drawing = drawing;
        let control_end = if ass && rest.starts_with('{') {
            let close = rest.find('}').ok_or(SubtitleError::Invalid)?;
            let control = &rest[..=close];
            for tag in control.split("\\p").skip(1) {
                let digits: String = tag
                    .trim_start()
                    .chars()
                    .take_while(|character| character.is_ascii_digit())
                    .collect();
                if let Ok(level) = digits.parse::<u32>() {
                    drawing = level > 0;
                }
            }
            Some(index + close + 1)
        } else if ass
            && (rest.starts_with("\\N")
                || rest.starts_with("\\n")
                || rest.starts_with("\\h")
                || rest.starts_with("\\{")
                || rest.starts_with("\\}"))
        {
            Some(index + 2)
        } else if !ass && rest.starts_with('<') {
            let close = rest.find('>');
            close
                .filter(|&close| {
                    close <= 256
                        && rest[1..].starts_with(|character: char| {
                            character.is_ascii_alphabetic()
                                || character == '/'
                                || (format == SubtitleFormat::Vtt
                                    && character.is_ascii_digit()
                                    && rest[1..close].contains(':'))
                        })
                })
                .map(|close| index + close + 1)
        } else if !ass
            && (rest.starts_with("\r\n") || rest.starts_with('\r') || rest.starts_with('\n'))
        {
            Some(index + if rest.starts_with("\r\n") { 2 } else { 1 })
        } else {
            None
        };
        if let Some(end) = control_end {
            if !was_drawing {
                push_text(&mut result, source, cursor..index);
            }
            cursor = end;
            index = end;
        } else {
            index += rest.chars().next().expect("nonempty rest").len_utf8();
        }
    }
    if !drawing {
        push_text(&mut result, source, cursor..source.len());
    }
    Ok(result)
}

fn push_text(result: &mut Vec<Range<usize>>, source: &str, range: Range<usize>) {
    let text = &source[range.clone()];
    let leading = text.len() - text.trim_start().len();
    let trailing = text.trim_end().len();
    if leading < trailing {
        result.push(range.start + leading..range.start + trailing);
    }
}

fn decode_entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn safe_translation(text: &str, format: SubtitleFormat) -> Result<String, SubtitleError> {
    if text.len() > MAX_CUE_BYTES
        || text
            .chars()
            .any(|character| character.is_control() && character != '\n')
    {
        return Err(SubtitleError::InvalidTranslation);
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err(SubtitleError::InvalidTranslation);
    }
    let text = text
        .replace('\\', "＼")
        .replace('{', "｛")
        .replace('}', "｝");
    Ok(match format {
        SubtitleFormat::Vtt => text
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;"),
        SubtitleFormat::Srt => text.replace('<', "＜").replace('>', "＞"),
        SubtitleFormat::Ass | SubtitleFormat::Ssa => text,
    })
}
