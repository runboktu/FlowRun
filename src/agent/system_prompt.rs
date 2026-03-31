//! 系统提示词渲染

/// 默认系统提示模板
pub const DEFAULT_SYSTEM_PROMPT: &str = r#"你需要解决一个问题。为此，你需要将问题分解为多个步骤。对于每个步骤，首先使用 <thought> 思考要做什么，然后使用可用工具之一决定一个 <action>。接着，你将根据你的行动从环境/工具中收到一个 <observation>。持续这个思考和行动的过程，直到你有足够的信息来提供 <final_answer>。

所有步骤请严格使用以下 XML 标签格式输出：
- <question> 用户问题
- <thought> 思考
- <action> 采取的工具操作
- <observation> 工具或环境返回的结果
- <final_answer> 最终答案

请严格遵守：
- 你每次回答都必须包括两个标签，第一个是 <thought>，第二个是 <action> 或 <final_answer>
- 输出 <action> 后立即停止生成，等待真实的 <observation>，擅自生成 <observation> 将导致错误

${user_prompt}

本次任务可用工具：
${tool_list}"#;

/// 渲染系统提示词
pub fn render_system_prompt(template: &str, user_prompt: &str, tool_list: &str) -> String {
    template
        .replace("${user_prompt}", user_prompt)
        .replace("${tool_list}", tool_list)
}

/// 使用默认模板渲染系统提示词
pub fn render_default_system_prompt(user_prompt: &str, tool_list: &str) -> String {
    render_system_prompt(DEFAULT_SYSTEM_PROMPT, user_prompt, tool_list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_system_prompt() {
        let template = "Hello ${user_prompt}, welcome to ${tool_list}!";
        let result = render_system_prompt(template, "Alice", "Rust");
        assert_eq!(result, "Hello Alice, welcome to Rust!");
    }

    #[test]
    fn test_render_default_system_prompt() {
        let result = render_default_system_prompt("Do something", "tool1, tool2");
        assert!(result.contains("Do something"));
        assert!(result.contains("tool1, tool2"));
        assert!(!result.contains("${user_prompt}"));
        assert!(!result.contains("${tool_list}"));
    }
}
