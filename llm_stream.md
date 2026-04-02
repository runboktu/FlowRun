# LLM 流式接口设计文档

## 1. 现状分析

### 1.1 现有架构

```
ReActAgent.run()
  └─→ LlmProvider.call(messages)  ← 阻塞，返回完整 LlmResponse
        └─→ 等待 LLM 全部生成完毕
  └─→ ResponseParser.parse(content)  ← 解析完整响应
  └─→ 执行工具 / 返回最终答案
```

**当前调用链：**
- `ReActAgent::run_internal()` 第 107 行：`self.llm_provider.call(&self.messages).await`
- 阻塞等待完整响应后才 push 到 messages、解析、执行
- 用户侧通过 `AgentCallback` 只能收到 `LlmCall` → `LlmResponse` 两个状态事件

### 1.2 已有铺垫

`AgentStatus` 枚举中**已存在** `LlmChunk` 变体（`types.rs` 第 21 行），说明流式调用在原始设计中已被考虑，但尚未实现。

设计文档 `agent-module-plan-xm.md` 第 142-151 行也预留了 `call_streaming` 方法的接口原型。

### 1.3 关键约束

| 约束 | 说明 |
|------|------|
| **响应解析** | `ResponseParser` 依赖完整 XML 标签（`<thought>...</thought>`），流式场景下标签可能跨 chunk 不完整 |
| **ReAct 循环** | 需要完整的 thought/action/final_answer 才能决定下一步，不能逐 chunk 决策 |
| **回调模型** | 现有 `AgentCallback = Arc<dyn Fn(String, AgentStatus)>` 是同步回调，流式需要高频调用 |
| **依赖** | Cargo.toml 中无 `tokio-stream`，需新增依赖 |
| **向后兼容** | `LlmProvider::call` 必须保持不变，已有 `MockLlmProvider` 和所有调用方不受影响 |

---

## 2. 设计方案

### 2.1 方案选择：Async Stream（推荐）

对比两种主流 Rust 流式模式：

| 维度 | Callback 模式 | Async Stream 模式 |
|------|-------------|-----------------|
| Rust 惯用程度 | 一般（C 风格） | 高（`tokio_stream::Stream`） |
| 背压支持 | 无 | 天然支持 |
| 组合性 | 差（需手动管理状态） | 强（map/filter/merge） |
| 与现有代码兼容 | 高（AgentCallback 已有） | 中（需适配层） |
| 错误传播 | 困难（回调中无法返回 Err） | 自然（Stream<Item=Result<T,E>>） |
| 测试难度 | 高（需 mock 回调行为） | 低（stream::iter 即可） |

**推荐 Async Stream 模式**，同时提供一个 callback 适配层供上层使用。

### 2.2 新增类型定义

#### 2.2.1 `LlmChunk` — 流式数据单元

```rust
/// LLM 流式响应的单个 chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmChunk {
    /// 增量文本内容（可能为空，如仅含 usage 信息）
    pub delta: String,
    /// 是否为该响应的最后一个 chunk
    pub done: bool,
    /// 可选：token 使用信息（在 done=true 时提供）
    pub usage: Option<TokenUsage>,
}

/// Token 使用统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}
```

**设计理由：**
- `delta` 而非 `content`：明确这是增量内容，非累积内容
- `done` 标记：消费者知道何时停止消费、何时拼接完整响应
- `usage` 仅在最后一个 chunk 提供：符合 OpenAI/Anthropic 的 SSE 行为

#### 2.2.2 `LlmStream` — 类型别名

```rust
use tokio_stream::Stream;
use std::pin::Pin;

/// LLM 流式响应类型
pub type LlmStream = Pin<Box<dyn Stream<Item = Result<LlmChunk, AgentError>> + Send>>;
```

**设计理由：**
- 返回 `Pin<Box<dyn Stream>>` 而非 `impl Stream`：trait object 允许不同实现返回不同类型
- `Item = Result<LlmChunk, AgentError>`：错误作为 stream item 传播，消费者用 `try_next()` 处理

