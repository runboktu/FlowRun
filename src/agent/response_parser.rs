//! 响应解析器
//!
//! 翻译自 gtht-agent 的 response_parser.hpp/cpp

use crate::agent::types::{ParsedResponse, ParserType};
use crate::agent::util::{normalize_response, remove_backslash_quotes};
use regex::Regex;
use tracing::debug;

/// 响应解析器 trait
pub trait ResponseParser: Send + Sync {
    /// 解析 LLM 响应
    fn parse(&self, response: &str) -> ParsedResponse;

    /// 解析动作字符串
    fn parse_action(&self, action: &str) -> (String, String);
}

/// XML 响应解析器
pub struct XmlResponseParser {
    thought_regex: Regex,
    action_regex: Regex,
    final_answer_regex: Regex,
}

impl XmlResponseParser {
    pub fn new() -> Self {
        Self {
            thought_regex: Regex::new(r"<thought>(?s)(.*?)</thought>").unwrap(),
            action_regex: Regex::new(r"<action>(?s)(.*?)</action>").unwrap(),
            final_answer_regex: Regex::new(r"<final_answer>(?s)(.*?)</final_answer>").unwrap(),
        }
    }

    /// 从字符串中提取最后一个完整的标签内容
    fn extract_last_complete_tag(&self, s: &str, tag_name: &str) -> Option<String> {
        let open_tag = format!("<{}>", tag_name);
        let close_tag = format!("</{}>", tag_name);
        let s_lower = s.to_lowercase();
        let open_lower = open_tag.to_lowercase();
        let close_lower = close_tag.to_lowercase();

        let mut pos = s_lower.rfind(&open_lower);
        while let Some(p) = pos {
            let content_start = p + open_lower.len();
            let search_region = &s_lower[content_start..];
            if let Some(close_pos_relative) = search_region.find(&close_lower) {
                let close_pos = content_start + close_pos_relative;
                let content = &s[content_start..close_pos];
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            if p == 0 {
                break;
            }
            pos = s_lower[..p].rfind(&open_lower);
        }
        None
    }
}

impl Default for XmlResponseParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseParser for XmlResponseParser {
    fn parse(&self, response: &str) -> ParsedResponse {
        let normalized = normalize_response(response);

        debug!(
            "[XmlResponseParser] Parsing response: {}",
            &normalized[..normalized.len().min(200)]
        );

        let thought = self.extract_last_complete_tag(&normalized, "thought");
        let action = self.extract_last_complete_tag(&normalized, "action");
        let final_answer = self.extract_last_complete_tag(&normalized, "final_answer");

        ParsedResponse {
            thought,
            action,
            final_answer,
        }
    }

    fn parse_action(&self, action_str: &str) -> (String, String) {
        let trimmed = remove_backslash_quotes(action_str).trim().to_string();

        debug!(
            "[XmlResponseParser] Parsing action: {}",
            &trimmed[..trimmed.len().min(200)]
        );

        // 尝试 JSON 解析: {"name":"...", "parameters":{...}}
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&trimmed) {
            if let (Some(name), Some(params)) = (json.get("name"), json.get("parameters")) {
                if let (Some(name_str), Ok(params_str)) =
                    (name.as_str(), serde_json::to_string(params))
                {
                    return (name_str.to_string(), params_str);
                }
            }
        }

        (String::new(), String::new())
    }
}

/// JSON 响应解析器
pub struct JsonResponseParser;

impl JsonResponseParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for JsonResponseParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseParser for JsonResponseParser {
    fn parse(&self, response: &str) -> ParsedResponse {
        debug!("[JsonResponseParser] Parsing response");

        match serde_json::from_str::<serde_json::Value>(response) {
            Ok(json) => {
                let thought = json
                    .get("thought")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let final_answer = json
                    .get("final_answer")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let action = json.get("action").and_then(|v| {
                    if let Some(s) = v.as_str() {
                        Some(s.to_string())
                    } else if v.is_object() {
                        let obj = v.as_object()?;
                        let name = obj.get("name")?.as_str()?;
                        let args = obj
                            .get("args")
                            .and_then(|a| serde_json::to_string(a).ok())
                            .unwrap_or_else(|| "[]".to_string());
                        Some(format!("{}({})", name, args))
                    } else {
                        None
                    }
                });

                ParsedResponse {
                    thought,
                    action,
                    final_answer,
                }
            }
            Err(_) => ParsedResponse {
                thought: None,
                action: None,
                final_answer: None,
            },
        }
    }

