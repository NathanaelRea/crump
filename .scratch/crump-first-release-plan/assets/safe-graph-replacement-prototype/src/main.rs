use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::f32::consts::TAU;
use std::fs::{self, File};
use std::io::{self, BufRead, IsTerminal, Write};
use std::mem::MaybeUninit;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

const SAMPLE_RATE: f32 = 48_000.0;
const BLOCK_SIZE: usize = 128;
const CROSSFADE_SAMPLES: usize = 192;
const ZERO_CROSSING_THRESHOLD: f32 = 0.002;
const MAX_ZERO_CROSSING_WAIT: usize = 2_400;
const QUEUE_SLOTS: usize = 8;
const OUTPUT_LIMIT: f32 = 0.95;
const DEMO_FIRST_CHANGE: usize = 24_055;
const DEMO_CHANGE_INTERVAL: usize = 24_000;

thread_local! {
    static IN_CALLBACK: Cell<bool> = const { Cell::new(false) };
}

static CALLBACK_ALLOCS: AtomicU64 = AtomicU64::new(0);
static CALLBACK_DEALLOCS: AtomicU64 = AtomicU64::new(0);

struct TrackingAllocator;

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        IN_CALLBACK.with(|flag| {
            if flag.get() {
                CALLBACK_ALLOCS.fetch_add(1, Ordering::Relaxed);
            }
        });
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        IN_CALLBACK.with(|flag| {
            if flag.get() {
                CALLBACK_DEALLOCS.fetch_add(1, Ordering::Relaxed);
            }
        });
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

struct Ring<T, const N: usize> {
    slots: [std::cell::UnsafeCell<MaybeUninit<T>>; N],
    head: AtomicUsize,
    tail: AtomicUsize,
}

unsafe impl<T: Send, const N: usize> Sync for Ring<T, N> {}

impl<T, const N: usize> Ring<T, N> {
    fn new() -> Self {
        assert!(N >= 2);
        Self {
            slots: std::array::from_fn(|_| std::cell::UnsafeCell::new(MaybeUninit::uninit())),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    fn has_space(&self) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let next = (tail + 1) % N;
        next != self.head.load(Ordering::Acquire)
    }

    fn push(&self, value: T) -> Result<(), T> {
        let tail = self.tail.load(Ordering::Relaxed);
        let next = (tail + 1) % N;
        if next == self.head.load(Ordering::Acquire) {
            return Err(value);
        }
        unsafe { (*self.slots[tail].get()).write(value) };
        self.tail.store(next, Ordering::Release);
        Ok(())
    }

    fn pop(&self) -> Option<T> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let value = unsafe { (*self.slots[head].get()).assume_init_read() };
        self.head.store((head + 1) % N, Ordering::Release);
        Some(value)
    }
}

impl<T, const N: usize> Drop for Ring<T, N> {
    fn drop(&mut self) {
        while self.pop().is_some() {}
    }
}

#[derive(Clone, Copy)]
struct Source {
    gain: f32,
    frequency: f32,
}

struct Graph {
    revision: u64,
    gain: f32,
    phase: f32,
    phase_step: f32,
}

impl Graph {
    fn compile(revision: u64, source: Source) -> Box<Self> {
        Box::new(Self {
            revision,
            gain: source.gain,
            phase: 0.0,
            phase_step: source.frequency / SAMPLE_RATE,
        })
    }

    fn sample(&mut self) -> f32 {
        let value = (self.phase * TAU).sin() * self.gain;
        self.phase = (self.phase + self.phase_step).fract();
        value
    }
}

enum PlaybackState {
    Active(Box<Graph>),
    Crossfading {
        old: Box<Graph>,
        new: Box<Graph>,
        position: usize,
    },
    WaitingForZero {
        old: Box<Graph>,
        new: Box<Graph>,
        remaining: usize,
    },
}

#[derive(Clone, Copy)]
enum TransitionMode {
    Crossfade,
    ZeroCrossing,
    HardSwap,
}