### 2.3 `LlmProvider` trait 扩展

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// 同步调用 LLM（保持不变）
    async fn call(&self, messages: &[Message]) -> Result<LlmResponse, AgentError>;

    /// 流式调用 LLM
    /// 
    /// 返回一个 Stream，逐个产出响应 chunk。
    /// 消费者负责累积 delta 以构建完整响应。
    /// 
    /// # 默认实现
    /// 默认实现将 `call()` 包装为单 chunk stream，
    /// 使得不实现流式的 provider 也能无缝使用流式 API。
    fn call_stream(&self, messages: Vec<Message>) -> LlmStream {
        let content = self.call(&messages);
        Box::pin(async_stream::stream! {
            let response = content.await?;
            yield Ok(LlmChunk {
                delta: response.content.clone(),
                done: true,
                usage: None,
            });
        })
    }
}
```

**关键设计决策：**

1. **`fn` 而非 `async fn`**：stream 返回是同步操作（创建 stream 对象），stream 内部才是异步产出。这与 `async_trait` 不兼容，所以不用 `#[async_trait]` 标注此方法。

2. **`Vec<Message>` 而非 `&[Message]`**：stream 的生命周期可能超过调用者的栈帧，需要 owned 数据。`Vec<Message>` 确保 messages 在 stream 存活期间有效。

3. **默认实现**：用 `call()` 包装为单 chunk，保证向后兼容。任何只实现 `call()` 的 provider 自动获得可用的（虽非真正流式的）`call_stream()`。

4. **需要 `async_stream` crate**：提供 `stream!` 宏，简化 stream 构建。需在 Cargo.toml 中添加 `async-stream = "0.3"`。

### 2.4 流式响应累积器

```rust
/// 流式响应累积器
/// 
/// 消费 LlmChunk stream，累积 delta 为完整字符串，
/// 同时提供可选的实时回调。
pub struct StreamConsumer {
    accumulated: String,
    callback: Option<Arc<dyn Fn(&str, AgentStatus) + Send + Sync>>,
}

impl StreamConsumer {
    pub fn new(callback: Option<Arc<dyn Fn(&str, AgentStatus) + Send + Sync>>) -> Self {
        Self {
            accumulated: String::new(),
            callback,
        }
    }

    /// 消费一个 chunk，返回是否完成
    pub fn consume(&mut self, chunk: &LlmChunk) -> bool {
        if !chunk.delta.is_empty() {
            self.accumulated.push_str(&chunk.delta);
            if let Some(cb) = &self.callback {
                cb(&chunk.delta, AgentStatus::LlmChunk);
            }
        }
        chunk.done
    }

    /// 获取累积的完整响应
    pub fn into_content(self) -> String {
        self.accumulated
    }
}
```

**作用：** 桥接 stream 消费和现有的回调系统。`ReActAgent` 可以用它来消费 stream，同时通过 callback 通知上层进度。

### 2.5 `ReActAgent` 流式运行

新增方法，不修改现有 `run()`：

```rust
impl ReActAgent {
    /// 流式运行 Agent
    /// 
    /// 与 run() 相同的 ReAct 循环逻辑，但 LLM 调用使用流式接口，
    /// 通过 callback 实时推送 chunk。
    pub async fn run_stream(
        &mut self,
        user_input: &str,
        callback: AgentCallback,
    ) -> Result<String, AgentError> {
        // 1. 初始化（与 run_internal_with_progress 相同）
        self.messages.clear();
        let system_prompt = self.render_system_prompt().await;
        self.messages.push(Message::system(system_prompt));
        self.messages.push(Message::user(format!("<question>{}</question>", user_input)));

        callback(
            serde_json::json!({"type": "iteration_start"}).to_string(),
            AgentStatus::IterationStart,
        );

        let mut iteration_count = 0;
        while iteration_count < self.max_iterations {
            iteration_count += 1;

            callback(
                serde_json::json!({"type": "llm_call", "iteration": iteration_count}).to_string(),
                AgentStatus::LlmCall,
            );

            // 2. 流式调用 LLM
            let messages = self.messages.clone();
            let mut stream = self.llm_provider.call_stream(messages);
            let mut accumulator = String::new();

            use tokio_stream::StreamExt;
            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result?;
                accumulator.push_str(&chunk.delta);

                // 实时推送 chunk
                callback(
                    serde_json::json!({
                        "type": "llm_chunk",
                        "delta": chunk.delta,
                        "accumulated_length": accumulator.len(),
                        "iteration": iteration_count,
                    }).to_string(),
                    AgentStatus::LlmChunk,
                );

                if chunk.done {
                    if let Some(usage) = chunk.usage {
                        callback(
                            serde_json::json!({
                                "type": "token_usage",
                                "prompt_tokens": usage.prompt_tokens,
                                "completion_tokens": usage.completion_tokens,
                                "total_tokens": usage.total_tokens,
                            }).to_string(),
                            AgentStatus::LlmResponse,
                        );
                    }
                }
            }

            // 3. 用累积的完整响应继续 ReAct 循环
            // （解析、工具调用等逻辑与 run_internal_with_progress 完全相同）
            self.messages.push(Message::assistant(accumulator.clone()));
            let parsed = self.parser.parse(&accumulator);
            // ... 后续逻辑不变
        }
    }
}
```

