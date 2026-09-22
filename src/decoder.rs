//! Декодер на symphonia с исправленным `byte_len`.
//!
//! rodio 0.19 отдаёт `byte_len() -> None` для любых `Read + Seek` источников,
//! из-за чего ридеры контейнеров (mp4, webm) не могут инициализироваться
//! ("Seek errors should not occur during initialization" / "end of stream").
//! Логика декодирования повторяет rodio, но длина источника сообщается честно.

use std::io::{Read, Seek, SeekFrom};
use std::time::Duration;

use rodio::source::Source;
use rodio::decoder::DecoderError;
use symphonia::core::audio::{AudioBufferRef, SampleBuffer, SignalSpec};
use symphonia::core::codecs::{Decoder as Codec, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekedTo, SeekTo};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::{self, Time};
use symphonia::default::{get_codecs, get_probe};

const MAX_DECODE_RETRIES: usize = 3;

/// Обёртка над `Read + Seek`, честно сообщающая длину источника.
struct ReadSeekSource<T: Read + Seek + Send + Sync> {
    inner: T,
    len: Option<u64>,
}

impl<T: Read + Seek + Send + Sync> MediaSource for ReadSeekSource<T> {
    fn is_seekable(&self) -> bool {
        true
    }

    // Главное исправление относительно rodio: настоящая длина, иначе
    // ридеры mp4/webm падают при инициализации.
    fn byte_len(&self) -> Option<u64> {
        self.len
    }
}

impl<T: Read + Seek + Send + Sync> Read for ReadSeekSource<T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<T: Read + Seek + Send + Sync> Seek for ReadSeekSource<T> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

pub struct Decoder {
    decoder: Box<dyn Codec>,
    track_id: u32,
    current_frame_offset: usize,
    format: Box<dyn FormatReader>,
    total_duration: Option<Time>,
    buffer: SampleBuffer<i16>,
    spec: SignalSpec,
}

impl Decoder {
    pub fn new<T: Read + Seek + Send + Sync + 'static>(mut data: T) -> Result<Self, DecoderError> {
        // Длину узнаём до оборачивания, позицию — возвращаем на место
        let len = (|| -> Option<u64> {
            let old = data.stream_position().ok()?;
            let end = data.seek(SeekFrom::End(0)).ok()?;
            data.seek(SeekFrom::Start(old)).ok()?;
            Some(end)
        })();

        let source = ReadSeekSource { inner: data, len };
        let mss = MediaSourceStream::new(Box::new(source), MediaSourceStreamOptions::default());

