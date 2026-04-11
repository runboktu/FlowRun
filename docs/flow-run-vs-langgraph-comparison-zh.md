# flow-run vs LangGraph 深度对比分析

> 英文版: [flow-run-vs-langgraph-comparison.md](./flow-run-vs-langgraph-comparison.md)

---

## TL;DR

**flow-run 是声明式工作流引擎，LangGraph 是代码式 Agent 编排框架。它们不是直接竞品，而是互补工具。**

- **flow-run 擅长**: 工作流执行（HTTP/Shell/CI/CD）、DevOps 自动化、为 Agent 提供结构化任务执行能力
- **LangGraph 擅长**: AI Agent 推理编排、多 Agent 协作、复杂状态管理、LLM 深度集成

理想架构：**LangGraph 负责「思考」，flow-run 负责「行动」**。

---

## 核心差异一览

| 维度 | flow-run | LangGraph |
|:---|:---|:---|
| **定义方式** | YAML 文件 | Python 代码 |
| **语言** | Rust（性能） | Python（生态） |
| **执行单元** | 步骤（HTTP/Shell/Loop 等） | 节点（Python 函数） |
| **状态管理** | 隐式（模板表达式引用） | 显式（TypedDict + Reducer） |
| **执行模型** | DAG 拓扑排序 | Pregel 超级步 |
| **循环** | 显式 loop 步骤 | 图中的环（天然支持） |
| **检查点** | 文件系统 JSON | 内存/SQLite/Postgres |
| **部署** | 单二进制 CLI | Python 库 / 云平台 |

---

详细对比请参考英文版完整报告：[flow-run-vs-langgraph-comparison.md](./flow-run-vs-langgraph-comparison.md)
