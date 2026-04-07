//! Kokoro TTS worker: synthesizes from the current reading position in a background thread.

use crate::epub::Chapter;
use kokoro_tiny::TtsEngine;
use rodio::{buffer::SamplesBuffer, OutputStream, Sink};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
const SPEED_MIN: f32 = 0.5;
const SPEED_MAX: f32 = 2.0;
const SPEED_STEP: f32 = 0.1;
const DEFAULT_SPEED: f32 = 0.9;
/// Kokoro voice id at startup (before `[` / `]` cycling).
const DEFAULT_VOICE_ID: &str = "af_nova";

/// Output volume (0.0–1.0) on the shared `Sink`.
const PLAY_VOLUME: f32 = 0.85;
/// Kokoro outputs mono f32 PCM at 24 kHz.
const KOKORO_SAMPLE_RATE: u32 = 24_000;
/// Poll interval while waiting for the current clip to finish (command drain responsiveness).
const DRAIN_POLL_MS: u64 = 20;
/// Abort synthesis if it hasn't returned within this many seconds.
const SYNTH_TIMEOUT_SECS: u64 = 30;

#[derive(Debug)]
pub enum TtsCommand {
    TogglePause,
    SpeedUp,
    SpeedDown,
    VoiceNext,
    VoicePrev,
    Stop,
}

#[derive(Clone)]
pub struct TtsDisplayState {
    /// Full text of the segment currently playing (UI truncates).
    pub seg_current: String,
    /// Full text of the following segment, if any.
    pub seg_next: String,
    /// Kokoro voice id, or `"default"` for engine default.
    pub voice_name: String,
    pub status: String,
    pub speed: f32,
    pub paused: bool,
}

impl Default for TtsDisplayState {
    fn default() -> Self {
        Self {
            seg_current: String::new(),
            seg_next: String::new(),
            voice_name: DEFAULT_VOICE_ID.into(),
            status: String::new(),
            speed: DEFAULT_SPEED,
            paused: false,
        }
    }
}

