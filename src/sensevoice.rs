//! SenseVoice-Small ONNX 推理：Fbank → LFR + CMVN → 模型 → CTC 贪心解码。
//! 前处理参数取自模型的 config.yaml（16 kHz、80 维 Fbank、LFR m=7 n=6）。

use anyhow::{Context, Result};
use ort::session::Session;
use ort::value::Tensor;
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::path::Path;
use std::sync::Arc;

const SAMPLE_RATE: f32 = 16_000.0;
const FRAME_LEN: usize = 400;
const FRAME_SHIFT: usize = 160;
const FFT_SIZE: usize = 512;
const N_MELS: usize = 80;
const LFR_M: usize = 7;
const LFR_N: usize = 6;
/// 自动识别语言。
const LANGUAGE_AUTO: i32 = 0;
/// 输出标点并做逆文本正则化（数字等写成阿拉伯数字）。
const TEXTNORM_WITH_ITN: i32 = 14;
/// 输出开头的语言、情感、事件、ITN 四个标签帧。
const PROMPT_FRAMES: usize = 4;

pub struct SenseVoice {
    session: Session,
    tokens: Vec<String>,
    cmvn_shift: Vec<f32>,
    cmvn_scale: Vec<f32>,
    mel_bank: Vec<Vec<f32>>,
    window: Vec<f32>,
    fft: Arc<dyn Fft<f32>>,
}

impl SenseVoice {
    pub fn load(dir: &Path) -> Result<Self> {
        let tokens = serde_json::from_str(&std::fs::read_to_string(dir.join("tokens.json"))?)?;
        let (cmvn_shift, cmvn_scale) = load_cmvn(&std::fs::read_to_string(dir.join("am.mvn"))?)?;
        let session = Session::builder()?.commit_from_file(dir.join("model_quant.onnx"))?;
        let window = (0..FRAME_LEN)
            .map(|n| {
                0.54 - 0.46 * (2.0 * std::f32::consts::PI * n as f32 / (FRAME_LEN - 1) as f32).cos()
            })
            .collect();
        Ok(Self {
            session,
            tokens,
            cmvn_shift,
            cmvn_scale,
            mel_bank: mel_bank(),
            window,
            fft: FftPlanner::new().plan_fft_forward(FFT_SIZE),
        })
    }

    pub fn recognize(&mut self, samples: &[f32]) -> Result<String> {
        let fbank = self.fbank(samples);
        if fbank.is_empty() {
            return Ok(String::new());
        }
        let feats = self.lfr_cmvn(&fbank);
        let frames = feats.len() / (N_MELS * LFR_M);
        let scalar = |v: i32| Tensor::from_array((vec![1i64], vec![v]));
        let outputs = self.session.run(ort::inputs![
            "speech" => Tensor::from_array((vec![1i64, frames as i64, (N_MELS * LFR_M) as i64], feats))?,
            "speech_lengths" => scalar(frames as i32)?,
            "language" => scalar(LANGUAGE_AUTO)?,
            "textnorm" => scalar(TEXTNORM_WITH_ITN)?,
        ])?;
        let (shape, logits) = outputs["ctc_logits"].try_extract_tensor::<f32>()?;
        let vocab = shape[2] as usize;

        let mut text = String::new();
        let mut prev = 0;
        for row in logits.chunks_exact(vocab).skip(PROMPT_FRAMES) {
            let id = argmax(row);
            if id != 0 && id != prev {
                let piece = &self.tokens[id];
                if !piece.starts_with("<|") {
                    text.push_str(&piece.replace('\u{2581}', " "));
                }
            }
            prev = id;
        }
        Ok(text)
    }