**关键观察：** ReAct 循环的**决策阶段**（解析 thought/action/final_answer）仍然需要完整响应。流式的价值在于：
- 用户体验：实时看到 LLM 生成内容
- 长响应场景：不必等待数十秒才看到第一个字符
- 工具调用场景：用户可以看到 agent 的 thought 逐步生成

### 2.6 `MockLlmProvider` 流式实现

```rust
#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn call(&self, _messages: &[Message]) -> Result<LlmResponse, AgentError> {
        // 现有实现不变
    }

    fn call_stream(&self, messages: Vec<Message>) -> LlmStream {
        let responses = self.responses.clone();
        let call_count = &self.call_count;

        Box::pin(async_stream::stream! {
            let count = call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let response = responses
                .get(count % responses.len())
                .cloned()
                .unwrap_or_else(|| "<final_answer>Mock response</final_answer>".to_string());

            // 模拟流式：按空格分割，逐词产出
            let words: Vec<&str> = response.split_whitespace().collect();
            for (i, word) in words.iter().enumerate() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                yield Ok(LlmChunk {
                    delta: if i == 0 { word.to_string() } else { format!(" {}", word) },
                    done: false,
                    usage: None,
                });
            }
            // 最后一个 chunk 标记 done
            yield Ok(LlmChunk {
                delta: String::new(),
                done: true,
                usage: None,
            });
        })
    }
}
```

---

### 2.7 Chunk 完整性保证

#### 问题本质

`stream.next()` 本身**不保证**语义完整性。TCP 是字节流协议，不保留消息边界：

```
网络层实际收到的可能是：
  data: {"choices":[{"delta":{"content":"你好"}}]}\n\ndata: {"choices":[{"delta":{"content":"世界"}}]}

但一次 read() 可能只拿到：
  data: {"choices":[{"delta":{"content":"你好"}}]}\n\ndata: {"choices":[{"d
  ← 第二条 JSON 被截断了
```

如果直接在 TCP read 后 yield，消费者会收到不完整的 chunk。

#### SSE（Server-Sent Events）帧详解

##### 什么是 SSE？

SSE 是基于 HTTP 长连接的**服务器推送文本事件协议**。服务器保持连接打开，持续向客户端发送文本数据。OpenAI、Anthropic、Gemini 等所有主流 LLM 的流式 API 都基于 SSE 传输。

##### 帧（Frame / Event）的概念

一个完整的 SSE 帧由若干**字段行**组成，以**双换行 `\n\n`** 作为帧结束标记：

```
event: content_block_delta
data: {"choices":[{"delta":{"content":"你"}}]}
id: 42
retry: 3000

```

| 字段 | 必需 | 说明 |
|------|------|------|
| `event:` | 否 | 事件类型名（如 `message`, `content_block_delta`） |
| `data:` | 是 | 载荷数据，**可跨多行**（每行一个 `data:` 前缀） |
| `id:` | 否 | 事件 ID，用于断线重连时告知服务器从哪恢复 |
| `retry:` | 否 | 客户端重连超时（毫秒） |
| `:` | 否 | 注释行（冒号开头无字段名，服务器用于保活心跳） |

