# anxinzhuan

离线语音转文字命令行工具，基于 [SenseVoice-Small](https://modelscope.cn/models/iic/SenseVoiceSmall-onnx)。支持中文、英文、粤语、日语、韩语，自动识别语言，输出带标点的文本。

An offline speech-to-text CLI powered by SenseVoice-Small (zh / en / yue / ja / ko).

## 安装

需要 [Rust](https://rustup.rs)：

```bash
cargo install --git https://github.com/233stone/anxinzhuan
```

## 使用

```bash
anxinzhuan 录音.m4a > 录音.txt
```

文字输出到标准输出，进度输出到标准错误。支持 wav、mp3、m4a、aac、flac、ogg、mp4、mkv 等常见音视频格式，取文件的第一条音轨。

首次运行会从 ModelScope 下载约 240 MB 模型到系统缓存目录（macOS 为 `~/Library/Caches/anxinzhuan`，Linux 为 `~/.cache/anxinzhuan`）。

## 许可证

代码采用 [MIT](LICENSE) 许可证。SenseVoice 模型由阿里巴巴通义实验室发布，使用须遵守其[模型许可协议](https://github.com/FunAudioLLM/SenseVoice/blob/main/MODEL_LICENSE)。
