mod audio;
mod model;
mod sensevoice;

use anyhow::Result;
use std::path::Path;

const SAMPLE_RATE: usize = 16_000;
/// SenseVoice 适合短输入，长音频按不超过 20 秒切块。
const MAX_CHUNK: usize = 20 * SAMPLE_RATE;
/// 在每块最后 5 秒里找最安静的位置下刀，尽量不切断字。
const SEARCH: usize = 5 * SAMPLE_RATE;
const WINDOW: usize = SAMPLE_RATE / 20;

const USAGE: &str = "用法: anxinzhuan <音频或视频文件>";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path] = args.as_slice() else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    if path == "-h" || path == "--help" {
        println!("{USAGE}");
        return Ok(());
    }

    let samples = audio::decode(Path::new(path), SAMPLE_RATE as u32)?;
    let model_dir = model::ensure()?;
    let mut engine = sensevoice::SenseVoice::load(&model_dir)?;

    let chunks = split(&samples);
    let mut text = String::new();
    for (i, chunk) in chunks.iter().enumerate() {
        eprint!("\r识别中 {}/{}", i + 1, chunks.len());
        text.push_str(&engine.recognize(chunk)?);
    }
    eprintln!();
    println!("{}", text.trim());
    Ok(())
}

fn split(mut rest: &[f32]) -> Vec<&[f32]> {
    let mut chunks = Vec::new();
    while rest.len() > MAX_CHUNK {
        let start = MAX_CHUNK - SEARCH;
        let (head, tail) = rest.split_at(start + quietest(&rest[start..MAX_CHUNK]));
        chunks.push(head);
        rest = tail;
    }
    if !rest.is_empty() {
        chunks.push(rest);
    }
    chunks
}

/// 能量最低的窗口中点。
fn quietest(samples: &[f32]) -> usize {
    let energy = |w: &[f32]| w.iter().map(|s| s * s).sum::<f32>();
    samples
        .chunks(WINDOW)
        .enumerate()
        .min_by(|a, b| energy(a.1).total_cmp(&energy(b.1)))
        .map_or(0, |(i, _)| i * WINDOW + WINDOW / 2)
}