**帧分隔符是 `\n\n`（双换行），不是单个 `\n`。** 单个 `\n` 只是帧内的字段分隔符。

##### 多行 data 字段

`data:` 可以跨多行，解析时将多行内容用 `\n` 拼接：

```
data: {"choices":[
data:   {"delta":{"content":"hello"}}
data: ]}

```

解析后 data 值为：`{"choices":[\n  {"delta":{"content":"hello"}}\n]}`

##### 实际 LLM 流式响应示例

OpenAI 流式响应中连续发送的帧：

```
data: {"id":"chatcmpl-123","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"你好"},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"，"},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"世界"},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5}}

data: [DONE]

```

每一对 `\n\n` 之间就是一个**帧**。最后一帧 `[DONE]` 是流结束的特殊标记。

##### 为什么需要显式帧解析？

TCP 是字节流，不保留帧边界。客户端一次 `read()` 可能遇到三种情况：

```
场景 A — 帧被截断（不完整帧）：
  data: {"choices":[{"delta":{"content":"你好"}}]}
  
  data: {"choices":[{"delta":{"content":"
  ← 第二个帧不完整，需要缓冲等待更多字节

场景 B — 多帧合并（Nagle 算法）：
  data: {"a":1}
  
  data: {"a":2}
  
  data: {"a":3}
  
  ← 一次 read 拿到三个完整帧，需要拆分

场景 C — 帧内换行（多行 data）：
  data: {"text":"line1
  data: line2"}
  
  ← 单个帧内的 data 跨了两行，不能按 \n 简单分割
```

SSE 解析器的职责就是：**在 TCP 字节流中识别 `\n\n` 边界，提取完整帧，缓冲不完整的部分**。

#### 解决方案：分层解析架构

保证 chunk 完整的是 **SSE 帧解析层**，而非网络层。正确的分层：

```
TCP 字节流 (reqwest bytes_stream())
    ↓ 按字节累积
SSE 帧解析器 (按 \n\n 分帧，缓冲不完整帧)
    ↓ 产出完整 data: 行
JSON 解析器 (解析 data: 后面的 JSON payload)
    ↓ 映射为 LlmChunk
LlmChunk 产出 (stream.yield — 仅在完整解析后触发)
```

**核心原则：`yield` 只在 SSE 帧完整解析之后触发，不完整的帧留在缓冲区等待后续字节。**

#### Provider 实现示意

```rust
fn call_stream(&self, messages: Vec<Message>) -> LlmStream {
    let client = self.client.clone();
    let request = self.build_request(messages);

    Box::pin(async_stream::stream! {
        let response = client.execute(request).await?;
        let mut stream = response.bytes_stream();

        // SSE 行缓冲 — 关键：跨 TCP read 维护状态
        let mut sse_buffer = String::new();

        while let Some(bytes_result) = stream.next().await {
            let bytes = bytes_result.map_err(|e| AgentError::LlmError(e.to_string()))?;
            sse_buffer.push_str(&String::from_utf8_lossy(&bytes));

            // 按 \n\n 分割 SSE 帧
            while let Some(pos) = sse_buffer.find("\n\n") {
                let frame = sse_buffer[..pos].to_string();
                sse_buffer = sse_buffer[pos + 2..].to_string();

                // 解析完整的一帧，只有成功解析才 yield
                if let Some(chunk) = parse_sse_frame(&frame)? {
                    yield Ok(chunk);
                }
            }
        }

        // 流结束：处理尾部可能残留的完整帧
        if let Some(chunk) = parse_sse_frame(&sse_buffer)? {
            yield Ok(chunk);
        }
    })
}
```

#### 类比

就像 `BufRead::read_line()` 保证返回完整的一行——不是因为 TCP 按行发送，而是因为解析器在内部缓冲、按 `\n` 切分。

`stream.next()` 返回的是**语义完整的 LlmChunk**，前提是 provider 的 `call_stream` 实现内部做了正确的 SSE 帧缓冲和解析。这是 provider 实现者的责任，不是消费者的责任。

---

## 3. 文件变更清单

