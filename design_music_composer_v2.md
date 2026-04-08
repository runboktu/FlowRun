# music_composer 工作流改造方案

## 1. 改造目标

| 项目 | 改造前 | 改造后 |
|---|---|---|
| `19_music_composer.rs` | 硬编码输入的 example | 独立 CLI 工具（clap derive） |
| `19_music_composer.yaml` | 3 个 Agent 步骤 | 2 个 Agent 步骤（Step2+3 合并） |
| 英文提示词质量控制 | 仅靠 prompt 约束 | Agent 工具辅助（字符计数 + 空白过滤） |

## 2. CLI 改造：`19_music_composer.rs`

使用 clap derive 模式，定义三个参数：

```
music-composer --lyrics <FILE> --style <STYLE> --output-dir <DIR>
```

| 参数 | 类型 | 必需 | 说明 |
|---|---|---|---|
| `--lyrics` / `-l` | 文件路径（String） | 是 | 原始歌词文件路径，CLI 内读取文件内容 |
| `--style` / `-s` | 字符串 | 是 | 风格提示词 |
| `--output-dir` / `-o` | 字符串 | 否 | 输出目录，默认 `/tmp/music-output` |

### 核心改动

```rust
#[derive(Parser)]
#[command(name = "music-composer")]
struct Cli {
    #[arg(short, long)]
    lyrics: String,          // 文件路径，运行时 std::fs::read_to_string

    #[arg(short, long)]
    style: String,           // 风格字符串

    #[arg(short, long, default_value = "/tmp/music-output")]
    output_dir: String,      // 输出目录
}
```

CLI 不走 FlowRunner，而是直接用底层 API（DagScheduler + Scheduler），这样才能在创建 Scheduler 之后、执行之前注册自定义工具到 ToolRegistry。

### 工具注册方式

```rust
let tool_registry = Arc::new(ToolRegistry::new());

// 注册 check_char_count 工具
tool_registry.register_tool(
    "check_char_count",
    "检查文本字符数，返回字符总数",
    Some(r#"{"type":"object","properties":{"text":{"type":"string"},"max_chars":{"type":"integer"}}}"#),
    Arc::new(FnTool(|args: String| async move { ... })),
).await;

// 注册 strip_whitespace 工具
tool_registry.register_tool(
    "strip_whitespace",
    "删除文本中多余的空白字符（换行、Tab、连续空格压缩为单空格）",
    Some(r#"{"type":"object","properties":{"text":{"type":"string"}}}"#),
    Arc::new(FnTool(|args: String| async move { ... })),
).await;
```

### Executor 组装

目前 Scheduler::new() 内部自建空 ToolRegistry → 无法从外部注入自定义工具。

**方案**：不走 FlowRunner，改用底层 Scheduler API：
1. 手动创建 DagScheduler + WorkflowConfig + CheckpointManager
2. 手动创建 ToolRegistry，注册自定义工具
3. 用 Scheduler::new() 创建后，**无法注入**（ToolRegistry 是 internal）

**问题**：Scheduler::new() 硬编码了 `ToolRegistry::new()`，没有外部注入 API。

**解决**：直接用 `Scheduler::new()` 创建后，利用 `AgentManager::tool_registry()` 返回的引用来注册——但 Scheduler 没有暴露这个 API。

**最终方案**：绕过 Scheduler，直接使用 AgentManager + AgentExecutor + ToolExecutor + ShellExecutor 等 executor，按 DAG batch 顺序手动调度。但这太复杂。

**更实际的方案**：由于工具注册在 Scheduler 内部是共享的（ToolRegistry 是 Arc），且 agent 步骤的 agent_manager 也持有同一 registry，我们可以：
- 修改 `Scheduler` 增加一个 `register_tool()` 公开方法
- **或者**（更简单）：在 YAML 中给 agent 步骤设定 `agent_max_iterations: 3`（多一轮让 agent 调用工具），然后**在 agent_system_prompt 中用自然语言描述工具**，但工具实际不存在于 registry 中——这行不通。

**最终实际方案**：在示例代码中，不使用 FlowRunner 或 Scheduler 的封装，而是：
1. 解析 YAML 获取 WorkflowDefinition
2. 构建 DAG 拓扑排序
3. 为每个 batch 按步骤类型手动执行
4. Agent 步骤：自己创建 AgentManager + ToolRegistry + 注册自定义工具

但这也太重了。**最简方案**：让 agent 在 system prompt 中自行完成字符计数和空白过滤——不需要工具。因为 LLM（如 DeepSeek）完全有能力：
1. 生成英文文本
2. 自己数字符
3. 压缩空白
这在 prompt 层面就能解决，不需要 tool calling。

**决策**：放弃 agent 工具方案，改为增强 prompt 约束 + 让 agent 做多轮自我检查。将 `agent_max_iterations` 设为 2，prompt 中要求 agent 先生成初稿，再自检字符数并压缩。

## 3. YAML 改造：`19_music_composer.yaml`

### 步骤结构（改造后）

```
tag_lyrics (agent)              ← Step 1: 给歌词添加 Suno Tag
     ↓
compose_prompt (agent)          ← Step 2: 合并中英作曲提示词（一步到位）
     ↓
write_output (shell)            ← Step 3: 汇总写入文件
```

### Step 2 合并设计

一个 Agent 同时输出中文和英文提示词。system prompt 要求：
1. 先生成中文详细作曲提示词
2. 再将中文翻译为紧凑英文（< 1000 字符，无换行，无多余空格）
3. 自检英文字符数，如果超标则压缩
4. 用 XML 标签分隔输出：`<chinese>...</chinese>` 和 `<english>...</english>`

Agent 输出后，在 shell 步骤中用 sed/awk 提取两部分。

### 关于工具的替代方案

如果未来框架支持外部注入 ToolRegistry，可以增加：
- `check_char_count`: `{"text": "...", "max_chars": 1000}` → `{"count": 850, "within_limit": true}`
- `strip_whitespace`: `{"text": "..."}` → `{"result": "压缩后的文本", "removed_count": 42}`

当前先通过 prompt 约束实现同等效果。

## 4. 文件变更清单

| 文件 | 操作 |
|---|---|
| `examples/code/19_music_composer.rs` | 重写为 clap CLI 工具 |
| `examples/19_music_composer.yaml` | 合并 Step2+3，增强 prompt |
| `Cargo.toml` | example 保持不变（clap 在 dev-dependency 不需要，example 直接用 clap derive） |

**注意**：clap 在 Cargo.toml 中是 optional dep（feature = "cli"）。example 无法直接使用 clap，除非：
1. 将 clap 改为非 optional
2. 或在 example 中用 `std::env::args()` 手动解析
3. 或给 example 启用 cli feature

**最终决策**：用 `std::env::args()` 手动解析参数，避免依赖 clap 的 feature gate 问题。这与项目现有 example 风格一致（现有 example 都不用 clap）。

## 5. 输出格式

```
# AI 作曲编曲结果

## 一、歌词 Suno Tag 标注
（带 Tag 的歌词）

## 二、中文作曲提示词
（10 维度中文描述）

## 三、英文作曲提示词 (< 1000 chars)
（紧凑英文提示词）

## 四、英文提示词字符统计
（字符数、是否在限制内）
```