    fn parse_action(&self, action_str: &str) -> (String, String) {
        let trimmed = remove_backslash_quotes(action_str).trim().to_string();

        // 尝试解析 JSON 格式
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&trimmed) {
            if let (Some(name), Some(params)) = (json.get("name"), json.get("parameters")) {
                if let (Some(name_str), Ok(params_str)) =
                    (name.as_str(), serde_json::to_string(params))
                {
                    return (name_str.to_string(), params_str);
                }
            }
        }

        // 尝试解析函数调用格式: tool_name(args)
        let func_regex = Regex::new(r"(\w+)\((.*)\)").unwrap();
        if let Some(captures) = func_regex.captures(&trimmed) {
            let name = captures
                .get(1)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let args = captures
                .get(2)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            return (name, args);
        }

        (String::new(), String::new())
    }
}

/// 创建解析器
pub fn create_parser(parser_type: ParserType) -> Box<dyn ResponseParser> {
    match parser_type {
        ParserType::Xml => Box::new(XmlResponseParser::new()),
        ParserType::Json => Box::new(JsonResponseParser::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xml_parser_thought_and_action() {
        let parser = XmlResponseParser::new();
        let response = r#"
<thought>I need to find information about the weather.</thought>
<action>{"name":"get_weather","parameters":{"city":"Beijing"}}</action>
"#;
        let result = parser.parse(response);
        assert!(result.thought.is_some());
        assert!(result.action.is_some());
        assert!(result.final_answer.is_none());
        assert!(result.thought.unwrap().contains("weather"));
    }

    #[test]
    fn test_xml_parser_final_answer() {
        let parser = XmlResponseParser::new();
        let response = r#"
<thought>I have all the information needed.</thought>
<final_answer>The weather in Beijing is sunny.</final_answer>
"#;
        let result = parser.parse(response);
        assert!(result.thought.is_some());
        assert!(result.action.is_none());
        assert!(result.final_answer.is_some());
        assert!(result.final_answer.unwrap().contains("sunny"));
    }

    #[test]
    fn test_xml_parser_action_parsing() {
        let parser = XmlResponseParser::new();
        let action = r#"{"name":"read_file","parameters":{"path":"/tmp/test.txt"}}"#;
        let (name, args) = parser.parse_action(action);
        assert_eq!(name, "read_file");
        assert!(args.contains("test.txt"));
    }

    #[test]
    fn test_xml_parser_multiple_tags() {
        let parser = XmlResponseParser::new();
        let response = r#"
<thought>First thought</thought>
<thought>Second thought</thought>
<action>{"name":"tool","parameters":{}}</action>
"#;
        let result = parser.parse(response);
        assert_eq!(result.thought.unwrap(), "Second thought");
    }

    #[test]
    fn test_json_parser() {
        let parser = JsonResponseParser::new();
        let response = r#"{
            "thought": "I need to call a tool",
            "action": {"name": "my_tool", "args": ["arg1", "arg2"]}
        }"#;
        let result = parser.parse(response);
        assert_eq!(result.thought.unwrap(), "I need to call a tool");
        assert!(result.action.is_some());
    }

    #[test]
    fn test_json_parser_final_answer() {
        let parser = JsonResponseParser::new();
        let response = r#"{
            "thought": "Done",
            "final_answer": "The answer is 42"
        }"#;
        let result = parser.parse(response);
        assert_eq!(result.final_answer.unwrap(), "The answer is 42");
    }

    #[test]
    fn test_create_parser() {
        let xml_parser = create_parser(ParserType::Xml);
        let json_parser = create_parser(ParserType::Json);

        let response = r#"<thought>Test</thought><final_answer>Answer</final_answer>"#;
        let xml_result = xml_parser.parse(response);
        assert!(xml_result.final_answer.is_some());
    }
}
