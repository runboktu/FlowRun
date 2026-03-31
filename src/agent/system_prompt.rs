//! 系统提示词渲染

/// 默认系统提示模板
pub const DEFAULT_SYSTEM_PROMPT: &str = r#"你需要解决一个问题。为此，你需要将问题分解为多个步骤。对于每个步骤，首先使用 <thought> 思考要做什么，然后使用可用工具之一决定一个 <action>。接着，你将根据你的行动从环境/工具中收到一个 <observation>。持续这个思考和行动的过程，直到你有足够的信息来提供 <final_answer>。

所有步骤请严格使用以下 XML 标签格式输出：
- <question> 用户问题
- <thought> 思考
- <action> 采取的工具操作
- <observation> 工具或环境返回的结果
- <final_answer> 最终答案

例子 1:

<question>帮我找一个简单的番茄炒蛋食谱，并看看家里的冰箱里有没有西红柿。</question>
<thought>这个任务分两步。第一步，找到番茄炒蛋的食谱。第二步，检查冰箱里是否有西红柿。我先用 find_recipe 工具找食谱。</thought>
<action>{"name":"find_recipe","parameters":{"dish":"番茄炒蛋"}}</action>
<observation>简单的番茄炒蛋食谱：将2个鸡蛋打散，2个番茄切块。热油，先炒鸡蛋，盛出。再热油，炒番茄至软烂，加入鸡蛋，放盐调味即可。</observation>
<thought>好的，我已经有食谱了。食谱需要西红柿。现在我需要用 check_fridge 工具看看冰箱里有没有西红柿。</thought>
<action>{"name":"check_fridge","parameters":{"item":"西红柿"}}</action>
<observation>冰箱检查结果：有3个西红柿。</observation>
<thought>我找到了食谱，并且确认了冰箱里有西红柿。可以回答问题了。</thought>
<final_answer>简单的番茄炒蛋食谱是：鸡蛋打散，番茄切块。先炒鸡蛋，再炒番茄，混合后加盐调味。冰箱里有3个西红柿。</final_answer>


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
