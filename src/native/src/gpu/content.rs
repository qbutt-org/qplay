#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentKind {
    Anime,
    Movie,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentChoice {
    Auto,
    Anime,
    Movie,
}

impl ContentChoice {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "auto" => Some(Self::Auto),
            "anime" => Some(Self::Anime),
            "movie" => Some(Self::Movie),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Anime => "anime",
            Self::Movie => "movie",
        }
    }

    pub fn effective(self, detected: ContentKind) -> ContentKind {
        match self {
            Self::Auto => detected,
            Self::Anime => ContentKind::Anime,
            Self::Movie => ContentKind::Movie,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StyleMetrics {
    pub repeat_fraction: f64,
    pub repeat_bursts: f64,
    pub edge_average: f64,
    pub edge_high: f64,
    pub edge_entropy: f64,
    pub base_entropy: f64,
    pub saturation: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Classification {
    pub kind: ContentKind,
    pub confidence: f64,
    pub drawn_score: f64,
    pub cadence_score: f64,
}

fn unit(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub fn classify(metrics: StyleMetrics) -> Classification {
    let repeat_fraction = unit(metrics.repeat_fraction);
    let repeat_bursts = metrics.repeat_bursts.max(0.0).min(6.0);
    let edge_average = metrics.edge_average.max(0.0);
    let edge_high = metrics.edge_high.max(0.0);
    let edge_entropy = unit(metrics.edge_entropy);
    let base_entropy = unit(metrics.base_entropy);
    let saturation = metrics.saturation.max(0.0);

    let cadence_score =
        0.60 * unit((repeat_fraction - 0.12) / 0.43) + 0.40 * unit((repeat_bursts - 1.0) / 4.0);
    let drawn_score = 0.30 * unit((edge_average - 10.0) / 10.0)
        + 0.20 * unit((edge_high - 30.0) / 60.0)
        + 0.20 * unit((edge_entropy - 0.14) / 0.07)
        + 0.15 * unit((base_entropy - 0.64) / 0.14)
        + 0.15 * unit((saturation - 5.0) / 15.0);
    let anime_score = 0.55 * drawn_score + 0.45 * cadence_score;

    let strong_anime =
        drawn_score >= 0.48 && cadence_score >= 0.30 && anime_score >= 0.58 && edge_average >= 12.0;
    let low_detail_anime = edge_average >= 11.5
        && edge_high >= 40.0
        && edge_entropy >= 0.16
        && saturation >= 10.0
        && repeat_fraction >= 0.36
        && cadence_score >= 0.55
        && anime_score >= 0.38;
    let cadence_dominant_anime = cadence_score >= 0.90
        && repeat_fraction >= 0.60
        && repeat_bursts >= 5.0
        && drawn_score >= 0.25
        && anime_score >= 0.58;

    let (kind, confidence) = if strong_anime {
        (
            ContentKind::Anime,
            0.70 + 0.28 * unit((anime_score - 0.58) / 0.32),
        )
    } else if cadence_dominant_anime {
        (
            ContentKind::Anime,
            0.62 + 0.18 * unit((anime_score - 0.58) / 0.20),
        )
    } else if low_detail_anime {
        (
            ContentKind::Anime,
            0.56 + 0.18 * unit((anime_score - 0.38) / 0.22),
        )
    } else if drawn_score <= 0.20 {
        (
            ContentKind::Movie,
            0.76 + 0.20 * unit((0.20 - drawn_score) / 0.20),
        )
    } else if cadence_score <= 0.12 {
        (
            ContentKind::Movie,
            0.68 + 0.20 * unit((0.12 - cadence_score) / 0.12),
        )
    } else {
        (
            ContentKind::Unknown,
            0.45 + 0.10 * unit((anime_score - 0.50).abs() / 0.50),
        )
    };

    Classification {
        kind,
        confidence: unit(confidence),
        drawn_score,
        cadence_score,
    }
}