        match Decoder::init(mss) {
            Err(e) => match e {
                Error::IoError(e) => Err(DecoderError::IoError(e.to_string())),
                Error::DecodeError(e) => Err(DecoderError::DecodeError(e)),
                Error::SeekError(_) => Err(DecoderError::IoError("seek error during init".into())),
                Error::Unsupported(_) => Err(DecoderError::UnrecognizedFormat),
                Error::LimitError(e) => Err(DecoderError::LimitError(e)),
                Error::ResetRequired => Err(DecoderError::ResetRequired),
            },
            Ok(Some(decoder)) => Ok(decoder),
            Ok(None) => Err(DecoderError::NoStreams),
        }
    }

    fn init(mss: MediaSourceStream) -> symphonia::core::errors::Result<Option<Decoder>> {
        let hint = Hint::new();
        let format_opts = FormatOptions { enable_gapless: true, ..Default::default() };
        let metadata_opts: MetadataOptions = Default::default();
        let mut probed = get_probe().format(&hint, mss, &format_opts, &metadata_opts)?;

        // В muxed-контейнере (Android отдаёт видео+аудио) нужен именно
        // аудиотрек — видео-дорожку symphonia не раскодирует.
        let track = probed
            .format
            .tracks()
            .iter()
            .find(|t| {
                t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some()
            })
            .or_else(|| probed.format.default_track())
            .ok_or(Error::Unsupported("No track with supported codec"))?;

        let track_id = track.id;

        let mut decoder = get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
        let total_duration = track
            .codec_params
            .time_base
            .zip(track.codec_params.n_frames)
            .map(|(base, frames)| base.calc_time(frames));

        let mut decode_errors: usize = 0;
        let decoded = loop {
            let current_frame = match probed.format.next_packet() {
                Ok(packet) => packet,
                Err(Error::IoError(_)) => break decoder.last_decoded(),
                Err(e) => return Err(e),
            };

            if current_frame.track_id() != track_id {
                continue;
            }

            match decoder.decode(&current_frame) {
                Ok(decoded) => break decoded,
                Err(e) => match e {
                    Error::DecodeError(_) => {
                        decode_errors += 1;
                        if decode_errors > MAX_DECODE_RETRIES {
                            return Err(e);
                        } else {
                            continue;
                        }
                    }
                    _ => return Err(e),
                },
            }
        };
        let spec = decoded.spec().to_owned();
        let buffer = Decoder::get_buffer(decoded, &spec);
        Ok(Some(Decoder {
            decoder,
            track_id,
            current_frame_offset: 0,
            format: probed.format,
            total_duration,
            buffer,
            spec,
        }))
    }

    fn get_buffer(decoded: AudioBufferRef, spec: &SignalSpec) -> SampleBuffer<i16> {
        let duration = units::Duration::from(decoded.capacity() as u64);
        let mut buffer = SampleBuffer::<i16>::new(duration, *spec);
        buffer.copy_interleaved_ref(decoded);
        buffer
    }

    fn refine_position(&mut self, seek_res: SeekedTo) -> Result<(), rodio::source::SeekError> {
        let mut samples_to_pass = seek_res.required_ts - seek_res.actual_ts;
        let packet = loop {
            let candidate = self.format.next_packet().map_err(seek_err)?;
            if candidate.track_id() != self.track_id {
                continue;
            }
            if candidate.dur() > samples_to_pass {
                break candidate;
            } else {
                samples_to_pass -= candidate.dur();
            }
        };

        let mut decoded = self.decoder.decode(&packet);
        for _ in 0..MAX_DECODE_RETRIES {
            if decoded.is_err() {
                let packet = self
                    .format
                    .next_packet()
                    .map_err(seek_err)?;
                decoded = self.decoder.decode(&packet);
            }
        }

        let decoded = decoded.map_err(seek_err)?;
        decoded.spec().clone_into(&mut self.spec);
        self.buffer = Decoder::get_buffer(decoded, &self.spec);
        self.current_frame_offset = samples_to_pass as usize * self.channels() as usize;
        Ok(())
    }
}

impl Source for Decoder {
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        Some(self.buffer.samples().len())
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.spec.channels.count() as u16
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.spec.rate
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
            .map(|Time { seconds, frac }| Duration::new(seconds, (1f64 / frac) as u32))
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        let seek_beyond_end = self
            .total_duration()
            .is_some_and(|dur| dur.saturating_sub(pos).as_millis() < 1);

        let time = if seek_beyond_end {
            let time = self.total_duration.expect("if guarantees this is Some");
            skip_back_a_tiny_bit(time)
        } else {
            pos.as_secs_f64().into()
        };

        let to_skip = self.current_frame_offset % self.channels() as usize;

        let seek_res = self
            .format
            .seek(SeekMode::Accurate, SeekTo::Time { time, track_id: None })
            .map_err(seek_err)?;

        self.refine_position(seek_res)?;
        self.current_frame_offset += to_skip;

        Ok(())
    }
}

impl Iterator for Decoder {
    type Item = i16;

    #[inline]
    fn next(&mut self) -> Option<i16> {
        if self.current_frame_offset >= self.buffer.len() {
            let packet = loop {
                let packet = self.format.next_packet().ok()?;
                if packet.track_id() == self.track_id {
                    break packet;
                }
            };
            let mut decoded = self.decoder.decode(&packet);
            for _ in 0..MAX_DECODE_RETRIES {
                if decoded.is_err() {
                    let packet = self.format.next_packet().ok()?;
                    decoded = self.decoder.decode(&packet);
                }
            }
            let decoded = decoded.ok()?;
            decoded.spec().clone_into(&mut self.spec);
            self.buffer = Decoder::get_buffer(decoded, &self.spec);
            self.current_frame_offset = 0;
        }

        let sample = *self.buffer.samples().get(self.current_frame_offset)?;
        self.current_frame_offset += 1;

        Some(sample)
    }
}

fn seek_err(e: symphonia::core::errors::Error) -> rodio::source::SeekError {
    rodio::source::SeekError::Other(Box::new(e))
}

fn skip_back_a_tiny_bit(Time { mut seconds, mut frac }: Time) -> Time {
    frac -= 0.0001;
    if frac < 0.0 {
        seconds = seconds.saturating_sub(1);
        frac = 1.0 - frac;
    }
    Time { seconds, frac }
}