pub struct TtsSession {
    pub cmd_tx: Sender<TtsCommand>,
    pub state: Arc<Mutex<TtsDisplayState>>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl TtsSession {
    pub fn join_worker(&self) {
        let h = self.join.lock().unwrap().take();
        if let Some(h) = h {
            let _ = h.join();
        }
    }
}

struct Segment {
    text: String,
}

fn normalize_chunk(s: &str) -> String {
    s.chars()
        .map(|ch| match ch {
            '\n' | '\r' | '\t' => ' ',
            _ => ch,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn last_alnum_token_lower(s: &str) -> Option<String> {
    let t = s.split_whitespace().last()?.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '\'');
    if t.is_empty() {
        return None;
    }
    Some(
        t.trim_end_matches('.')
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '\'')
            .flat_map(|c| c.to_lowercase())
            .collect(),
    )
}

/// Avoid splitting after common English abbreviations.
fn dot_is_sentence_end(rest: &str, dot_byte_end: usize) -> bool {
    let before = rest.get(..dot_byte_end.saturating_sub(1)).unwrap_or("");
    let Some(tok) = last_alnum_token_lower(before) else {
        return true;
    };
    !matches!(
        tok.as_str(),
        "mr"
            | "mrs"
            | "ms"
            | "dr"
            | "prof"
            | "sr"
            | "jr"
            | "vs"
            | "st"
            | "etc"
            | "al"
            | "eg"
            | "ie"
            | "viz"
            | "cf"
            | "ed"
            | "eds"
            | "vol"
            | "no"
    )
}

fn byte_after_n_chars(s: &str, n: usize) -> usize {
    s.char_indices()
        .nth(n)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

fn char_count_prefix(s: &str) -> usize {
    s.chars().count()
}

fn word_split_before(s: &str, max_byte: usize) -> usize {
    let max_byte = max_byte.min(s.len());
    if max_byte == 0 {
        return s.chars().next().map(|c| c.len_utf8()).unwrap_or(0).min(s.len());
    }
    const MIN_SPACE: usize = 20;
    let head = &s[..max_byte];
    if let Some(sp) = head.rfind(' ') {
        if sp >= MIN_SPACE {
            return sp + 1;
        }
    }
    max_byte
}

/// Exclusive end offset in `rest` for the next segment.
/// Sentence boundaries (`.?!` and full-width variants) end a segment when the text so far is
/// long enough, or there is nothing left to merge with; otherwise the next sentence is included.
/// Long unpunctuated spans split at a word boundary near `MAX_CHARS`.
fn next_segment_end(rest: &str) -> usize {
    const MAX_CHARS: usize = 280;
    const SOFT_SPLIT_CHARS: usize = 260;
    /// Merge `"Hi."`-style fragments with the following sentence so TTS is less choppy.
    const MIN_SEGMENT_CHARS: usize = 36;

    if rest.is_empty() {
        return 0;
    }

    let mut n_chars = 0_usize;

    for (i, c) in rest.char_indices() {
        let pos_after = i + c.len_utf8();
        n_chars += 1;

        let mut end_here: Option<usize> = None;
        match c {
            '?' | '!' | '。' | '！' | '？' => end_here = Some(pos_after),
            '.' => {
                let after_ch = rest[pos_after..].chars().next();
                let ok = match after_ch {
                    None => true,
                    Some(x) if x.is_whitespace() => dot_is_sentence_end(rest, pos_after),
                    Some('"' | '\'' | ')' | ']' | '”' | '»' | '…') => {
                        dot_is_sentence_end(rest, pos_after)
                    }
                    _ => false,
                };
                if ok {
                    end_here = Some(pos_after);
                }
            }
            _ => {}
        }
        if let Some(b) = end_here {
            let head = rest.get(..b).unwrap_or(rest);
            let head_chars = char_count_prefix(head);
            let no_more_text = b >= rest.len();
            if head_chars >= MIN_SEGMENT_CHARS || no_more_text {
                return b.max(1).min(rest.len());
            }
        }

        if n_chars >= MAX_CHARS {
            let soft_b = byte_after_n_chars(rest, SOFT_SPLIT_CHARS);
            let cut = word_split_before(rest, soft_b);
            return cut.max(1).min(rest.len());
        }
    }

    rest.len().max(1).min(rest.len())
}

/// Split chapter text into speakable chunks. `rest` is a slice of `chapter.text`; `abs_byte` is
/// that slice's start offset in `chapter.text`.
fn split_chapter_text(chapter: usize, mut rest: &str, mut abs_byte: usize) -> Vec<Segment> {
    let mut out = Vec::new();

    loop {
        let trimmed = rest.trim_start();
        let skipped = rest.len().saturating_sub(trimmed.len());
        abs_byte += skipped;
        rest = trimmed;
        if rest.is_empty() {
            break;
        }

        let end_rel = next_segment_end(rest);
        let end_rel = end_rel.max(1).min(rest.len());
        let slice = rest.get(..end_rel).unwrap_or(rest).trim();
        if !slice.is_empty() {
            let normalized = normalize_chunk(slice);
            if !normalized.is_empty() {
                out.push(Segment { text: normalized });
            }
        }
        rest = rest.get(end_rel..).unwrap_or("");
        abs_byte += end_rel;
    }
    out
}

fn build_segments(chapters: &[Chapter], start_chapter: usize, start_line: usize) -> Vec<Segment> {
    let mut segments = Vec::new();
    for ch in start_chapter..chapters.len() {
        let c = &chapters[ch];
        let start_byte = if ch == start_chapter {
            c.lines.get(start_line).map(|x| x.0).unwrap_or(0)
        } else {
            0
        };
        if start_byte >= c.text.len() {
            continue;
        }
        let slice = &c.text[start_byte..];
        segments.extend(split_chapter_text(ch, slice, start_byte));
    }
    segments
}

fn voice_synth_arg<'a>(voices: &'a [String], pick: &Mutex<Option<usize>>) -> Option<&'a str> {
    let idx = *pick.lock().unwrap();
    idx.and_then(|i| voices.get(i).map(|s| s.as_str()))
}

fn voice_for_synth_job(voices: &[String], pick: &Mutex<Option<usize>>) -> Option<String> {
    voice_synth_arg(voices, pick).map(str::to_string)
}

fn voice_next(voices: &[String], pick: &Mutex<Option<usize>>, state: &Arc<Mutex<TtsDisplayState>>) {
    if voices.is_empty() {
        return;
    }
    let n = voices.len();
    let new_idx = {
        let mut idx = pick.lock().unwrap();
        *idx = match *idx {
            None => Some(0),
            Some(i) if i + 1 < n => Some(i + 1),
            Some(_) => None,
        };
        *idx
    };
    let label = match new_idx {
        None => "default".into(),
        Some(i) => voices
            .get(i)
            .cloned()
            .unwrap_or_else(|| "default".into()),
    };
    if let Ok(mut g) = state.lock() {
        g.voice_name = label;
    }
}

fn voice_prev(voices: &[String], pick: &Mutex<Option<usize>>, state: &Arc<Mutex<TtsDisplayState>>) {
    if voices.is_empty() {
        return;
    }
    let n = voices.len();
    let new_idx = {
        let mut idx = pick.lock().unwrap();
        *idx = match *idx {
            None => Some(n.saturating_sub(1)),
            Some(0) => None,
            Some(i) => Some(i - 1),
        };
        *idx
    };
    let label = match new_idx {
        None => "default".into(),
        Some(i) => voices
            .get(i)
            .cloned()
            .unwrap_or_else(|| "default".into()),
    };
    if let Ok(mut g) = state.lock() {
        g.voice_name = label;
    }
}

fn apply_playhead_ui(
    state: &Arc<Mutex<TtsDisplayState>>,
    segments: &[Segment],
    play_index: usize,
    speed_holder: &Arc<Mutex<f32>>,
    paused_holder: &Arc<Mutex<bool>>,
    status: &str,
) {
    let cur = segments
        .get(play_index)
        .map(|s| s.text.as_str())
        .unwrap_or("");
    let nxt = segments
        .get(play_index + 1)
        .map(|s| s.text.as_str())
        .unwrap_or("");
    if let Ok(mut g) = state.lock() {
        g.seg_current = cur.to_string();
        g.seg_next = nxt.to_string();
        g.status = status.to_string();
        g.paused = *paused_holder.lock().unwrap();
        g.speed = *speed_holder.lock().unwrap();
    }
}

pub fn spawn_session(chapters: Arc<Vec<Chapter>>, start_chapter: usize, start_line: usize) -> TtsSession {
    let segments = build_segments(&chapters, start_chapter, start_line);
    let (cmd_tx, cmd_rx) = mpsc::channel::<TtsCommand>();
    let state = Arc::new(Mutex::new(TtsDisplayState {
        status: "Starting…".into(),
        ..Default::default()
    }));
    let state_w = Arc::clone(&state);
    let speed = Arc::new(Mutex::new(DEFAULT_SPEED));
    let paused = Arc::new(Mutex::new(false));
    let voice_pick = Arc::new(Mutex::new(None::<usize>));

    let join = std::thread::spawn(move || {
        run_worker(
            segments,
            cmd_rx,
            state_w,
            Arc::clone(&speed),
            Arc::clone(&paused),
            Arc::clone(&voice_pick),
        );
    });

    TtsSession {
        cmd_tx,
        state,
        join: Mutex::new(Some(join)),
    }
}

fn sync_sink_speed(sink: &Sink, speed_holder: &Arc<Mutex<f32>>) {
    if let Ok(sp) = speed_holder.lock() {
        sink.set_speed((*sp).clamp(SPEED_MIN, SPEED_MAX));
    }
}

/// Drain pending commands from `cmd_rx`.
/// If `block_while_paused` is true, keeps sleeping (50 ms) until unpaused or stopped.
/// Returns `false` if `Stop` was received (sink cleared, status set to "Stopped").
fn drain_commands(
    cmd_rx: &Receiver<TtsCommand>,
    state: &Arc<Mutex<TtsDisplayState>>,
    speed_holder: &Arc<Mutex<f32>>,
    paused_holder: &Arc<Mutex<bool>>,
    voices: &[String],
    voice_pick: &Arc<Mutex<Option<usize>>>,
    sink: Option<&Sink>,
    block_while_paused: bool,
) -> bool {
    loop {
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                TtsCommand::Stop => {
                    if let Some(s) = sink {
                        s.clear();
                    }
                    if let Ok(mut g) = state.lock() {
                        g.status = "Stopped".into();
                    }
                    return false;
                }
                TtsCommand::TogglePause => {
                    let mut p = paused_holder.lock().unwrap();
                    *p = !*p;
                    if let Some(s) = sink {
                        if *p {
                            s.pause();
                        } else {
                            s.play();
                        }
                    }
                    if let Ok(mut g) = state.lock() {
                        g.paused = *p;
                        g.status = if *p { "Paused".into() } else { "Playing".into() };
                    }
                }
                TtsCommand::SpeedUp | TtsCommand::SpeedDown => {
                    let mut sp = speed_holder.lock().unwrap();
                    if matches!(cmd, TtsCommand::SpeedUp) {
                        *sp = (*sp + SPEED_STEP).min(SPEED_MAX);
                    } else {
                        *sp = (*sp - SPEED_STEP).max(SPEED_MIN);
                    }
                    if let Some(s) = sink {
                        s.set_speed(*sp);
                    }
                    if let Ok(mut g) = state.lock() {
                        g.speed = *sp;
                    }
                }
                TtsCommand::VoiceNext => voice_next(voices, voice_pick, state),
                TtsCommand::VoicePrev => voice_prev(voices, voice_pick, state),
            }
        }
        if !block_while_paused || !*paused_holder.lock().unwrap() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn run_worker(
    segments: Vec<Segment>,
    cmd_rx: Receiver<TtsCommand>,
    state: Arc<Mutex<TtsDisplayState>>,
    speed_holder: Arc<Mutex<f32>>,
    paused_holder: Arc<Mutex<bool>>,
    voice_pick: Arc<Mutex<Option<usize>>>,
) {
    if let Ok(mut g) = state.lock() {
        g.status = "Loading TTS model…".into();
    }

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            if let Ok(mut g) = state.lock() {
                g.status = format!("Tokio init error: {}", e);
            }
            return;
        }
    };

    let tts = match rt.block_on(TtsEngine::new()) {
        Ok(e) => e,
        Err(e) => {
            if let Ok(mut g) = state.lock() {
                g.status = format!("TTS init: {}", e);
            }
            return;
        }
    };

    if segments.is_empty() {
        if let Ok(mut g) = state.lock() {
            g.status = "No text to read".into();
        }
        return;
    }

    let mut voices = tts.voices();
    voices.sort();

    {
        let initial = voices
            .iter()
            .position(|v| v == DEFAULT_VOICE_ID)
            .or_else(|| voices.iter().position(|v| v.eq_ignore_ascii_case(DEFAULT_VOICE_ID)));
        if let Some(i) = initial {
            *voice_pick.lock().unwrap() = Some(i);
            if let Ok(mut g) = state.lock() {
                g.voice_name = voices[i].clone();
            }
        } else if let Ok(mut g) = state.lock() {
            g.voice_name = "default".into();
        }
    }

    let tts = Arc::new(Mutex::new(tts));
    let (job_tx, job_rx) = mpsc::sync_channel::<(String, Option<String>)>(1);
    let (pcm_tx, pcm_rx) = mpsc::sync_channel::<Result<Vec<f32>, String>>(1);
    let tts_worker = Arc::clone(&tts);
    std::thread::spawn(move || {
        while let Ok((text, voice)) = job_rx.recv() {
            let voice_ref = voice.as_deref();
            let result = match tts_worker.lock() {
                Ok(mut engine) => engine.synthesize(&text, voice_ref),
                Err(e) => Err(format!("TTS lock poisoned: {}", e)),
            };
            if pcm_tx.send(result).is_err() {
                break;
            }
        }
    });

    let synth_send = |text: String, voice: Option<String>| -> Result<(), String> {
        job_tx
            .send((text, voice))
            .map_err(|_| "Synthesis worker disconnected".into())
    };

    let synth_recv = || -> Result<Vec<f32>, String> {
        match pcm_rx.recv_timeout(Duration::from_secs(SYNTH_TIMEOUT_SECS)) {
            Ok(Ok(pcm)) => Ok(pcm),
            Ok(Err(e)) => Err(e),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err(format!("Synthesis timed out ({}s)", SYNTH_TIMEOUT_SECS))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("Synthesis worker disconnected".into())
            }
        }
    };

    if let Err(e) = synth_send(
        segments[0].text.clone(),
        voice_for_synth_job(&voices, voice_pick.as_ref()),
    ) {
        if let Ok(mut g) = state.lock() {
            g.status = e;
        }
        return;
    }

    let mut pcm_cur: Option<Vec<f32>> = Some(match synth_recv() {
        Ok(p) => p,
        Err(e) => {
            if let Ok(mut g) = state.lock() {
                g.status = format!("Synthesize: {}", e);
            }
            return;
        }
    });

    // One `OutputStream` / `Sink` for the whole session so the device is not reopened per
    // segment (that caused audible gaps). Commands are drained while each clip plays.
    let (_stream, stream_handle) = match OutputStream::try_default() {
        Ok(h) => h,
        Err(e) => {
            if let Ok(mut g) = state.lock() {
                g.status = format!("Audio output: {}", e);
            }
            return;
        }
    };

    let sink = match Sink::try_new(&stream_handle) {
        Ok(s) => s,
        Err(e) => {
            if let Ok(mut g) = state.lock() {
                g.status = format!("Audio sink: {}", e);
            }
            return;
        }
    };

    sink.set_volume(PLAY_VOLUME.clamp(0.0, 1.0));
    sync_sink_speed(&sink, &speed_holder);

    let n = segments.len();
    for play_index in 0..n {
        if !drain_commands(
            &cmd_rx,
            &state,
            &speed_holder,
            &paused_holder,
            &voices,
            &voice_pick,
            Some(&sink),
            true,
        ) {
            return;
        }

        apply_playhead_ui(
            &state,
            &segments,
            play_index,
            &speed_holder,
            &paused_holder,
            "Playing",
        );

        sync_sink_speed(&sink, &speed_holder);

        // Queue the next segment's synthesis before this clip ends so playback stays continuous
        // (avoids device underrun / clipped onsets between segments).
        if play_index + 1 < n {
            if let Err(e) = synth_send(
                segments[play_index + 1].text.clone(),
                voice_for_synth_job(&voices, voice_pick.as_ref()),
            ) {
                sink.clear();
                if let Ok(mut g) = state.lock() {
                    g.status = e;
                }
                return;
            }
        }

        let pcm = match pcm_cur.take() {
            Some(p) => p,
            None => {
                if let Ok(mut g) = state.lock() {
                    g.status = "Internal error: PCM buffer missing".into();
                }
                return;
            }
        };
        // Prime the audio device on the first segment to avoid clipped onsets.
        if play_index == 0 {
            let silence_len = (KOKORO_SAMPLE_RATE / 3) as usize; // 333 ms
            sink.append(SamplesBuffer::new(1, KOKORO_SAMPLE_RATE, vec![0.0f32; silence_len]));
        }
        sink.append(SamplesBuffer::new(1, KOKORO_SAMPLE_RATE, pcm));

        loop {
            if !drain_commands(
                &cmd_rx,
                &state,
                &speed_holder,
                &paused_holder,
                &voices,
                &voice_pick,
                Some(&sink),
                false,
            ) {
                return;
            }
            if sink.empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(DRAIN_POLL_MS));
        }

        if play_index + 1 < n {
            match synth_recv() {
                Ok(p) => pcm_cur = Some(p),
                Err(e) => {
                    sink.clear();
                    if let Ok(mut g) = state.lock() {
                        g.status = format!("Synthesize: {}", e);
                    }
                    return;
                }
            }
        }
    }

    if let Ok(mut g) = state.lock() {
        g.status = "Finished".into();
        g.seg_next.clear();
        g.seg_current.clear();
    }
}
