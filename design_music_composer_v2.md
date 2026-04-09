# music_composer 工作流 v2 改造方案

## 1. 改造结果

| 项目 | 改造前 | 改造后 |
|---|---|---|
| `19_music_composer.rs` | 硬编码歌词的 example | CLI 工具（`--lyrics` 文件路径 / `--style` / `--output-dir`） |
| `19_music_composer.yaml` | 3 Agent + 1 Shell（4步） | 2 Agent + 1 Shell（3步） |
| 英文提示词质量控制 | 仅 prompt 约束 | Agent 工具辅助（check_char_count + strip_whitespace） |

## 2. 架构设计

### 执行流程

```
CLI 参数解析（--lyrics / --style / --output-dir）
  ↓
解析 YAML → DAG 拓扑排序
  ↓
创建 ToolRegistry（注册 check_char_count / strip_whitespace）
  ↓
创建 AgentManager（注入 ToolRegistry）
  ↓
按 DAG 批次手动调度：
  Batch 1: tag_lyrics (agent, max_iter=1)
  Batch 2: compose_prompt (agent, max_iter=5, 可调用工具)
  Batch 3: write_output (直接写文件，不走 ShellExecutor)
```

### 为什么绕过 Scheduler

`Scheduler::new()` 内部自建空 `ToolRegistry`，无外部注入 API。
自定义工具直接注册在 `AgentManager` 持有的 `ToolRegistry` 上。

Agent 通过 ReAct 范式自动发现并调用工具：
1. LLM 生成 `<action>{"name":"check_char_count","parameters":{...}}</action>`
2. `ReActAgent` 在 `ToolRegistry` 中查找并执行
3. 返回 `<observation>...</observation>` 给 LLM
4. LLM 根据结果决定下一步（压缩 / 精简 / 完成）

### 工具定义

#### check_char_count
- 输入: `{"text": "...", "max_chars": 1000}`
- 输出: `{"char_count": 850, "max_chars": 1000, "within_limit": true, "exceeded_by": 0}`
- 作用: Agent 生成英文后调用，验证是否超限

#### strip_whitespace
- 输入: `{"text": "..."}`
- 输出: `{"result": "压缩后文本", "original_length": 1200, "cleaned_length": 950, "removed_count": 250}`
- 作用: 删除换行/Tab/连续空格，压缩为单空格分隔的紧凑文本

### 工具调用流程

```
Agent 生成英文提示词
  → 调用 check_char_count → 发现超限
  → 调用 strip_whitespace → 压缩空白
  → 再次 check_char_count → 仍在限内
  → 返回 <final_answer>
```

## 3. YAML 步骤结构

```
tag_lyrics (agent)              ← Step 1: 给歌词添加 Suno Tag (max_iter=1)
     ↓
compose_prompt (agent)          ← Step 2: 中英双语作曲提示词 (max_iter=5, 含工具调用)
     ↓
write_output (shell)            ← Step 3: 汇总写入 result.md
```

Step 2 的 Agent 同时输出中英文，用 `<chinese>...</chinese>` 和 `<english>...</english>` XML 标签分隔。
Rust 代码中用 `extract_tag()` 提取两部分内容。

## 4. CLI 参数

```
cargo run --example 19_music_composer -- \
  --lyrics lyrics.txt \
  --style "heavy blues, saxophone Solo" \
  --output-dir /tmp/music-output
```

| 参数 | 短 | 必需 | 说明 |
|---|---|---|---|
| `--lyrics` | `-l` | 是 | 歌词文件路径 |
| `--style` | `-s` | 是 | 风格提示词 |
| `--output-dir` | `-o` | 否 | 输出目录，默认 `/tmp/music-output` |

## 5. 输出格式

`{output_dir}/result.md` 包含四个章节：
1. 歌词 Suno Tag 标注
2. 中文作曲提示词（10 维度）
3. 英文作曲提示词（< 1000 字符，无换行）
4. 英文提示词字符统计（字符数 / 是否通过）

## 6. 文件变更清单

| 文件 | 操作 |
|---|---|
| `examples/code/19_music_composer.rs` | 重写为 CLI + 手动 DAG 调度 + 工具注册 |
| `examples/19_music_composer.yaml` | 合并 Step2+3 → compose_prompt，增加工具提示 |
| `design_music_composer_v2.md` | 本文件 |