impl TransitionMode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "crossfade" => Some(Self::Crossfade),
            "zero-crossing" => Some(Self::ZeroCrossing),
            "hard-swap" => Some(Self::HardSwap),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Crossfade => "crossfade",
            Self::ZeroCrossing => "zero-crossing",
            Self::HardSwap => "hard-swap",
        }
    }

    fn all() -> [Self; 3] {
        [Self::Crossfade, Self::ZeroCrossing, Self::HardSwap]
    }
}

struct Telemetry {
    candidate_revision: AtomicU64,
    active_revision: AtomicU64,
    rejected_revisions: AtomicU64,
    swaps: AtomicU64,
    callbacks: AtomicU64,
    peak_bits: AtomicU32,
    max_delta_bits: AtomicU32,
}

impl Telemetry {
    fn new() -> Self {
        Self {
            candidate_revision: AtomicU64::new(0),
            active_revision: AtomicU64::new(0),
            rejected_revisions: AtomicU64::new(0),
            swaps: AtomicU64::new(0),
            callbacks: AtomicU64::new(0),
            peak_bits: AtomicU32::new(0.0f32.to_bits()),
            max_delta_bits: AtomicU32::new(0.0f32.to_bits()),
        }
    }
}

fn update_max(target: &AtomicU32, value: f32) {
    let mut current = target.load(Ordering::Relaxed);
    while value > f32::from_bits(current) {
        match target.compare_exchange_weak(
            current,
            value.to_bits(),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

fn parse_source(line: &str) -> Result<Source, &'static str> {
    let parts: Vec<_> = line.split_whitespace().collect();
    if parts.len() != 4 || parts[0] != "gain" || parts[2] != "freq" {
        return Err("expected: gain <0..2> freq <20..1000>");
    }
    let gain = parts[1]
        .parse::<f32>()
        .map_err(|_| "gain is not a number")?;
    let frequency = parts[3]
        .parse::<f32>()
        .map_err(|_| "frequency is not a number")?;
    if !gain.is_finite() || !(0.0..=2.0).contains(&gain) {
        return Err("gain must be finite and between 0 and 2");
    }
    if !frequency.is_finite() || !(20.0..=1000.0).contains(&frequency) {
        return Err("frequency must be finite and between 20 and 1000");
    }
    Ok(Source { gain, frequency })
}

fn audio_callback(
    state: &mut Option<PlaybackState>,
    incoming: &Ring<Box<Graph>, QUEUE_SLOTS>,
    retired: &Ring<Box<Graph>, QUEUE_SLOTS>,
    telemetry: &Telemetry,
    previous_sample: &mut f32,
    mode: TransitionMode,
) {
    IN_CALLBACK.with(|flag| flag.set(true));

    if matches!(state, Some(PlaybackState::Active(_)))
        && retired.has_space()
        && let Some(new) = incoming.pop()
    {
        let Some(PlaybackState::Active(old)) = state.take() else {
            unreachable!();
        };
        match mode {
            TransitionMode::Crossfade => {
                *state = Some(PlaybackState::Crossfading {
                    old,
                    new,
                    position: 0,
                });
            }
            TransitionMode::ZeroCrossing => {
                *state = Some(PlaybackState::WaitingForZero {
                    old,
                    new,
                    remaining: MAX_ZERO_CROSSING_WAIT,
                });
            }
            TransitionMode::HardSwap => {
                let revision = new.revision;
                if let Err(_old) = retired.push(old) {
                    unreachable!("retirement slot was reserved by the single producer");
                }
                *state = Some(PlaybackState::Active(new));
                telemetry.active_revision.store(revision, Ordering::Release);
                telemetry.swaps.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    for _ in 0..BLOCK_SIZE {
        let mut zero_crossing_reached = false;
        let raw = match state.as_mut().expect("audio state exists") {
            PlaybackState::Active(graph) => graph.sample(),
            PlaybackState::Crossfading { old, new, position } => {
                let alpha = *position as f32 / (CROSSFADE_SAMPLES - 1) as f32;
                *position += 1;
                old.sample() * (1.0 - alpha) + new.sample() * alpha
            }
            PlaybackState::WaitingForZero { old, remaining, .. } => {
                let sample = old.sample();
                *remaining -= 1;
                let crossed_zero = (*previous_sample < 0.0 && sample >= 0.0)
                    || (*previous_sample > 0.0 && sample <= 0.0);
                zero_crossing_reached =
                    sample.abs() <= ZERO_CROSSING_THRESHOLD || crossed_zero || *remaining == 0;
                sample
            }
        };
        let output = if raw.is_finite() {
            raw.clamp(-OUTPUT_LIMIT, OUTPUT_LIMIT)
        } else {
            0.0
        };
        update_max(&telemetry.peak_bits, output.abs());
        update_max(&telemetry.max_delta_bits, (output - *previous_sample).abs());
        *previous_sample = output;

        if zero_crossing_reached {
            let Some(PlaybackState::WaitingForZero { old, new, .. }) = state.take() else {
                unreachable!();
            };
            let revision = new.revision;
            if let Err(_old) = retired.push(old) {
                unreachable!("retirement slot was reserved by the single producer");
            }
            *state = Some(PlaybackState::Active(new));
            telemetry.active_revision.store(revision, Ordering::Release);
            telemetry.swaps.fetch_add(1, Ordering::Relaxed);
        }
    }

    let transition_done = matches!(
        state,
        Some(PlaybackState::Crossfading { position, .. }) if *position >= CROSSFADE_SAMPLES
    );
    if transition_done && retired.has_space() {
        let Some(PlaybackState::Crossfading { old, new, .. }) = state.take() else {
            unreachable!();
        };
        let revision = new.revision;
        if let Err(_old) = retired.push(old) {
            unreachable!("retirement slot was reserved by the single producer");
        }
        *state = Some(PlaybackState::Active(new));
        telemetry.active_revision.store(revision, Ordering::Release);
        telemetry.swaps.fetch_add(1, Ordering::Relaxed);
    }

    telemetry.callbacks.fetch_add(1, Ordering::Relaxed);
    IN_CALLBACK.with(|flag| flag.set(false));
}

fn publish(
    source_text: &str,
    outgoing: &Ring<Box<Graph>, QUEUE_SLOTS>,
    retired: &Ring<Box<Graph>, QUEUE_SLOTS>,
    telemetry: &Telemetry,
) -> Result<u64, &'static str> {
    while retired.pop().is_some() {}
    let revision = telemetry.candidate_revision.fetch_add(1, Ordering::Relaxed) + 1;
    let source = match parse_source(source_text) {
        Ok(source) => source,
        Err(error) => {
            telemetry.rejected_revisions.fetch_add(1, Ordering::Relaxed);
            return Err(error);
        }
    };
    let mut graph = Graph::compile(revision, source);
    loop {
        match outgoing.push(graph) {
            Ok(()) => break,
            Err(returned) => {
                graph = returned;
                while retired.pop().is_some() {}
                thread::sleep(Duration::from_millis(1));
            }
        }
    }
    Ok(revision)
}

fn show(telemetry: &Telemetry, message: &str, clear: bool, mode: TransitionMode) {
    if clear {
        print!("\x1b[2J\x1b[H");
    }
    println!("PROTOTYPE: safe graph replacement\n");
    println!("transition version : {}", mode.name());
    println!(
        "candidate revision : {}",
        telemetry.candidate_revision.load(Ordering::Relaxed)
    );
    println!(
        "active revision    : {}",
        telemetry.active_revision.load(Ordering::Acquire)
    );
    println!(
        "rejected revisions : {}",
        telemetry.rejected_revisions.load(Ordering::Relaxed)
    );
    println!(
        "completed swaps    : {}",
        telemetry.swaps.load(Ordering::Relaxed)
    );
    println!(
        "audio callbacks    : {}",
        telemetry.callbacks.load(Ordering::Relaxed)
    );
    println!(
        "callback allocs    : {}",
        CALLBACK_ALLOCS.load(Ordering::Relaxed)
    );
    println!(
        "callback deallocs  : {}",
        CALLBACK_DEALLOCS.load(Ordering::Relaxed)
    );
    println!(
        "peak output        : {:.4}",
        f32::from_bits(telemetry.peak_bits.load(Ordering::Relaxed))
    );
    println!(
        "worst sample delta : {:.4}\n",
        f32::from_bits(telemetry.max_delta_bits.load(Ordering::Relaxed))
    );
    println!("{message}\n");
    println!("gain <0..2> freq <20..1000> | stress | quit");
    print!("> ");
    io::stdout().flush().expect("flush display");
}

fn run_stress(
    outgoing: &Ring<Box<Graph>, QUEUE_SLOTS>,
    retired: &Ring<Box<Graph>, QUEUE_SLOTS>,
    telemetry: &Telemetry,
) {
    for index in 0..200 {
        let source = if index % 7 == 0 {
            "gain invalid freq 440".to_owned()
        } else {
            format!(
                "gain {:.2} freq {}",
                0.1 + (index % 20) as f32 / 10.0,
                80 + (index * 37) % 920
            )
        };
        match publish(&source, outgoing, retired, telemetry) {
            Ok(_) | Err("gain is not a number") => {}
            Err(error) => panic!("unexpected stress error: {error}"),
        }
    }

    let expected_swaps = telemetry.candidate_revision.load(Ordering::Relaxed)
        - telemetry.rejected_revisions.load(Ordering::Relaxed);
    while telemetry.swaps.load(Ordering::Relaxed) < expected_swaps {
        while retired.pop().is_some() {}
        thread::sleep(Duration::from_millis(1));
    }
}

fn render_demo(mode: TransitionMode) -> Vec<f32> {
    let duration_samples = SAMPLE_RATE as usize * 4;
    let frequencies = [220.0, 880.0, 330.0, 990.0, 440.0, 770.0, 275.0, 660.0];
    let mut frequency_index = 0;
    let mut active = Graph::compile(
        0,
        Source {
            gain: 0.8,
            frequency: frequencies[frequency_index],
        },
    );
    let mut transition: Option<(Box<Graph>, usize)> = None;
    let mut samples = Vec::with_capacity(duration_samples);
    let mut previous = 0.0;

    for index in 0..duration_samples {
        if index >= DEMO_FIRST_CHANGE
            && (index - DEMO_FIRST_CHANGE).is_multiple_of(DEMO_CHANGE_INTERVAL)
        {
            frequency_index = (frequency_index + 1) % frequencies.len();
            transition = Some((
                Graph::compile(
                    index as u64,
                    Source {
                        gain: 0.8,
                        frequency: frequencies[frequency_index],
                    },
                ),
                0,
            ));
        }

        let mut finish = false;
        let raw = if let Some((next, progress)) = transition.as_mut() {
            match mode {
                TransitionMode::HardSwap => {
                    finish = true;
                    next.sample()
                }
                TransitionMode::Crossfade => {
                    let alpha = *progress as f32 / (CROSSFADE_SAMPLES - 1) as f32;
                    *progress += 1;
                    finish = *progress >= CROSSFADE_SAMPLES;
                    active.sample() * (1.0 - alpha) + next.sample() * alpha
                }
                TransitionMode::ZeroCrossing => {
                    let sample = active.sample();
                    *progress += 1;
                    let crossed_zero =
                        (previous < 0.0 && sample >= 0.0) || (previous > 0.0 && sample <= 0.0);
                    finish = sample.abs() <= ZERO_CROSSING_THRESHOLD
                        || crossed_zero
                        || *progress >= MAX_ZERO_CROSSING_WAIT;
                    sample
                }
            }
        } else {
            active.sample()
        };

        let output = raw.clamp(-OUTPUT_LIMIT, OUTPUT_LIMIT);
        samples.push(output);
        previous = output;
        if finish {
            let (next, _) = transition.take().expect("transition exists");
            active = next;
        }
    }
    samples
}

fn write_wav(path: &Path, samples: &[f32]) -> io::Result<()> {
    let mut file = File::create(path)?;
    let data_bytes = (samples.len() * 2) as u32;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_bytes).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&(SAMPLE_RATE as u32).to_le_bytes())?;
    file.write_all(&((SAMPLE_RATE as u32) * 2).to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_bytes.to_le_bytes())?;
    for sample in samples {
        let value = (sample * i16::MAX as f32) as i16;
        file.write_all(&value.to_le_bytes())?;
    }
    Ok(())
}

fn plot_transition(samples: &[f32], mode: TransitionMode) {
    const WIDTH: usize = 96;
    const HEIGHT: usize = 17;
    const BEFORE: usize = 48;
    const WINDOW: usize = 384;
    let request = DEMO_FIRST_CHANGE;
    let start = request - BEFORE;
    let mut canvas = [[' '; WIDTH]; HEIGHT];
    let zero_row = HEIGHT / 2;
    for cell in &mut canvas[zero_row] {
        *cell = '-';
    }
    let request_column = BEFORE * WIDTH / WINDOW;
    for row in &mut canvas {
        row[request_column] = '|';
    }
    let plotted_rows: [usize; WIDTH] = std::array::from_fn(|column| {
        let sample_index = start + column * WINDOW / WIDTH;
        let normalized = (samples[sample_index] / OUTPUT_LIMIT).clamp(-1.0, 1.0);
        ((1.0 - normalized) * (HEIGHT - 1) as f32 / 2.0).round() as usize
    });
    for (column, row) in plotted_rows.into_iter().enumerate() {
        canvas[row][column] = '*';
    }
    println!("\n{}: 220 Hz -> 880 Hz", mode.name());
    for row in canvas {
        println!("{}", row.iter().collect::<String>());
    }
}

fn run_demo() {
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("demo-output");
    fs::create_dir_all(&output).expect("create demo output directory");
    for mode in TransitionMode::all() {
        let samples = render_demo(mode);
        plot_transition(&samples, mode);
        write_wav(&output.join(format!("{}.wav", mode.name())), &samples).expect("write demo wav");
    }
    println!("\nPlayable files written to {}", output.display());
}

fn main() {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.iter().any(|argument| argument == "--demo") {
        run_demo();
        return;
    }
    let mode = arguments
        .iter()
        .find_map(|argument| TransitionMode::parse(argument))
        .unwrap_or(TransitionMode::Crossfade);
    let outgoing = Arc::new(Ring::<Box<Graph>, QUEUE_SLOTS>::new());
    let retired = Arc::new(Ring::<Box<Graph>, QUEUE_SLOTS>::new());
    let telemetry = Arc::new(Telemetry::new());
    let stop = Arc::new(AtomicBool::new(false));

    let audio_outgoing = Arc::clone(&outgoing);
    let audio_retired = Arc::clone(&retired);
    let audio_telemetry = Arc::clone(&telemetry);
    let audio_stop = Arc::clone(&stop);
    let audio = thread::spawn(move || {
        let initial = Graph::compile(
            0,
            Source {
                gain: 0.1,
                frequency: 110.0,
            },
        );
        let mut state = Some(PlaybackState::Active(initial));
        let mut previous_sample = 0.0;
        while !audio_stop.load(Ordering::Acquire) {
            audio_callback(
                &mut state,
                &audio_outgoing,
                &audio_retired,
                &audio_telemetry,
                &mut previous_sample,
                mode,
            );
            thread::sleep(Duration::from_micros(500));
        }
    });

    let stress_only = arguments.iter().any(|argument| argument == "--stress");
    if stress_only {
        run_stress(&outgoing, &retired, &telemetry);
        show(&telemetry, "stress run complete", false, mode);
    } else {
        let interactive = io::stdin().is_terminal();
        show(&telemetry, "initial graph is audible", interactive, mode);
        for line in io::stdin().lock().lines() {
            let line = line.expect("read source command");
            let line = line.trim();
            if line == "quit" || line == "q" {
                break;
            }
            let message = if line == "stress" {
                run_stress(&outgoing, &retired, &telemetry);
                "stress run complete".to_owned()
            } else {
                match publish(line, &outgoing, &retired, &telemetry) {
                    Ok(revision) => format!("valid candidate revision {revision} queued"),
                    Err(error) => format!("candidate rejected: {error}"),
                }
            };
            thread::sleep(Duration::from_millis(5));
            show(&telemetry, &message, interactive, mode);
        }
    }

    stop.store(true, Ordering::Release);
    audio.join().expect("audio simulation thread");
    while retired.pop().is_some() {}

    assert_eq!(CALLBACK_ALLOCS.load(Ordering::Relaxed), 0);
    assert_eq!(CALLBACK_DEALLOCS.load(Ordering::Relaxed), 0);
    assert!(f32::from_bits(telemetry.peak_bits.load(Ordering::Relaxed)) <= OUTPUT_LIMIT);
}
