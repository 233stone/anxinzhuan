//! 解码音视频文件的第一条音轨，混成单声道并重采样。

use anyhow::{Context, Result};
use rubato::{FftFixedIn, Resampler};
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub fn decode(path: &Path, target_rate: u32) -> Result<Vec<f32>> {
    let file = File::open(path).with_context(|| format!("打不开文件 {}", path.display()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .context("不支持的文件格式")?
        .format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .context("文件里没有音轨")?;
    let track_id = track.id;
    let rate = track.codec_params.sample_rate.context("未知采样率")?;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let mut mono = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder.decode(&packet)?;
        let channels = decoded.spec().channels.count();
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        buffer.copy_interleaved_ref(decoded);
        mono.extend(
            buffer
                .samples()
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
    }
    resample(mono, rate, target_rate)
}

fn resample(samples: Vec<f32>, from: u32, to: u32) -> Result<Vec<f32>> {
    if from == to {
        return Ok(samples);
    }
    let mut resampler = FftFixedIn::<f32>::new(from as usize, to as usize, 1024, 2, 1)?;
    let chunk = resampler.input_frames_next();
    let delay = resampler.output_delay();
    let expected = samples.len() * to as usize / from as usize;
    let mut output = Vec::with_capacity(expected + delay);
    // 末尾补零，把延迟留在缓冲里的样本也推出来。
    for start in (0..).step_by(chunk) {
        if output.len() >= expected + delay {
            break;
        }
        let mut block: Vec<f32> = samples.iter().skip(start).take(chunk).copied().collect();
        block.resize(chunk, 0.0);
        output.extend(resampler.process(&[block], None)?.remove(0));
    }
    output.drain(..delay);
    output.truncate(expected);
    Ok(output)
}