| 文件 | 变更类型 | 内容 |
|------|---------|------|
| `Cargo.toml` | 新增依赖 | `tokio-stream = "0.1"`, `async-stream = "0.3"` |
| `src/agent/types.rs` | 新增类型 | `LlmChunk`, `TokenUsage`, `LlmStream` 类型别名 |
| `src/agent/llm_adapter.rs` | 扩展 trait | 添加 `call_stream()` 方法（含默认实现）；`MockLlmProvider` 实现流式 |
| `src/agent/react_agent.rs` | 新增方法 | 添加 `run_stream()` 方法 |
| `src/agent/mod.rs` | 导出更新 | 导出新类型 |
| `src/agent/error.rs` | 可选 | 考虑添加 `StreamError` 变体（如果需要区分流式特有错误） |

---

## 4. 架构关系图

```
                    ┌──────────────────────────────────┐
                    │         ReActAgent               │
                    │                                  │
                    │  run() ──→ call() (同步)         │
                    │  run_stream() ──→ call_stream()  │
                    └──────────┬───────────────────────┘
                               │
              ┌────────────────┼────────────────┐
              ▼                ▼                ▼
    ┌─────────────┐  ┌──────────────┐  ┌─────────────┐
    │ call()      │  │ call_stream()│  │ call_stream()│
    │ (阻塞)      │  │ (OpenAI)     │  │ (Mock)       │
    └─────────────┘  └──────┬───────┘  └──────┬──────┘
                            │                 │
                            ▼                 ▼
                   Stream<Item=Result<   Stream<Item=Result<
                     LlmChunk, Err>>       LlmChunk, Err>>
                            │                 │
                            ▼                 ▼
                    ┌─────────────┐   ┌─────────────┐
                    │ SSE 解析     │   │ 逐词分割     │
                    │ (reqwest)   │   │ (sleep模拟)  │
                    └─────────────┘   └─────────────┘
```

---

## 5. 未来扩展：具体 Provider 实现

设计文档不包含具体实现，但提供接口后，各 provider 实现流式的模式如下：

### 5.1 OpenAI Provider（预期结构）

```rust
impl LlmProvider for OpenAiProvider {
    async fn call(&self, messages: &[Message]) -> Result<LlmResponse, AgentError> {
        // 现有：POST /chat/completions, stream=false
    }

    fn call_stream(&self, messages: Vec<Message>) -> LlmStream {
        // POST /chat/completions, stream=true
        // 使用 reqwest 的 bytes() 逐行解析 SSE
        // 每行 data: {"choices":[{"delta":{"content":"..."}}]}
        // 产出 LlmChunk { delta, done: false, .. }
        // 最后产出 LlmChunk { delta: "", done: true, usage }
    }
}
```

### 5.2 Anthropic Provider（预期结构）

```rust
impl LlmProvider for AnthropicProvider {
    fn call_stream(&self, messages: Vec<Message>) -> LlmStream {
        // POST /v1/messages, stream=true
        // SSE event: content_block_delta { delta: { text: "..." } }
        // SSE event: message_stop → done = true
    }
}
```

---

## 6. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| SSE 解析复杂性 | 不同 provider 的 SSE 格式不同 | 每个 provider 独立实现解析逻辑，不共享 |
| Stream 内存泄漏 | 消费者未消费完导致资源占用 | Stream 的 Drop 实现应取消底层 HTTP 请求 |
| 回调高频调用 | `AgentCallback` 每次 chunk 都调用，可能影响性能 | 回调中避免重操作；或提供节流选项 |
| `async-stream` 依赖 | 新增外部依赖 | 该 crate 由 tokio 团队维护，生态成熟 |
| `Vec<Message>` clone | 每次 `call_stream` 需要 owned messages | ReAct 循环中 messages 本就需要 clone（stream 生命周期 > 调用栈） |

---

## 7. 总结

**核心设计原则：**
1. **向后兼容**：`call()` 不变，`call_stream()` 有默认实现
2. **Rust 惯用**：使用 `tokio_stream::Stream` 而非回调
3. **最小侵入**：仅扩展 trait，不修改现有方法签名
4. **渐进采用**：可以先实现 `call_stream()` 接口，具体 provider 逐步迁移
5. **ReAct 循环不变**：流式只改变 LLM 调用的数据传输方式，不改变推理逻辑
