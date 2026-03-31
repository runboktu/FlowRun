//! Agent 模块工具函数

use serde_json::json;

/// 构建进度数据 JSON
pub fn build_progress_data(session_id: &str, iteration: usize, data: serde_json::Value) -> String {
    json!({
        "session_id": session_id,
        "iteration": iteration,
        "data": data,
    })
    .to_string()
}

/// 构建 LLM 响应进度数据
pub fn build_llm_response_data(content: &str, success: bool) -> serde_json::Value {
    json!({
        "type": "llm_response",
        "content": content,
        "success": success,
    })
}

/// 规范化响应字符串
pub fn normalize_response(s: &str) -> String {
    s.trim()
        .trim_matches('"')
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\r", "\r")
}

/// 移除反斜杠引号
pub fn remove_backslash_quotes(s: &str) -> String {
    s.replace("\\\"", "\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_progress_data() {
        let data = json!({"type": "llm_call"});
        let result = build_progress_data("session-123", 1, data);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["session_id"], "session-123");
        assert_eq!(parsed["iteration"], 1);
        assert_eq!(parsed["data"]["type"], "llm_call");
    }

    #[test]
    fn test_normalize_response() {
        assert_eq!(normalize_response("  hello  "), "hello");
        assert_eq!(normalize_response("\"hello\""), "hello");
        assert_eq!(normalize_response("line1\\nline2"), "line1\nline2");
        assert_eq!(normalize_response("col1\\tcol2"), "col1\tcol2");
    }

    #[test]
    fn test_remove_backslash_quotes() {
        assert_eq!(
            remove_backslash_quotes("hello \\\"world\\\""),
            "hello \"world\""
        );
    }
}
