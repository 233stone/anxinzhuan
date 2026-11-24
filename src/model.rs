//! 首次运行时从 ModelScope 下载 SenseVoice-Small 到缓存目录。

use anyhow::{Context, Result};
use std::fs::{self, File};
use std::path::PathBuf;

const BASE_URL: &str = "https://modelscope.cn/models/iic/SenseVoiceSmall-onnx/resolve/master";
const FILES: [&str; 3] = ["tokens.json", "am.mvn", "model_quant.onnx"];

pub fn ensure() -> Result<PathBuf> {
    let dir = dirs::cache_dir()
        .context("找不到缓存目录")?
        .join("anxinzhuan")
        .join("SenseVoiceSmall-onnx");
    fs::create_dir_all(&dir)?;
    for name in FILES {
        let path = dir.join(name);
        if path.exists() {
            continue;
        }
        eprintln!("下载模型 {name} ...");
        let part = dir.join(format!("{name}.part"));
        let response = ureq::get(format!("{BASE_URL}/{name}")).call()?;
        std::io::copy(
            &mut response.into_body().into_reader(),
            &mut File::create(&part)?,
        )?;
        fs::rename(part, path)?;
    }
    Ok(dir)
}
