use super::content::ContentKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    TensorRt,
    OpenVino,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FramegenChoice {
    Auto,
    On,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rate {
    pub num: u64,
    pub den: u64,
}

impl Rate {
    pub fn new(num: u64, den: u64) -> Option<Self> {
        if num == 0 || den == 0 {
            return None;
        }
        let mut left = num;
        let mut right = den;
        while right != 0 {
            (left, right) = (right, left % right);
        }
        Some(Self {
            num: num / left,
            den: den / left,
        })
    }

    pub fn fps(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    pub fn from_fps(fps: f64) -> Option<Self> {
        if !fps.is_finite() || fps <= 0.0 {
            return None;
        }
        for num in [24_000, 30_000, 48_000, 60_000, 120_000, 144_000] {
            let candidate = Self::new(num, 1001).unwrap();
            if (candidate.fps() - fps).abs() < 0.01 {
                return Some(candidate);
            }
        }
        Self::new((fps * 1000.0).round() as u64, 1000)
    }

    fn below(self, other: Self) -> bool {
        u128::from(self.num) * u128::from(other.den) < u128::from(other.num) * u128::from(self.den)
    }

    fn multiply(self, numerator: u64, denominator: u64) -> Option<Self> {
        Self::new(
            self.num.checked_mul(numerator)?,
            self.den.checked_mul(denominator)?,
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VideoInfo {
    pub has_video: bool,
    pub width: u32,
    pub height: u32,
    pub source_rate: Option<Rate>,
    pub display_rate: Option<Rate>,
    pub timing_verified: bool,
    pub interlaced: bool,
    pub content_kind: ContentKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unavailable {
    NoVideo,
    Runtime,
    Timing,
    Interlaced,
    Resolution,
    SourceRate,
    DisplayRate,
    NoHeadroom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub rate: Rate,
    pub multiplier: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
pub struct Selection {
    pub target: Target,
    pub output_width: u32,
    pub output_height: u32,
}

pub fn target_ladder(source: Rate, display: Rate, backend: Backend) -> Vec<Target> {
    if !source.below(display) {
        return Vec::new();
    }

    let Some(double) = source.multiply(2, 1) else {
        return Vec::new();
    };
    if display.below(double) {
        let mut targets = vec![Target {
            rate: display,
            multiplier: None,
        }];
        for n in 2..=8 {
            let Some(adjacent) = source.multiply(n + 1, n) else {
                break;
            };
            if !display.below(adjacent) {
                if adjacent != display {
                    targets.push(Target {
                        rate: adjacent,
                        multiplier: None,
                    });
                }
                break;
            }
        }
        return targets;
    }

    if backend == Backend::OpenVino {
        return vec![Target {
            rate: double,
            multiplier: Some(2),
        }];
    }

    let ratio_num = u128::from(display.num) * u128::from(source.den);
    let ratio_den = u128::from(display.den) * u128::from(source.num);
    let whole = ratio_num / ratio_den;
    let exact = ratio_num % ratio_den == 0;
    let mut targets = vec![Target {
        rate: display,
        multiplier: exact.then(|| u64::try_from(whole).ok()).flatten(),
    }];
    // More than 16 interpolated frames per decoded frame is outside the
    // useful playback range and would create an unbounded ladder for bad data.
    for multiplier in (2..=u64::try_from(whole.min(16)).unwrap()).rev() {
        let Some(candidate) = source.multiply(multiplier, 1) else {
            continue;
        };
        if candidate.below(display) {
            targets.push(Target {
                rate: candidate,
                multiplier: Some(multiplier),
            });
        }
    }
    targets
}

pub fn choose(
    choice: FramegenChoice,
    video: VideoInfo,
    backend: Option<Backend>,
    model_installed: bool,
) -> Result<Option<Selection>, Unavailable> {
    if choice == FramegenChoice::Off
        || choice == FramegenChoice::Auto && video.content_kind != ContentKind::Movie
    {
        return Ok(None);
    }
    if !video.has_video {
        return Err(Unavailable::NoVideo);
    }
    let Some(backend) = backend.filter(|_| model_installed) else {
        return Err(Unavailable::Runtime);
    };
    if !video.timing_verified {
        return Err(Unavailable::Timing);
    }
    if video.interlaced {
        return Err(Unavailable::Interlaced);
    }
    if video.width == 0 || video.height == 0 {
        return Err(Unavailable::Resolution);
    }
    let source = video.source_rate.ok_or(Unavailable::SourceRate)?;
    let display = video.display_rate.ok_or(Unavailable::DisplayRate)?;
    let targets = target_ladder(source, display, backend);
    let target = match backend {
        Backend::OpenVino => targets.first(),
        Backend::TensorRt => targets
            .iter()
            .find(|target| target.multiplier.is_some_and(|multiple| multiple <= 4))
            .or_else(|| targets.first()),
    }
    .ok_or(Unavailable::NoHeadroom)?;

    let scale = (1920.0 / video.width as f64)
        .min(1080.0 / video.height as f64)
        .min(1.0);
    let width = ((video.width as f64 * scale / 2.0).floor() as u32 * 2).max(2);
    let height = ((video.height as f64 * scale / 2.0).floor() as u32 * 2).max(2);
    Ok(Some(Selection {
        target: *target,
        output_width: width,
        output_height: height,
    }))
}

#[derive(Clone, Copy, Debug)]
pub struct QualitySample {
    pub playback_position: f64,
    pub filter_fps: f64,
    pub frame_drops: u64,
    pub decoder_drops: u64,
    pub mistimed_frames: u64,
    pub delayed_frames: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityAction {
    Keep,
    Lower,
    DisableUntilNextVideo,
}

#[derive(Clone, Copy, Debug)]
pub struct PlaybackState {
    pub now: f64,
    pub paused: bool,
    pub seeking: bool,
    pub caching: bool,
    pub eof: bool,
    pub idle: bool,
    pub filter_active: bool,
    pub output_ready: bool,
    pub sample: QualitySample,
}

pub struct SessionQuality {
    targets: Vec<Target>,
    index: usize,
    bad_windows: u8,
    disabled: bool,
    waiting_for_output: bool,
    settle_started: Option<f64>,
    first: Option<(f64, QualitySample)>,
}

impl SessionQuality {
    pub fn new(targets: Vec<Target>, initial: Rate) -> Option<Self> {
        let index = targets.iter().position(|target| target.rate == initial)?;
        Some(Self {
            targets,
            index,
            bad_windows: 0,
            disabled: false,
            waiting_for_output: true,
            settle_started: None,
            first: None,
        })
    }

    pub fn target(&self) -> Target {
        self.targets[self.index]
    }

    pub fn filter_changed(&mut self) {
        self.waiting_for_output = true;
        self.settle_started = None;
        self.first = None;
    }

    pub fn tick(&mut self, state: PlaybackState) -> QualityAction {
        if self.disabled || !state.now.is_finite() || !state.filter_active {
            self.first = None;
            return QualityAction::Keep;
        }
        if state.paused || state.seeking || state.caching || state.eof {
            self.settle_started = None;
            self.first = None;
            return QualityAction::Keep;
        }

        if self.waiting_for_output {
            let started = *self.settle_started.get_or_insert(state.now);
            let elapsed = state.now - started;
            if elapsed < 1.0 {
                return QualityAction::Keep;
            }
            if state.output_ready
                && state.sample.playback_position > 0.0
                && (state.sample.filter_fps - self.target().rate.fps()).abs()
                    <= self.target().rate.fps() * 0.03
            {
                self.waiting_for_output = false;
                self.settle_started = None;
                self.first = Some((state.now, state.sample));
                return QualityAction::Keep;
            }
            if elapsed < 15.0 {
                return QualityAction::Keep;
            }
            self.filter_changed();
            return self.lower();
        }

        if state.idle || !state.output_ready || state.sample.playback_position <= 0.0 {
            self.first = None;
            return QualityAction::Keep;
        }
        let (started, first) = *self.first.get_or_insert((state.now, state.sample));
        let elapsed = state.now - started;
        if elapsed < 4.0 {
            return QualityAction::Keep;
        }
        self.first = None;
        if state.sample.frame_drops < first.frame_drops
            || state.sample.decoder_drops < first.decoder_drops
            || state.sample.mistimed_frames < first.mistimed_frames
            || state.sample.delayed_frames < first.delayed_frames
        {
            self.filter_changed();
            return QualityAction::Keep;
        }
        let action = self.observe(first, state.sample, elapsed);
        if action == QualityAction::Lower {
            self.filter_changed();
        }
        action
    }

    fn lower(&mut self) -> QualityAction {
        self.bad_windows = 0;
        if self.index + 1 < self.targets.len() {
            self.index += 1;
            QualityAction::Lower
        } else {
            self.disabled = true;
            QualityAction::DisableUntilNextVideo
        }
    }

    fn observe(
        &mut self,
        first: QualitySample,
        last: QualitySample,
        seconds: f64,
    ) -> QualityAction {
        if self.disabled
            || !seconds.is_finite()
            || seconds < 4.0
            || !first.playback_position.is_finite()
            || !last.playback_position.is_finite()
            || !last.filter_fps.is_finite()
        {
            return QualityAction::Keep;
        }
        if last.frame_drops < first.frame_drops
            || last.decoder_drops < first.decoder_drops
            || last.mistimed_frames < first.mistimed_frames
            || last.delayed_frames < first.delayed_frames
        {
            self.bad_windows = 0;
            return QualityAction::Keep;
        }

        let target_fps = self.target().rate.fps();
        let progress = last.playback_position - first.playback_position;
        let frame_drops = last.frame_drops - first.frame_drops;
        let unhealthy = progress < seconds * 0.90
            || last.filter_fps < target_fps * 0.97
            || frame_drops > 1
            || last.decoder_drops > first.decoder_drops
            || last.mistimed_frames - first.mistimed_frames > 1
            || last.delayed_frames - first.delayed_frames > 2;
        if !unhealthy {
            self.bad_windows = 0;
            return QualityAction::Keep;
        }

        let severe = progress < seconds * 0.75
            || last.filter_fps < target_fps * 0.90
            || frame_drops as f64 > (target_fps * seconds * 0.03).max(4.0);
        self.bad_windows += 1;
        if !severe && self.bad_windows < 2 {
            return QualityAction::Keep;
        }
        self.lower()
    }
}
