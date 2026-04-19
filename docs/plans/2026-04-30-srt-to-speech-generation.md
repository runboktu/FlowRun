# SRT 字幕转语音方案

## 目标

根据 SRT 字幕文件生成语音，语音时间轴与 SRT 时间戳严格对齐。

## 核心思路

解析 SRT → 按时间戳逐段 TTS 生成 → 时长对齐（短则补静音，长则加速）→ 按时间轴拼接。

```
SRT文件
  ↓ 解析
[{start_ms: 1000, end_ms: 4000, text: "你好世界"}, ...]
  ↓ TTS生成 + 时长对齐
audio_segment_1 (pad to 3.0s)
audio_segment_2 (pad to 2.5s)
  ↓ FFmpeg concat
final_output.mp3/wav
```

## 详细流程

### 1. SRT 解析

提取每条字幕的四个字段：

```
序号
start_time --> end_time
文本内容
(空行分隔下一条)
```

时间戳格式：`HH:MM:SS,mmm` → 统一转为毫秒，便于计算。

### 2. 逐段 TTS 生成

为每条字幕文本调用 TTS 引擎生成音频：

- **Edge-TTS**（推荐）：微软免费 TTS，中文自然，API 简洁
- **OpenAI TTS**：质量高，需 API Key
- **ChatTTS / CosyVoice**：本地模型，可控性高

### 3. 时长对齐

这是时间轴同步的关键步骤。

```
target_duration = (end_ms - start_ms) / 1000.0

if actual_duration < target_duration:
    # 短了 → 尾部补静音
    silence = AudioSegment.silent(duration=(target_duration - actual_duration) * 1000)
    result = audio + silence

elif actual_duration > target_duration:
    # 长了 → 变速拉伸（±10%以内人耳不可察觉）
    speed_factor = target_duration / actual_duration
    result = audio._spawn(audio.raw_data, overrides={
        "frame_rate": int(audio.frame_rate * speed_factor)
    })
```

- **短于预期**：尾部补静音对齐
- **长于预期**：小幅加速拉伸（限 ±10%，超出则说明 SRT 时间轴不合理，需告警）

### 4. 时间轴定位

**关键问题**：SRT 字幕之间可能有时间间隔（静默期）。

最终音频生成逻辑：

```
cursor = 0
final = AudioSegment.empty()

for entry in srt_entries:
    # 先填入从 cursor 到 start_time 的静音（如果字幕间有空隙）
    if entry.start_ms > cursor:
        silence = AudioSegment.silent(duration=entry.start_ms - cursor)
        final += silence

    # 放入对齐后的字幕音频
    final += aligned_audio
    cursor = entry.start_ms + entry_duration_ms  # 实际对齐后的时长
```

### 5. 音频输出

- 默认 WAV 格式（无损）
- 可用 FFmpeg 转码为 MP3 / AAC

## 工具链对比

| 方案 | TTS 引擎 | 中文质量 | 成本 | 推荐场景 |
|---|---|---|---|---|
| **Edge-TTS** | 微软 TTS | ⭐⭐⭐⭐⭐ | 免费 | 通用首选 |
| **OpenAI TTS** | OpenAI TTS | ⭐⭐⭐⭐ | 按 token 收费 | 英文高质量 |
| **ChatTTS** | 本地模型 | ⭐⭐⭐⭐ | 免费（需 GPU） | 本地离线 |
| **CosyVoice** | 阿里通义 | ⭐⭐⭐⭐⭐ | 免费（需 GPU） | 中文高质量 |

## 依赖

```
edge-tts         # 或 openai / chattts
pysrt            # SRT 解析
pydub            # 音频操作 + 时长对齐
ffmpeg           # 格式转换
```

## 边界情况处理

- **空白字幕**：跳过，仅填充静音
- **重叠时间戳**：按 SRT 序号优先级截断或告警
- **极短字幕**（< 300ms）：TTS 可能无法生成足够短的音频，直接使用变速压缩
- **超长文本**（> 50 字）：可能需调整语速或分段
- **HTML 标签**（`<i>`, `<font>`）：清理后保留纯文本

## 扩展方向

- 多说话人：按 SRT 中的说话人标签切换不同音色
- 背景音混音：在静默期填入 BGM
- 字幕样式（粗体/斜体）：对应调整 TTS 的 SSML 标记