    /// Kaldi 风格 Fbank：逐帧去直流、预加重、汉明窗、功率谱、对数梅尔能量。
    fn fbank(&self, samples: &[f32]) -> Vec<Vec<f32>> {
        if samples.len() < FRAME_LEN {
            return Vec::new();
        }
        let mut buffer = vec![Complex::new(0.0, 0.0); FFT_SIZE];
        samples
            .windows(FRAME_LEN)
            .step_by(FRAME_SHIFT)
            .map(|frame| {
                let mut x: Vec<f32> = frame.iter().map(|s| s * 32768.0).collect();
                let mean = x.iter().sum::<f32>() / FRAME_LEN as f32;
                x.iter_mut().for_each(|v| *v -= mean);
                for i in (1..FRAME_LEN).rev() {
                    x[i] -= 0.97 * x[i - 1];
                }
                x[0] -= 0.97 * x[0];
                buffer.fill(Complex::new(0.0, 0.0));
                for ((b, v), w) in buffer.iter_mut().zip(&x).zip(&self.window) {
                    b.re = v * w;
                }
                self.fft.process(&mut buffer);
                let power: Vec<f32> = buffer[..FFT_SIZE / 2]
                    .iter()
                    .map(|c| c.norm_sqr())
                    .collect();
                self.mel_bank
                    .iter()
                    .map(|filter| {
                        let energy: f32 = filter.iter().zip(&power).map(|(w, p)| w * p).sum();
                        energy.max(f32::EPSILON).ln()
                    })
                    .collect()
            })
            .collect()
    }

    /// 每 6 帧取一次、拼接前后共 7 帧（首尾复制填充），再做 CMVN。按行展平。
    fn lfr_cmvn(&self, fbank: &[Vec<f32>]) -> Vec<f32> {
        let frames = fbank.len().div_ceil(LFR_N);
        let left = (LFR_M - 1) / 2;
        let mut out = Vec::with_capacity(frames * LFR_M * N_MELS);
        for i in 0..frames {
            for j in 0..LFR_M {
                let index = (i * LFR_N + j).saturating_sub(left).min(fbank.len() - 1);
                out.extend_from_slice(&fbank[index]);
            }
        }
        for row in out.chunks_exact_mut(LFR_M * N_MELS) {
            for ((v, shift), scale) in row.iter_mut().zip(&self.cmvn_shift).zip(&self.cmvn_scale) {
                *v = (*v + shift) * scale;
            }
        }
        out
    }
}

/// HTK 梅尔刻度三角滤波器，20 Hz 到奈奎斯特频率。
fn mel_bank() -> Vec<Vec<f32>> {
    let mel = |hz: f32| 1127.0 * (1.0 + hz / 700.0).ln();
    let (low, high) = (mel(20.0), mel(SAMPLE_RATE / 2.0));
    let delta = (high - low) / (N_MELS + 1) as f32;
    (0..N_MELS)
        .map(|m| {
            let (left, center, right) = (
                low + m as f32 * delta,
                low + (m + 1) as f32 * delta,
                low + (m + 2) as f32 * delta,
            );
            (0..FFT_SIZE / 2)
                .map(|bin| {
                    let f = mel(bin as f32 * SAMPLE_RATE / FFT_SIZE as f32);
                    if f <= left || f >= right {
                        0.0
                    } else if f <= center {
                        (f - left) / (center - left)
                    } else {
                        (right - f) / (right - center)
                    }
                })
                .collect()
        })
        .collect()
}

/// am.mvn 里依次是 AddShift 和 Rescale 两行向量。
fn load_cmvn(content: &str) -> Result<(Vec<f32>, Vec<f32>)> {
    let mut vectors = content
        .lines()
        .filter(|line| line.contains("<LearnRateCoef>"))
        .map(|line| -> Result<Vec<f32>> {
            let values = line
                .split_once('[')
                .and_then(|(_, rest)| rest.split_once(']'))
                .context("am.mvn 格式错误")?
                .0;
            Ok(values
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()?)
        });
    let shift = vectors.next().context("am.mvn 格式错误")??;
    let scale = vectors.next().context("am.mvn 格式错误")??;
    Ok((shift, scale))
}

fn argmax(row: &[f32]) -> usize {
    row.iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map_or(0, |(i, _)| i)
}
