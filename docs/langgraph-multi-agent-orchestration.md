# LangGraph 多 Agent 编排详解

> **版本**: v2.0 | **日期**: 2026-04-11 | **作者**: Sisyphus AI Architect

---

## 目录

1. [多 Agent 系统概述](#1-多-agent-系统概述)
2. [核心 API 与原语](#2-核心-api-与原语)
3. [模式一：Supervisor（管理者）](#3-模式一supervisor管理者)
   - [3.4 内部机制：Agent 如何通信](#34-内部机制agent-如何通信)
   - [3.5 内部机制：Agent 如何创建](#35-内部机制agent-如何创建)
   - [3.6 内部机制：Supervisor 如何决定调用哪个 Agent](#36-内部机制supervisor-如何决定调用哪个-agent)
   - [3.7 内部机制：大模型会话消息是否共享](#37-内部机制大模型会话消息是否共享)
4. [模式二：Swarm（群集对等）](#4-模式二swarm群集对等)
5. [模式三：Hierarchical（层级管理）](#5-模式三hierarchical层级管理)
6. [模式四：Agent-as-Tool（工具化代理）](#6-模式四agent-as-tool工具化代理)
7. [模式五：Orchestrator-Worker / MapReduce](#7-模式五orchestrator-worker--mapreduce)
8. [模式六：Network / P2P（对等网络）](#8-模式六network--p2p对等网络)
9. [状态共享与隔离](#9-状态共享与隔离)
10. [最佳实践](#10-最佳实践)
11. [反模式与常见陷阱](#11-反模式与常见陷阱)
12. [与 flow-run 的对比](#12-与-flow-run-的对比)

---

## 1. 多 Agent 系统概述

### 1.1 为什么需要多 Agent？

单个 Agent 在处理简单任务时表现出色，但面对复杂场景时会遇到瓶颈：

| 单 Agent 问题 | 多 Agent 方案 |
|:---|:---|
| Prompt 过长，上下文窗口溢出 | 每个 Agent 有独立的 Prompt 和上下文 |
| 工具过多，LLM 选择困难 | 每个 Agent 只关注自己的工具集 |
| 角色冲突（同时做研究+写代码） | 专职 Agent 各司其职 |
| 错误传播，一个环节出错全盘失败 | Agent 隔离，错误可控 |
| 难以并行处理 | 独立 Agent 可并行执行 |

### 1.2 LangGraph 多 Agent 架构层次

LangGraph 提供了从简单到复杂的多 Agent 编排能力：

```
┌─────────────────────────────────────────────────┐
│              LangGraph 多 Agent 编排              │
│                                                   │
│  ┌── 声明式 API ──────────────────────────────┐  │
│  │  create_supervisor()   — 快速构建管理者模式  │  │
│  │  create_swarm()        — 快速构建群集模式    │  │
│  └────────────────────────────────────────────┘  │
│                                                   │
│  ┌── 底层 API ────────────────────────────────┐  │
│  │  StateGraph + add_node() — 自定义图结构      │  │
│  │  Command(goto=...)      — 动态路由           │  │
│  │  Send("node", data)     — 动态并行           │  │
│  │  Command.PARENT         — 跨图导航           │  │
│  └────────────────────────────────────────────┘  │
│                                                   │
│  ┌── 预构建 Agent ────────────────────────────┐  │
│  │  create_react_agent()   — ReAct 模式 Agent   │  │
│  └────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────┘
```

---

## 2. 核心 API 与原语

### 2.1 create_react_agent — 构建单个 Agent

所有多 Agent 系统的基础单元。创建一个工具增强的 ReAct Agent：

```python
from langchain.chat_models import init_chat_model
from langgraph.prebuilt import create_react_agent

# 定义工具
def search_web(query: str) -> str:
    """搜索互联网"""
    return f"搜索结果: {query} 的最新信息..."

def calculate(expression: str) -> float:
    """数学计算"""
    return eval(expression)

# 创建 Agent
research_agent = create_react_agent(
    model=init_chat_model("openai:gpt-4.1-mini"),
    tools=[search_web],
    name="research_expert",
    prompt="你是一个世界级研究员，擅长使用网络搜索获取信息。不要做数学计算。"
)

math_agent = create_react_agent(
    model=init_chat_model("openai:gpt-4.1-mini"),
    tools=[calculate],
    name="math_expert",
    prompt="你是一个数学专家，擅长精确计算。每次只使用一个工具。"
)
```

### 2.2 Command — 状态更新 + 路由

`Command` 是多 Agent 编排的核心路由机制，允许一个节点同时**更新状态**和**指定下一个节点**：

```python
from langgraph.types import Command
from typing import Literal

def my_agent_node(state) -> Command[Literal["agent_b", "agent_c", "__end__"]]:
    """Agent 根据当前状态决定路由到哪个 Agent"""
    result = llm.invoke(state["messages"])

    if need_agent_b(result):
        return Command(
            update={"messages": [result]},   # 更新状态
            goto="agent_b"                   # 路由到 agent_b
        )
    elif need_agent_c(result):
        return Command(
            update={"messages": [result]},
            goto="agent_c"
        )
    else:
        return Command(
            update={"messages": [result]},
            goto="__end__"                   # 结束
        )
```

**Command 三要素**:

| 参数 | 作用 | 说明 |
|:---|:---|:---|
| `update` | 状态更新 | 字典，合并到当前状态 |
| `goto` | 路由目标 | 下一个节点名称 |
| `graph` | 跨图导航 | `Command.PARENT` 导航到父图 |

### 2.3 Command.PARENT — 跨子图导航

当 Agent 作为子图嵌入父图时，使用 `Command.PARENT` 从子图直接路由到父图的其他节点：

```python
import operator
from typing import Annotated
from typing_extensions import TypedDict
from langgraph.graph import StateGraph, START, END
from langgraph.types import Command

class State(TypedDict):
    # 注意：父图必须定义 reducer，因为子图也会更新这个 key
    messages: Annotated[list, operator.add]

def agent_in_subgraph(state: State):
    """子图中的 Agent，可以直接路由到父图的节点"""
    result = llm.invoke(state["messages"])

    return Command(
        update={"messages": [result]},
        goto="parent_node_b",              # 父图中的节点
        graph=Command.PARENT               # 关键：导航到父图
    )

# 子图
subgraph = (
    StateGraph(State)
    .add_node("agent_a", agent_in_subgraph)
    .add_edge(START, "agent_a")
    .compile()
)

# 父图
builder = StateGraph(State)
builder.add_edge(START, "subgraph")
builder.add_node("subgraph", subgraph)       # 子图作为节点
builder.add_node("parent_node_b", node_b)    # 父图节点
builder.add_node("parent_node_c", node_c)

graph = builder.compile()
```

### 2.4 Send — 动态并行分发

`Send` API 用于 map-reduce 风格的并行工作流。动态创建节点实例，每个实例有独立状态：

```python
from langgraph.types import Send

def orchestrator(state):
    """规划任务，将每个分片发送给 worker"""
    sections = planner.invoke(state["topic"])
    return {"sections": sections.sections}

def assign_workers(state):
    """为每个 section 创建一个 worker"""
    return [
        Send("llm_call", {"section": s})
        for s in state["sections"]
    ]

def llm_call(state):
    """Worker：处理单个 section"""
    section = state["section"]
    result = llm.invoke(f"请撰写关于 {section.name} 的报告段落")
    return {"completed_sections": [result.content]}

def synthesizer(state):
    """汇总所有 worker 的结果"""
    return {"final_report": "\n".join(state["completed_sections"])}
```

### 2.5 create_handoff_tool — Agent 间移交

`create_handoff_tool` 创建一个工具，允许 Agent 将控制权移交给另一个 Agent：

```python
from langgraph.prebuilt import create_handoff_tool

# Agent A 可以移交到 Agent B 或 Agent C
agent_a = create_react_agent(
    model=model,
    tools=[
        tool_a,
        create_handoff_tool(agent_name="agent_b"),
        create_handoff_tool(agent_name="agent_c"),
    ],
    name="agent_a",
    prompt="你是通用助手。遇到科学问题移交给 agent_b，翻译问题移交给 agent_c。"
)
```

---

## 3. 模式一：Supervisor（管理者）

### 3.1 架构图

```
                 ┌──────────────┐
                 │   Supervisor  │
                 │  （路由决策）  │
                 └──┬───┬───┬───┘
                    │   │   │
            ┌───────┘   │   └───────┐
            ▼           ▼           ▼
      ┌──────────┐ ┌──────────┐ ┌──────────┐
      │ Agent A  │ │ Agent B  │ │ Agent C  │
      │ (研究)    │ │ (计算)    │ │ (写作)    │
      └──────────┘ └──────────┘ └──────────┘
            │           │           │
            └───────┬───┘───────────┘
                    ▼
                 返回 Supervisor
              （继续或结束）
```

**特点**: 中央 Supervisor 接收所有请求，决定路由到哪个专业 Agent，Agent 完成后返回 Supervisor。

### 3.2 使用 create_supervisor 快速构建

```python
from langchain.chat_models import init_chat_model
from langgraph.prebuilt import create_react_agent
from langgraph_supervisor import create_supervisor

model = init_chat_model("openai:gpt-4.1-mini")

# 定义工具
def web_search(query: str) -> str:
    """搜索互联网获取信息"""
    return f"搜索结果: {query} 的最新信息..."

def add(a: int, b: int) -> int:
    """加法运算"""
    return a + b

def multiply(a: int, b: int) -> int:
    """乘法运算"""
    return a * b

# 创建专业 Agent
research_agent = create_react_agent(
    model=model,
    tools=[web_search],
    name="research_expert",
    prompt="你是一个世界级研究员，擅长使用网络搜索。不做数学计算。"
)

math_agent = create_react_agent(
    model=model,
    tools=[add, multiply],
    name="math_expert",
    prompt="你是一个数学专家。每次只使用一个工具。"
)

# 创建 Supervisor
workflow = create_supervisor(
    [research_agent, math_agent],
    model=model,
    prompt=(
        "你是一个团队管理者，管理一个研究专家和一个数学专家。"
        "需要搜索信息时，使用 research_expert。"
        "需要数学计算时，使用 math_expert。"
    )
)

# 编译并运行
app = workflow.compile()
result = app.invoke({
    "messages": [
        {"role": "user", "content": "FAANG 公司 2024 年总员工数是多少？"}
    ]
})
```

### 3.3 手动构建 Supervisor（底层 API）

当需要更精细的控制时，可以用 StateGraph 手动构建：

```python
from typing import Literal
from typing_extensions import TypedDict
from langgraph.graph import StateGraph, MessagesState, START, END
from langgraph.types import Command
from langchain.messages import HumanMessage

# 创建专业 Agent
research_agent = create_react_agent(model, [web_search], name="research")
math_agent = create_react_agent(model, [add, multiply], name="math")

# Supervisor 节点：决定路由
def supervisor_node(state: MessagesState) -> Command[Literal["research", "math", "__end__"]]:
    """Supervisor 分析用户请求，决定路由到哪个 Agent"""
    response = model.invoke([
        {"role": "system", "content": """你是路由器。分析用户消息，决定交给哪个 Agent：
        - research：需要搜索信息、查找资料
        - math：需要数学计算
        - 如果已获得最终答案，回复 FINISH"""},
        *state["messages"]
    ])

    decision = response.content.strip().lower()

    if "research" in decision:
        return Command(goto="research")
    elif "math" in decision:
        return Command(goto="math")
    else:
        return Command(goto="__end__")

# Agent 包装节点：Agent 执行后返回 Supervisor
def research_node(state: MessagesState) -> Command[Literal["supervisor"]]:
    result = research_agent.invoke(state)
    return Command(
        update={
            "messages": [
                HumanMessage(content=result["messages"][-1].content, name="research")
            ]
        },
        goto="supervisor",
    )

def math_node(state: MessagesState) -> Command[Literal["supervisor"]]:
    result = math_agent.invoke(state)
    return Command(
        update={
            "messages": [
                HumanMessage(content=result["messages"][-1].content, name="math")
            ]
        },
        goto="supervisor",
    )

# 构建图
builder = StateGraph(MessagesState)
builder.add_node("supervisor", supervisor_node)
builder.add_node("research", research_node)
builder.add_node("math", math_node)

builder.add_edge(START, "supervisor")

graph = builder.compile()
```

### 3.4 内部机制：Agent 如何通信

#### 共享消息列表（messages）

LangGraph Supervisor 的核心通信机制是一个 **共享的 `messages` 列表**。所有 Agent（包括 Supervisor 和 Worker）读写同一个 `state["messages"]`：

```python
from langgraph.graph import MessagesState

# MessagesState 内部定义：
# class MessagesState(TypedDict):
#     messages: Annotated[list, add_messages]   # ← reducer: operator.add
#
# add_messages 的行为：新消息自动追加到列表末尾，不会覆盖
```

**消息流转过程**：

```
用户消息 → state["messages"] = [HumanMessage("FAANG 员工总数?")]
                │
                ▼
          ┌───────────┐
          │ Supervisor │  读取 state["messages"]（看到用户消息）
          │    LLM     │  决定调用 transfer_to_research_expert 工具
          └─────┬─────┘
                │  AIMessage(tool_calls=[transfer_to_research_expert])
                │  ToolMessage("Successfully transferred to research_expert")
                ▼  ← 两条消息追加到 state["messages"]
          ┌───────────┐
          │  Research  │  读取 state["messages"]（看到完整历史）
          │   Agent    │  使用 web_search 工具搜索
          └─────┬─────┘
                │  [Agent 内部的所有 tool call / tool result / AI response]
                │  ← Agent 输出写回 state["messages"]
                ▼
          ┌───────────┐
          │ Supervisor │  读取更新后的 state["messages"]（含 Research 结果）
          │    LLM     │  决定调用 transfer_to_math_expert 工具
          └─────┬─────┘
                │  ... 循环 ...
```

**关键点**：
- `messages` 使用 `operator.add` reducer，所有写入都是**追加**而非覆盖
- 每个 Worker Agent 能看到之前所有 Agent 的对话记录
- Supervisor 每次循环都能看到最新的完整消息历史

#### output_mode：控制 Agent 输出如何写回

`create_supervisor()` 的 `output_mode` 参数决定 Worker Agent 的输出如何进入共享消息列表：

| output_mode | 行为 | 适用场景 |
|:---|:---|:---|
| `last_message`（默认） | 只保留 Worker 的**最后一条**消息 | 节省 token，简洁 |
| `full_history` | 保留 Worker 的**完整**对话历史（含 tool call） | 需要审计 Agent 推理过程 |

```
output_mode="last_message" 的消息流：
  [User] → [Supervisor] → [Research] → [Research final answer] → [Supervisor] → ...

output_mode="full_history" 的消息流：
  [User] → [Supervisor] → [Research tool_call] → [Research tool_result]
  → [Research tool_call] → [Research tool_result] → [Research answer] → [Supervisor] → ...
```

### 3.5 内部机制：Agent 如何创建

#### create_react_agent 返回的是编译后的子图

`create_react_agent()` 并非返回一个简单的函数，而是返回一个 **LangGraph CompiledStateGraph**（即 Pregel 实例）。它本身就是一个完整的 ReAct 循环图：

```python
research_agent = create_react_agent(
    model=model,
    tools=[web_search],
    name="research_expert",
    prompt="你是一个世界级研究员..."
)
# research_agent 是一个 CompiledStateGraph
# 内部结构：START → agent_node → (tool_call? → tool_node → agent_node) → END
#                                   └──(no tool_call) → END
```

**子图内部结构**：

```
research_agent (CompiledStateGraph)
  ┌─────────────────────────────────────┐
  │  START                               │
  │    ↓                                 │
  │  agent_node ──→ LLM 推理             │
  │    │                                 │
  │    ├── 需要 tool_call?               │
  │    │   ├── YES → tool_node → agent_node (循环) │
  │    │   └── NO  → END                 │
  │                                      │
  │  输入: {"messages": [...]}            │
  │  输出: {"messages": [AI response]}    │
  └─────────────────────────────────────┘
```

**关键属性**：
- `name`: 唯一标识符，Supervisor 用这个名字路由
- `prompt`: 转为 SystemMessage，插入到 messages 列表最前面
- `tools`: 该 Agent 独有的工具集（其他 Agent 看不到这些工具）
- 每个 Agent 有独立的 LLM 调用上下文

#### create_supervisor 返回的是 StateGraph

`create_supervisor()` 返回一个未编译的 `StateGraph`，需要调用 `.compile()` 才能运行：

```python
workflow = create_supervisor(
    [research_agent, math_agent],
    model=model,
    prompt="你是团队管理者..."
)
# workflow 是 StateGraph，需要 compile
app = workflow.compile(checkpointer=MemorySaver())
# app 是 CompiledStateGraph，可以 invoke / astream
```

### 3.6 内部机制：Supervisor 如何决定调用哪个 Agent

#### 核心：Agent 被转换为 Handoff Tool

这是最关键的设计。`create_supervisor()` 在内部将每个 Agent **包装成一个 Tool**，供 Supervisor 的 LLM 通过 **tool calling** 选择调用：

```python
# create_supervisor 内部大致做了这些事情：

# 1. 为每个 Agent 创建 handoff tool
for agent in agents:
    tool = create_handoff_tool(agent_name=agent.name)
    # 例如 agent.name="research_expert"
    # 生成名为 "transfer_to_research_expert" 的工具

# 2. 这些 handoff tool 加入 Supervisor 的工具列表
supervisor_tools = handoff_tools + (user_provided_tools or [])

# 3. Supervisor 本身也是一个 create_react_agent
supervisor = create_react_agent(
    model=model,
    tools=supervisor_tools,    # ← handoff tools 在这里
    prompt=supervisor_prompt,
)
```

**Supervisor 看到的工具列表示例**：

```
Supervisor 的 tools:
  - transfer_to_research_expert    ← handoff tool（自动生成）
  - transfer_to_math_expert        ← handoff tool（自动生成）
  - (用户自定义的额外工具)          ← 可选
```

#### handoff tool 的内部实现

每个 handoff tool 本质上是一个 LangChain Tool，调用时返回一个 `Command`：

```python
# create_handoff_tool 的简化实现
@tool(f"transfer_to_{agent_name}")
def handoff_to_agent(
    state: Annotated[dict, InjectedState],    # 注入当前图状态
    tool_call_id: Annotated[str, InjectedToolCallId],
):
    # 返回 Command：路由到目标 Agent，同时更新消息
    return Command(
        goto=agent_name,                       # 路由到目标 Agent 子图
        graph=Command.PARENT,                  # 在父图级别路由
        update={
            "messages": state["messages"] + [  # 追加 handoff 消息
                ToolMessage(
                    content=f"Successfully transferred to {agent_name}",
                    tool_call_id=tool_call_id,
                )
            ],
            "active_agent": agent_name,
        },
    )
```

#### 完整的路由决策流程

```
1. 用户输入 → state["messages"] = [HumanMessage("FAANG 总人数?")]

2. Supervisor LLM 被调用
   输入: state["messages"]（含 system prompt + 用户消息）
   输出: AIMessage(tool_calls=[
     {"name": "transfer_to_research_expert", "args": {}}
   ])
   ↑ Supervisor 的 LLM 通过标准 tool calling 选择了 handoff tool

3. Handoff tool 被执行
   - 追加 ToolMessage("Successfully transferred to research_expert")
   - 设置 active_agent = "research_expert"
   - 路由 Command(goto="research_expert", graph=Command.PARENT)

4. Research Agent 子图被调用
   输入: state["messages"]（含完整历史）
   输出: Research Agent 的回答被追加到 messages

5. 回到 Supervisor LLM
   输入: 更新后的 state["messages"]
   Supervisor 看到 Research 的结果，决定下一步

6. 可能调用 transfer_to_math_expert
   ... 或决定完成（不再调用任何 handoff tool）
```

**核心洞察**：Supervisor **不是通过 prompt 解析或 JSON 路由**，而是通过标准的 **LLM tool calling** 机制选择 Agent。每个 Agent 就是一个 tool，LLM 的 tool selection 能力直接决定了路由。

#### add_handoff_back_messages 参数

当 Worker Agent 完成后返回 Supervisor 时，可以选择添加「回传消息」：

```python
workflow = create_supervisor(
    [research_agent, math_agent],
    add_handoff_back_messages=True,  # 默认 False
)
# 当设为 True 时，Worker 返回 Supervisor 时会添加：
# AIMessage("转回 supervisor") + ToolMessage("成功转回")
# 这让 Worker Agent 也可以主动请求转交
```

### 3.7 内部机制：大模型会话消息是否共享

#### 答案：是的，完全共享

在 `create_supervisor()` 的默认实现中，**所有 Agent 共享同一个 `messages` 列表**：

```python
# Supervisor 的 StateGraph 使用 MessagesState
# messages: Annotated[list, add_messages]  ← reducer = operator.add

# 每次调用 Worker Agent 时，传入的是完整的 state["messages"]
# Worker Agent 的内部 LLM 能看到：
#   1. 自己的 system prompt
#   2. 用户的原始消息
#   3. Supervisor 的路由决策消息
#   4. 之前其他 Agent 的所有输出
```

**消息可见性时间线**：

```
消息列表增长过程：

[1] HumanMessage("FAANG 公司 2024 总员工数?平方根?")
    → Supervisor 看到: [1]
    → Supervisor 决定: 调用 research_expert

[2] AIMessage(tool_calls=[transfer_to_research_expert])
[3] ToolMessage("Successfully transferred to research_expert")
    → Research Agent 看到: [1, 2, 3]
    → Research Agent 执行搜索, 产出最终回答

[4] AIMessage(content="FAANG 员工数: Meta 67K, Apple 164K, ...")
    → Supervisor 看到: [1, 2, 3, 4]  ← 能看到 Research 的结果
    → Supervisor 决定: 调用 math_expert

[5] AIMessage(tool_calls=[transfer_to_math_expert])
[6] ToolMessage("Successfully transferred to math_expert")
    → Math Agent 看到: [1, 2, 3, 4, 5, 6]  ← 能看到 Research 的结果!
    → Math Agent 执行计算

[7] AIMessage(content="FAANG 总人数 1,977,586，平方根 ≈ 1406.3")
    → Supervisor 看到: [1, 2, 3, 4, 5, 6, 7]
    → Supervisor 决定: 完成（不再调用 tool）
    → 最终输出
```

#### 但每个 Agent 有自己的 System Prompt

虽然消息历史是共享的，但每个 Agent 有独立的 system prompt：

```
Supervisor LLM 看到:
  [SystemMessage("你是团队管理者...")] + [共享 messages]

Research Agent LLM 看到:
  [SystemMessage("你是世界级研究员...")] + [共享 messages]

Math Agent LLM 看到:
  [SystemMessage("你是数学专家...")] + [共享 messages]
```

#### Token 消耗与优化

共享 messages 意味着 **token 消耗随迭代线性增长**。优化策略：

| 策略 | 方法 | 效果 |
|:---|:---|:---|
| `output_mode="last_message"` | 只保留 Worker 最终回答 | 减少 50-70% token |
| `pre_model_hook` | 自定义消息裁剪/摘要 | 可控的 token 预算 |
| `add_handoff_messages=False` | 不记录 handoff 过程消息 | 减少 10-20% token |
| 轻量路由模型 | Supervisor 用小模型 | 降低路由成本 |

### 3.8 适用场景

- 客服系统（Supervisor → 账单 Agent / 技术支持 Agent / 套餐推荐 Agent）
- 研究助手（Supervisor → 网络搜索 Agent / 文档分析 Agent / 数据计算 Agent）
- DevOps 编排（Supervisor → 部署 Agent / 监控 Agent / 告警 Agent）

---

## 4. 模式二：Swarm（群集对等）

### 4.1 架构图

```
    ┌────────────┐    handoff    ┌────────────┐
    │  Agent A   │ ────────────▶ │  Agent B   │
    │ (通用问答)  │               │ (科学专家)  │
    └────────────┘               └────────────┘
          ▲                           │
          │         handoff           │
          └───────────────────────────┘
    ┌────────────┐    handoff    ┌────────────┐
    │  Agent C   │ ◀──────────── │  Agent D   │
    │ (翻译专家)  │               │ (编程专家)  │
    └────────────┘               └────────────┘
```

**特点**: 无中心管理者。每个 Agent 独立决策，通过 `handoff` 工具将控制权移交给最合适的 Agent。类似客服转接。

### 4.2 使用 create_swarm 快速构建

```python
from langgraph.prebuilt import create_react_agent
from langgraph_swarm import create_swarm, create_handoff_tool

# 定义工具
def answer_question(question: str) -> str:
    """回答通用知识问题"""
    return f"答案: {question}"

def science_explain(topic: str) -> str:
    """解释科学概念"""
    return f"科学解释: {topic}"

def translate_text(text: str, target_lang: str) -> str:
    """翻译文本"""
    return f"翻译结果 ({target_lang}): {text}"

# 创建 Agent，每个 Agent 知道可以 handoff 给谁
qa_agent = create_react_agent(
    model=model,
    tools=[
        answer_question,
        create_handoff_tool(agent_name="science_agent"),
        create_handoff_tool(agent_name="translator_agent"),
    ],
    name="question_answering_agent",
    prompt=(
        "你是通用问答 Agent。遇到科学问题，转交给 science_agent。"
        "遇到翻译请求，转交给 translator_agent。只回答通用知识问题。"
    ),
)

science_agent = create_react_agent(
    model=model,
    tools=[
        science_explain,
        create_handoff_tool(agent_name="question_answering_agent"),
        create_handoff_tool(agent_name="translator_agent"),
    ],
    name="science_agent",
    prompt=(
        "你是科学专家，擅长物理、化学、生物、天文。"
        "非科学问题转交给 question_answering_agent。"
        "翻译请求转交给 translator_agent。"
    ),
)

translator_agent = create_react_agent(
    model=model,
    tools=[
        translate_text,
        create_handoff_tool(agent_name="question_answering_agent"),
        create_handoff_tool(agent_name="science_agent"),
    ],
    name="translator_agent",
    prompt=(
        "你是语言翻译专家。非翻译问题转交给 question_answering_agent。"
        "科学问题转交给 science_agent。"
    ),
)

# 创建 Swarm
swarm = create_swarm(
    agents=[qa_agent, science_agent, translator_agent],
    default_active_agent="question_answering_agent",
)

app = swarm.compile(checkpointer=MemorySaver())

# 运行
result = app.invoke({
    "messages": [{"role": "user", "content": "请用中文解释量子纠缠"}]
})
# qa_agent → handoff → science_agent → 回答
```

### 4.3 Swarm vs Supervisor 对比

| 维度 | Supervisor | Swarm |
|:---|:---|:---|
| **控制中心** | 有（Supervisor 集中决策） | 无（Agent 自主决策） |
| **路由方式** | Supervisor 分析后路由 | Agent 自行 handoff |
| **复杂度** | 中（Supervisor 是单点） | 低（无中心节点） |
| **可扩展性** | 新 Agent 需更新 Supervisor prompt | 新 Agent 只需添加 handoff tool |
| **灵活性** | Supervisor 控制流程 | Agent 自主决策更灵活 |
| **可预测性** | 高（集中路由可追踪） | 中（handoff 路径不可预知） |
| **适用场景** | 明确的部门分工 | 扁平化团队协作 |

---

## 5. 模式三：Hierarchical（层级管理）

### 5.1 架构图

```
                    ┌──────────────────┐
                    │  Top Supervisor   │
                    │   （总管理者）     │
                    └────┬────────┬────┘
                         │        │
              ┌──────────┘        └──────────┐
              ▼                              ▼
     ┌────────────────┐            ┌────────────────┐
     │ Sub-Supervisor  │            │ Sub-Supervisor  │
     │  （技术团队）    │            │  （商务团队）    │
     └──┬─────┬────┬──┘            └──┬─────┬────┬──┘
        │     │    │                  │     │    │
        ▼     ▼    ▼                  ▼     ▼    ▼
      Dev   QA  DevOps           Sales  Billing  Legal
      Agent Agent  Agent         Agent  Agent    Agent
```

**特点**: 多层 Supervisor，每层管理一个子团队。顶层 Supervisor 管理子 Supervisor，子 Supervisor 管理具体的 Worker Agent。

### 5.2 实现

```python
from langgraph.graph import StateGraph, MessagesState, START, END
from langgraph.types import Command
from typing import Literal

# ─── 底层 Worker Agent ───

code_agent = create_react_agent(model, [write_code_tool], name="code_agent")
test_agent = create_react_agent(model, [run_tests_tool], name="test_agent")
deploy_agent = create_react_agent(model, [deploy_tool], name="deploy_agent")

sales_agent = create_react_agent(model, [crm_tool], name="sales_agent")
billing_agent = create_react_agent(model, [payment_tool], name="billing_agent")

# ─── 中层 Sub-Supervisor ───

tech_supervisor = create_supervisor(
    [code_agent, test_agent, deploy_agent],
    model=model,
    prompt="你是技术团队管理者。管理开发、测试和部署 Agent。"
)

biz_supervisor = create_supervisor(
    [sales_agent, billing_agent],
    model=model,
    prompt="你是商务团队管理者。管理销售和账单 Agent。"
)

# ─── 顶层 Supervisor ───

top_supervisor = create_supervisor(
    [tech_supervisor, biz_supervisor],
    model=model,
    prompt=(
        "你是 CEO，管理技术团队和商务团队。"
        "技术问题交给 tech_supervisor。"
        "商务问题交给 biz_supervisor。"
    )
)

app = top_supervisor.compile()
```

### 5.3 适用场景

- 大型企业应用（部门 → 团队 → 个人）
- 复杂产品开发（产品经理 → 技术主管 → 开发/测试/运维）
- 多层级客服系统（前台 → 部门 → 专员）

---

## 6. 模式四：Agent-as-Tool（工具化代理）

### 6.1 架构图

```
                ┌──────────────────┐
                │   Outer Agent    │
                │  （主控 Agent）   │
                └──┬──────────┬────┘
                   │          │
            ┌──────┘          └──────┐
            ▼                        ▼
    ┌──────────────┐        ┌──────────────┐
    │ ask_fruit_   │        │ ask_veggie_  │
    │ expert()     │        │ expert()     │
    │  (tool)      │        │  (tool)      │
    └──────┬───────┘        └──────┬───────┘
           ▼                       ▼
    ┌──────────────┐        ┌──────────────┐
    │ Fruit Agent  │        │ Veggie Agent │
    │ (内部 Agent) │        │ (内部 Agent) │
    └──────────────┘        └──────────────┘
```

**特点**: 将整个 Agent 包装成一个 Tool，对外暴露简单的函数接口。主 Agent 通过 Tool 调用的方式使用子 Agent。

### 6.2 实现

```python
from langchain.tools import tool
from langgraph.prebuilt import create_react_agent
from langgraph.checkpoint.memory import MemorySaver

# ─── Step 1: 创建专业 Agent ───

def fruit_info(fruit_name: str) -> str:
    """查询水果信息"""
    return f"{fruit_name}: 富含维生素 C，产地热带"

fruit_agent = create_react_agent(
    model=model,
    tools=[fruit_info],
    name="fruit_agent",
    prompt="你是水果专家，回答关于水果的问题。",
    checkpointer=MemorySaver(),
)

def veggie_info(veggie_name: str) -> str:
    """查询蔬菜信息"""
    return f"{veggie_name}: 富含纤维素，健康蔬菜"

veggie_agent = create_react_agent(
    model=model,
    tools=[veggie_info],
    name="veggie_agent",
    prompt="你是蔬菜专家，回答关于蔬菜的问题。",
    checkpointer=MemorySaver(),
)

# ─── Step 2: 将 Agent 包装为 Tool ───

@tool
def ask_fruit_expert(question: str) -> str:
    """向水果专家提问。用于所有水果相关的问题。"""
    response = fruit_agent.invoke(
        {"messages": [{"role": "user", "content": question}]},
    )
    return response["messages"][-1].content

@tool
def ask_veggie_expert(question: str) -> str:
    """向蔬菜专家提问。用于所有蔬菜相关的问题。"""
    response = veggie_agent.invoke(
        {"messages": [{"role": "user", "content": question}]},
    )
    return response["messages"][-1].content

# ─── Step 3: 创建主 Agent ───

outer_agent = create_react_agent(
    model=model,
    tools=[ask_fruit_expert, ask_veggie_expert],
    prompt=(
        "你有两个专家：ask_fruit_expert 和 ask_veggie_expert。"
        "总是将问题转交给合适的专家。"
    ),
    checkpointer=MemorySaver(),
)

# ─── 运行 ───

result = outer_agent.invoke({
    "messages": [{"role": "user", "content": "苹果和胡萝卜哪个更健康？"}]
})
# outer_agent → ask_fruit_expert("苹果") + ask_veggie_expert("胡萝卜") → 汇总回答
```

### 6.3 注意事项

- **Tool 限流**: 使用 `ToolCallLimitMiddleware` 防止无限循环调用
- **命名空间隔离**: 每个子 Agent 需要唯一的 `name`，避免状态冲突
- **状态隔离**: 子 Agent 有独立的状态，通过 Tool 接口通信

```python
from langgraph.prebuilt import ToolCallLimitMiddleware

outer_agent = create_react_agent(
    model=model,
    tools=[ask_fruit_expert, ask_veggie_expert],
    middleware=[
        ToolCallLimitMiddleware(tool_name="ask_fruit_expert", run_limit=3),
        ToolCallLimitMiddleware(tool_name="ask_veggie_expert", run_limit=3),
    ],
)
```

### 6.4 适用场景

- 将已有 Agent 集成到新系统中
- 跨团队/跨部门 Agent 复用
- 第三方 Agent 服务封装

---

## 7. 模式五：Orchestrator-Worker / MapReduce

### 7.1 架构图

```
        ┌──────────────────┐
        │   Orchestrator    │
        │  （规划 + 分配）   │
        └──────┬────────────┘
               │ Send()
        ┌──────┼──────┐
        ▼      ▼      ▼
   ┌────────┐┌────────┐┌────────┐
   │Worker 1││Worker 2││Worker 3│
   │(并行)   ││(并行)   ││(并行)   │
   └───┬────┘└───┬────┘└───┬────┘
       │         │         │
       └─────────┼─────────┘
                 ▼
        ┌──────────────────┐
        │   Synthesizer    │
        │   （汇总结果）    │
        └──────────────────┘
```

**特点**: Orchestrator 将任务拆分为多个子任务，通过 `Send` API 动态创建并行 Worker，最终由 Synthesizer 汇总结果。

### 7.2 完整实现

```python
import operator
from typing import Annotated, List
from pydantic import BaseModel, Field
from langgraph.graph import StateGraph, START, END
from langgraph.types import Send
from langchain.messages import SystemMessage, HumanMessage

# ─── 数据模型 ───

class Section(BaseModel):
    name: str = Field(description="报告段落名称")
    description: str = Field(description="段落内容简介")

class Sections(BaseModel):
    sections: List[Section] = Field(description="报告段落列表")

# ─── 状态定义 ───

class State(TypedDict):
    topic: str                          # 报告主题
    sections: list[Section]             # 段落列表
    completed_sections: Annotated[list, operator.add]  # 完成的段落（reducer 自动合并）

class WorkerState(TypedDict):
    section: Section                    # 单个段落（Worker 独立状态）

# ─── 节点 ───

planner = llm.with_structured_output(Sections)

def orchestrator(state: State):
    """规划报告结构"""
    report_sections = planner.invoke([
        SystemMessage(content="生成报告大纲。"),
        HumanMessage(content=f"报告主题: {state['topic']}")
    ])
    return {"sections": report_sections.sections}

def llm_worker(state: WorkerState):
    """Worker: 撰写单个段落"""
    section = state["section"]
    result = llm.invoke([
        SystemMessage(content="撰写报告段落。要详细、有深度。"),
        HumanMessage(content=f"段落名: {section.name}\n要求: {section.description}")
    ])
    return {"completed_sections": [result.content]}

def assign_workers(state: State):
    """动态创建 Worker，每个 section 一个"""
    return [Send("llm_worker", {"section": s}) for s in state["sections"]]

def synthesizer(state: State):
    """汇总所有段落"""
    return {"final_report": "\n\n---\n\n".join(state["completed_sections"])}

# ─── 构建图 ───

builder = StateGraph(State)

builder.add_node("orchestrator", orchestrator)
builder.add_node("llm_worker", llm_worker)
builder.add_node("synthesizer", synthesizer)

builder.add_edge(START, "orchestrator")
builder.add_conditional_edges("orchestrator", assign_workers, ["llm_worker"])
builder.add_edge("llm_worker", "synthesizer")
builder.add_edge("synthesizer", END)

graph = builder.compile()

# ─── 运行 ───

result = graph.invoke({"topic": "AI Agent 的未来发展趋势"})
print(result["final_report"])
```

### 7.3 关键机制

1. **Send API**: `Send("node", state)` 动态创建节点实例，每个有独立状态
2. **Reducer**: `Annotated[list, operator.add]` 自动合并所有 Worker 的输出
3. **并行执行**: 所有 Worker 并行运行，完成后统一进入 Synthesizer

### 7.4 适用场景

- 报告生成（拆分章节 → 并行撰写 → 汇总）
- 批量数据处理（拆分数据 → 并行处理 → 合并结果）
- 多源研究（多角度并行搜索 → 汇总分析）

---

## 8. 模式六：Network / P2P（对等网络）

### 8.1 架构图

```
    ┌──────────┐   Command   ┌──────────┐
    │ Agent A  │ ──────────▶ │ Agent B  │
    │(前端专家)│             │(后端专家)│
    └──────────┘             └──────────┘
         ▲                        │
         │         Command        │
         │◀───────────────────────┘
    ┌──────────┐   Command   ┌──────────┐
    │ Agent D  │◀────────────│ Agent C  │
    │(测试专家)│             │(DevOps)  │
    └──────────┘             └──────────┘
```

**特点**: 无中心节点，Agent 之间通过 `Command(goto=...)` 直接通信。每个 Agent 知道其他 Agent 的能力，自主决定移交目标。

### 8.2 实现

```python
from langgraph.graph import StateGraph, MessagesState, START, END
from langgraph.types import Command
from typing import Literal

class DevState(TypedDict):
    messages: Annotated[list, operator.add]
    current_task: str

def frontend_agent(state: DevState) -> Command[Literal["backend_agent", "test_agent", "__end__"]]:
    result = llm.invoke([
        {"role": "system", "content": "你是前端专家。完成前端任务后决定下一步。"},
        *state["messages"]
    ])

    # 根据任务进展决定移交给谁
    if "需要 API" in result.content:
        return Command(update={"messages": [result]}, goto="backend_agent")
    elif "需要测试" in result.content:
        return Command(update={"messages": [result]}, goto="test_agent")
    else:
        return Command(update={"messages": [result]}, goto="__end__")

def backend_agent(state: DevState) -> Command[Literal["frontend_agent", "devops_agent", "__end__"]]:
    result = llm.invoke([
        {"role": "system", "content": "你是后端专家。完成后决定下一步。"},
        *state["messages"]
    ])

    if "需要部署" in result.content:
        return Command(update={"messages": [result]}, goto="devops_agent")
    elif "需要前端调整" in result.content:
        return Command(update={"messages": [result]}, goto="frontend_agent")
    else:
        return Command(update={"messages": [result]}, goto="__end__")

def devops_agent(state: DevState) -> Command[Literal["test_agent", "__end__"]]:
    result = llm.invoke([
        {"role": "system", "content": "你是 DevOps 专家。部署完成后交给测试。"},
        *state["messages"]
    ])
    return Command(update={"messages": [result]}, goto="test_agent")

def test_agent(state: DevState) -> Command[Literal["frontend_agent", "backend_agent", "__end__"]]:
    result = llm.invoke([
        {"role": "system", "content": "你是测试专家。测试通过则结束，失败则退回开发。"},
        *state["messages"]
    ])

    if "前端 bug" in result.content:
        return Command(update={"messages": [result]}, goto="frontend_agent")
    elif "后端 bug" in result.content:
        return Command(update={"messages": [result]}, goto="backend_agent")
    else:
        return Command(update={"messages": [result]}, goto="__end__")

# 构建图
builder = StateGraph(DevState)
builder.add_node("frontend_agent", frontend_agent)
builder.add_node("backend_agent", backend_agent)
builder.add_node("devops_agent", devops_agent)
builder.add_node("test_agent", test_agent)
builder.add_edge(START, "frontend_agent")

graph = builder.compile()
```

### 8.3 适用场景

- 软件开发流程（前端 ↔ 后端 ↔ 测试 ↔ 运维）
- 自适应工作流（Agent 根据结果动态决定下一步）
- 无固定流程的探索性任务

---

## 9. 状态共享与隔离

### 9.1 核心问题

多 Agent 系统中，Agent 之间如何共享数据是最关键的设计决策：

```
┌─────────────────────────────────────────┐
│            状态共享策略                   │
│                                          │
│  共享状态 ─────── 所有 Agent 读写同一 State │
│  隔离状态 ─────── 每个 Agent 有独立 State   │
│  混合模式 ─────── 共享 messages，私有 context│
└─────────────────────────────────────────┘
```

### 9.2 共享状态模式

最常见的模式 — 所有 Agent 共享 `messages` 列表：

```python
from langgraph.graph import MessagesState

# MessagesState 内置了 messages 的 reducer (operator.add)
# 所有 Agent 写入的 message 自动追加到列表

builder = StateGraph(MessagesState)
builder.add_node("agent_a", agent_a_node)  # 共享 messages
builder.add_node("agent_b", agent_b_node)  # 共享 messages
```

### 9.3 命名空间隔离

当多个子图共存时，必须确保命名空间隔离：

```python
from langgraph.graph import MessagesState, StateGraph

def create_sub_agent(model, *, name, **kwargs):
    """用唯一的节点名包装 Agent，确保命名空间隔离"""
    agent = create_agent(model=model, name=name, **kwargs)
    return (
        StateGraph(MessagesState)
        .add_node(name, agent)          # 唯一的节点名
        .add_edge("__start__", name)
        .compile()
    )

# 两个独立 Agent，命名空间不冲突
fruit_agent = create_sub_agent("gpt-4.1-mini", name="fruit_agent", tools=[fruit_info])
veggie_agent = create_sub_agent("gpt-4.1-mini", name="veggie_agent", tools=[veggie_info])
```

### 9.4 Reducer 的重要性

当子图向父图的共享 key 写入数据时，**必须定义 Reducer**：

```python
import operator
from typing import Annotated

class State(TypedDict):
    # 必须：定义 reducer，否则子图写入会覆盖而非追加
    messages: Annotated[list, operator.add]

# 子图通过 Command.PARENT 更新父图状态
def subgraph_node(state: State):
    return Command(
        update={"messages": [new_message]},  # reducer 自动追加
        goto="next_node",
        graph=Command.PARENT
    )
```

---

## 10. 最佳实践

### 10.1 选择合适的模式

| 需求 | 推荐模式 | 理由 |
|:---|:---|:---|
| 明确的部门分工 | Supervisor | 集中控制，路由清晰 |
| 扁平化团队协作 | Swarm | 自主移交，无需中心 |
| 大规模分层组织 | Hierarchical | 多层管理，职责分明 |
| 复用已有 Agent | Agent-as-Tool | 封装为 Tool，即插即用 |
| 并行处理大量子任务 | Orchestrator-Worker | Send API 动态并行 |
| 自适应工作流 | Network/P2P | Agent 自主决策路由 |

### 10.2 通用最佳实践

1. **明确的 Agent 职责**: 每个 Agent 应有清晰的 system prompt，说明它能做什么、不能做什么
2. **限制工具数量**: 每个 Agent 的工具不超过 5-10 个，避免 LLM 选择困难
3. **命名空间隔离**: 多个子图时，确保节点名唯一
4. **使用 Checkpointer**: 长时间运行的多 Agent 系统必须有持久化
5. **限流保护**: 使用 `ToolCallLimitMiddleware` 防止无限循环
6. **add_handoff_back_messages**: Supervisor 模式启用此选项，让 Worker Agent 可以请求转接
7. **共享 messages**: 大多数场景下共享 `messages` 是最简单的通信方式
8. **最小状态**: 只在 State 中放必要的 key，避免状态膨胀

### 10.3 性能优化

1. **并行执行**: 利用 Send API 和 Pregel 超级步自动并行
2. **轻量模型**: 路由/分类任务用小模型（gpt-4.1-mini），核心推理用大模型
3. **缓存工具结果**: 相同查询不重复调用工具
4. **流式输出**: 使用 `astream()` 减少用户等待时间

---

## 11. 反模式与常见陷阱

### 11.1 无限循环

**问题**: Agent A → B → A → B ... 无限循环

**解决**:
```python
# 方案 1: ToolCallLimitMiddleware
middleware=[ToolCallLimitMiddleware(tool_name="agent_b", run_limit=3)]

# 方案 2: 状态中记录迭代次数
class State(TypedDict):
    messages: Annotated[list, operator.add]
    handoff_count: int  # 超过阈值强制结束
```

### 11.2 状态爆炸

**问题**: 所有 Agent 共享一个巨大的 state，随着对话增长无限膨胀

**解决**:
```python
# 使用 message trimming
from langgraph.graph import add_messages

class State(TypedDict):
    messages: Annotated[list, add_messages]  # add_messages 支持 pruning
```

### 11.3 过度路由

**问题**: Supervisor 模式下，Supervisor 本身成为瓶颈，每个请求都要经过 Supervisor

**解决**: 考虑 Swarm 模式让 Agent 直接移交，或在 Supervisor 中缓存路由决策

### 11.4 忽略错误处理

**问题**: 子 Agent 抛出异常，整个系统崩溃

**解决**: 在每个 Agent 包装节点中加入 try-catch，返回错误消息而非崩溃

---

## 12. 与 flow-run 的对比

| 维度 | LangGraph 多 Agent | flow-run |
|:---|:---|:---|
| **Agent 间通信** | Command / handoff / 共享 messages | 模板表达式 `${{ steps.id.output }}` |
| **动态路由** | Python 函数自由决策 | YAML expression 有限表达 |
| **并行 Worker** | Send API 动态创建 | parallel 步骤 + max_concurrent |
| **层级管理** | 子图嵌套 + Command.PARENT | 子工作流 YAML 引用 |
| **Agent 自主性** | 高（Agent 自行决策路由） | 低（YAML 预定义流程） |
| **编排复杂度** | 高（需要编写大量代码） | 低（YAML 声明即可） |
| **适用场景** | AI Agent 推理协作 | DevOps/CI/CD 自动化 |

**互补建议**: LangGraph 负责 Agent 推理和决策层，flow-run 负责具体的操作执行层。

---

## 附录：模式选择决策树

```
需要多 Agent 协作？
├── 是否有明确的部门/角色分工？
│   ├── 是 → 是否需要多层级管理？
│   │   ├── 是 → Hierarchical（层级管理）
│   │   └── 否 → Supervisor（管理者）
│   └── 否 → Agent 之间是否对等协作？
│       ├── 是 → Swarm（群集对等）
│       └── 否 → 是否需要并行处理子任务？
│           ├── 是 → Orchestrator-Worker（MapReduce）
│           └── 否 → Network/P2P（对等网络）
└── 是否需要复用已有 Agent？
    └── 是 → Agent-as-Tool（工具化代理）
```

---

## 参考资源

- [LangGraph 官方文档 — Multi-Agent](https://langchain-ai.github.io/langgraph/concepts/multi_agent/)
- [LangGraph Supervisor 模式](https://github.com/langchain-ai/langgraph/blob/main/libs/langgraph-supervisor/)
- [LangGraph Swarm 模式](https://github.com/langchain-ai/langgraph/blob/main/libs/langgraph-swarm/)
- [LangGraph Workflows and Agents](https://docs.langchain.com/oss/python/langgraph/workflows-agents)
- [AWS — Build multi-agent systems with LangGraph](https://aws.amazon.com/blogs/machine-learning/build-multi-agent-systems-with-langgraph-and-amazon-bedrock/)

---

*文档版本: v2.0*
*最后更新: 2026-04-11*
*新增: Supervisor 内部机制详解（Agent 通信 / 创建 / 路由 / 消息共享）*
