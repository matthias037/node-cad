//! What the API backends share, on the desktop and in the browser: the shape of a model's
//! answer and of a tool call, the tool results as the model reads them, and the conversation
//! kept short. No input or output here: the desktop backend (`api_client`) sends its requests
//! from a thread, the browser's (`web_client`) with `fetch`.

use serde_json::{Value, json};

use super::agents::Protocol;
use super::backend::PromptPart;

/// Model requests for one message, at most (each answer with tool calls is one).
pub const MAX_REQUESTS_PER_TURN: usize = 24;
/// Longest tool result handed back to the model.
pub(super) const MAX_TOOL_TEXT: usize = 20_000;
/// Messages kept in the conversation (the system note stays).
pub(super) const MAX_HISTORY: usize = 240;
/// A tool result of an earlier message keeps this many characters: the model rarely needs a
/// past reading again, and each request sends the whole history.
pub(super) const OLD_TOOL_TEXT: usize = 600;
/// Tool results kept whole within a message: the most recent ones.
pub(super) const KEPT_TOOL_RESULTS: usize = 3;
/// Tools the model is not offered.
pub(super) const HIDDEN_TOOLS: &[&str] = &["crash_reports"];

/// An endpoint's refusal as people read it.
pub fn http_error(status: u16, body: &Value) -> String {
    let detail =
        body.pointer("/error/message").or_else(|| body.get("error")).and_then(Value::as_str).unwrap_or("").chars().take(300).collect::<String>();
    let what = match status {
        401 | 403 => crate::i18n::tr("ui.ai_http_key"),
        404 => crate::i18n::tr("ui.ai_http_not_found"),
        429 => crate::i18n::tr("ui.ai_http_busy"),
        _ => "",
    };
    format!(
        "HTTP {status}{}{}",
        if what.is_empty() { String::new() } else { format!(": {what}") },
        if detail.is_empty() { String::new() } else { format!(" ({detail})") }
    )
}

/// A tool call's arguments as an object: the JSON text a model sends (or an object, for servers
/// that send one), or why they cannot be used. Missing or empty arguments are an empty object.
pub(super) fn tool_arguments(raw: Option<&Value>) -> Result<Value, String> {
    let text = match raw {
        None | Some(Value::Null) => return Ok(json!({})),
        Some(v @ Value::Object(_)) => return Ok(v.clone()),
        Some(Value::String(s)) => s.trim(),
        Some(_) => return Err("the tool arguments must be a JSON object".into()),
    };
    if text.is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_str::<Value>(text) {
        Ok(v @ Value::Object(_)) => Ok(v),
        Ok(_) => Err("the tool arguments must be a JSON object".into()),
        Err(e) => Err(format!("the tool arguments were not valid JSON ({e}); send the call again with a JSON object")),
    }
}

/// The message for what the user typed (and the view, as the endpoint's own content blocks).
pub(super) fn user_message(protocol: Protocol, parts: &[PromptPart]) -> Value {
    let images = parts.iter().any(|p| matches!(p, PromptPart::Image { .. }));
    if !images {
        let text: Vec<&str> = parts.iter().filter_map(|p| if let PromptPart::Text(t) = p { Some(t.as_str()) } else { None }).collect();
        return json!({"role": "user", "content": text.join("\n")});
    }
    let content: Vec<Value> = parts
        .iter()
        .filter_map(|p| match (protocol, p) {
            (_, PromptPart::Text(t)) => Some(json!({"type": "text", "text": t})),
            (Protocol::OpenAi, PromptPart::Image { mime, base64 }) => {
                Some(json!({"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{base64}")}}))
            }
            (Protocol::Anthropic, PromptPart::Image { mime, base64 }) => {
                Some(json!({"type": "image", "source": {"type": "base64", "media_type": mime, "data": base64}}))
            }
            // These endpoints take no documents (the chat doesn't send any); the message names the file.
            (_, PromptPart::Document { .. }) => None,
        })
        .collect();
    json!({"role": "user", "content": content})
}

/// Whether a history message starts a turn: a user message that is not only tool results.
pub(super) fn starts_a_turn(message: &Value) -> bool {
    message["role"] == "user" && !message["content"].as_array().is_some_and(|blocks| blocks.iter().any(|b| b["type"] == "tool_result"))
}

/// One tool call the endpoint asked for.
#[derive(Debug)]
pub(super) struct Call {
    pub(super) id: String,
    pub(super) name: String,
    /// The arguments as an object, or why they cannot be used.
    pub(super) arguments: Result<Value, String>,
}

/// An answer from the endpoint, in the protocol's shape.
#[derive(Debug)]
pub(super) struct Reply {
    /// Text to show: `(thought, text)` in the order it came.
    pub(super) text: Vec<(bool, String)>,
    pub(super) calls: Vec<Call>,
    /// The assistant message to keep in the history, in the endpoint's own shape. Tool calls are
    /// capped to the ones the loop answers, so every call kept here gets a result.
    pub(super) assistant: Value,
}

/// The answer in the endpoint's shape.
pub(super) fn reply(protocol: Protocol, body: &Value) -> Reply {
    match protocol {
        Protocol::OpenAi => openai_reply(body),
        Protocol::Anthropic => anthropic_reply(body),
    }
}

pub(super) fn openai_reply(body: &Value) -> Reply {
    let message = body.pointer("/choices/0/message").cloned().unwrap_or(Value::Null);
    let text = message.get("content").and_then(Value::as_str).filter(|t| !t.is_empty()).map(|t| vec![(false, t.to_string())]).unwrap_or_default();
    // No more than the loop answers: every call kept here must get a `tool` message.
    let raw: Vec<Value> = message.get("tool_calls").and_then(Value::as_array).map(|a| a.iter().take(32).cloned().collect()).unwrap_or_default();
    let calls = raw
        .iter()
        .enumerate()
        .map(|(i, c)| Call {
            id: c.get("id").and_then(Value::as_str).map_or_else(|| format!("call-{i}"), str::to_string),
            name: c.pointer("/function/name").and_then(Value::as_str).unwrap_or("").to_string(),
            arguments: tool_arguments(c.pointer("/function/arguments")),
        })
        .collect();
    let mut assistant = json!({"role": "assistant", "content": message.get("content").cloned().unwrap_or(Value::Null)});
    if !raw.is_empty() {
        assistant["tool_calls"] = Value::Array(raw);
    }
    Reply { text, calls, assistant }
}

pub(super) fn anthropic_reply(body: &Value) -> Reply {
    let content = body.get("content").cloned().unwrap_or(Value::Null);
    let mut text = Vec::new();
    let mut calls = Vec::new();
    let assistant = match &content {
        Value::Array(blocks) => {
            // Keep the answer as it came (thinking blocks and signatures included), but leave out
            // tool calls beyond the loop's cap so every `tool_use` kept gets its `tool_result`.
            let mut kept = Vec::new();
            for (i, block) in blocks.iter().take(128).enumerate() {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(t) = block.get("text").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
                            text.push((false, t.to_string()));
                        }
                        kept.push(block.clone());
                    }
                    Some("thinking") => {
                        let thought = block.get("thinking").or_else(|| block.get("text"));
                        if let Some(t) = thought.and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
                            text.push((true, t.to_string()));
                        }
                        kept.push(block.clone());
                    }
                    Some("tool_use") if calls.len() < 32 => {
                        calls.push(Call {
                            id: block.get("id").and_then(Value::as_str).map_or_else(|| format!("toolu-{i}"), str::to_string),
                            name: block.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                            arguments: match block.get("input") {
                                None | Some(Value::Null) => Ok(json!({})),
                                Some(v @ Value::Object(_)) => Ok(v.clone()),
                                Some(_) => Err("the tool arguments must be a JSON object".into()),
                            },
                        });
                        kept.push(block.clone());
                    }
                    Some("tool_use") => {}
                    _ => kept.push(block.clone()),
                }
            }
            json!({"role": "assistant", "content": kept})
        }
        // An endpoint that answers with plain text instead of content blocks.
        Value::String(s) => {
            if !s.trim().is_empty() {
                text.push((false, s.clone()));
            }
            json!({"role": "assistant", "content": content.clone()})
        }
        _ => json!({"role": "assistant", "content": content.clone()}),
    };
    Reply { text, calls, assistant }
}

/// MCP `tools/list` as OpenAI function tools (the hidden ones left out).
pub fn openai_tools(list: &Value) -> Vec<Value> {
    list.get("tools")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(128)
                .filter_map(|t| {
                    let name = t.get("name")?.as_str()?;
                    if HIDDEN_TOOLS.contains(&name) {
                        return None;
                    }
                    let params = t.get("inputSchema").cloned().filter(Value::is_object).unwrap_or(json!({"type": "object", "properties": {}}));
                    Some(json!({"type": "function", "function": {
                        "name": name,
                        "description": t.get("description").and_then(Value::as_str).unwrap_or(""),
                        "parameters": params,
                    }}))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// MCP `tools/list` as Anthropic tools (the hidden ones left out).
pub fn anthropic_tools(list: &Value) -> Vec<Value> {
    list.get("tools")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(128)
                .filter_map(|t| {
                    let name = t.get("name")?.as_str()?;
                    if HIDDEN_TOOLS.contains(&name) {
                        return None;
                    }
                    let schema = t.get("inputSchema").cloned().filter(Value::is_object).unwrap_or(json!({"type": "object", "properties": {}}));
                    Some(json!({
                        "name": name,
                        "description": t.get("description").and_then(Value::as_str).unwrap_or(""),
                        "input_schema": schema,
                    }))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The offered tools' names, whichever shape the endpoint takes.
pub(super) fn tool_names(list: &Value) -> Vec<String> {
    list.get("tools")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(128)
                .filter_map(|t| {
                    let name = t.get("name")?.as_str()?;
                    (!HIDDEN_TOOLS.contains(&name)).then(|| name.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A tool call's result as text for the model, and whether it failed.
/// An unknown tool call that names an engine command: the `execute` tool and its arguments.
/// `{command, params?}` under any name, or a dotted command id as the name with its parameters
/// as the arguments. `None` when the server has no `execute` or the call names no command.
pub(super) fn as_execute(tools: &[String], name: &str, args: &Value) -> Option<(String, Value)> {
    let execute = tools.iter().find(|n| n.rsplit("__").next() == Some("execute"))?.clone();
    if let Some(command) = args.get("command").and_then(Value::as_str) {
        let params = args.get("params").cloned().filter(Value::is_object).unwrap_or_else(|| json!({}));
        return Some((execute, json!({"command": command, "params": params})));
    }
    let id = name.rsplit("__").next().unwrap_or(name);
    let dotted = id.split('.').count() >= 2 && id.split('.').all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
    dotted.then(|| (execute, json!({"command": id, "params": if args.is_object() { args.clone() } else { json!({}) }})))
}

/// The pictures in an MCP tool result: (media type, base64), at most four.
pub(super) fn tool_images(r: &Value) -> Vec<(String, String)> {
    r.get("content")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|c| c.get("type").and_then(Value::as_str) == Some("image"))
                .filter_map(|c| Some((c.get("mimeType").and_then(Value::as_str).unwrap_or("image/png").to_string(), c.get("data")?.as_str()?.to_string())))
                .take(4)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn tool_text(r: &Value) -> (String, bool) {
    let failed = r.get("isError").and_then(Value::as_bool).unwrap_or(false);
    let parts: Vec<String> = r
        .get("content")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(16)
                .map(|c| match c.get("type").and_then(Value::as_str) {
                    Some("text") => c.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
                    Some("image") => "[a picture of the view]".to_string(),
                    _ => String::new(),
                })
                .collect()
        })
        .unwrap_or_default();
    let mut text = parts.join("\n");
    // Compact JSON for the model: the indentation tool servers add for people costs about half
    // the tokens of a design reading and tells the model nothing.
    if let Ok(v) = serde_json::from_str::<Value>(text.trim())
        && (v.is_object() || v.is_array())
    {
        text = v.to_string();
    }
    if text.len() > MAX_TOOL_TEXT {
        let mut cut = MAX_TOOL_TEXT;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("\n…(cut)");
    }
    (text, failed)
}

/// Where the last `keep` messages carrying tool results begin: everything before is cut.
pub(super) fn keep_last_tool_results(history: &[Value], keep: usize) -> usize {
    let carries = |m: &Value| {
        m.get("role").and_then(Value::as_str) == Some("tool")
            || m.get("content")
                .and_then(Value::as_array)
                .is_some_and(|b| b.iter().any(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")))
    };
    let mut seen = 0;
    for (i, m) in history.iter().enumerate().rev() {
        if carries(m) {
            seen += 1;
            if seen == keep {
                return i;
            }
        }
    }
    0
}

/// Cut the tool results already in `history` to `max` characters (OpenAI `tool` messages, and
/// Anthropic `tool_result` blocks): called before a new message, they are past readings.
pub(super) fn shorten_tool_results(history: &mut [Value], max: usize) {
    fn cut(text: &mut Value, max: usize) {
        // A result with pictures: its text alone; the pictures are past views.
        if let Some(blocks) = text.as_array() {
            let words: Vec<&str> = blocks.iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect();
            let pictures = blocks.iter().any(|b| matches!(b.get("type").and_then(Value::as_str), Some("image" | "image_url")));
            *text = Value::String(format!("{}{}", words.join("\n"), if pictures { "\n(the pictures of that step are no longer shown)" } else { "" }));
        }
        if let Some(s) = text.as_str()
            && s.chars().count() > max
        {
            let kept: String = s.chars().take(max).collect();
            *text = Value::String(format!("{kept}…(shortened: a result from an earlier message)"));
        }
    }
    for m in history.iter_mut() {
        if m.get("role").and_then(Value::as_str) == Some("tool") {
            if let Some(c) = m.get_mut("content") {
                cut(c, max);
            }
        } else if let Some(blocks) = m.get_mut("content").and_then(Value::as_array_mut) {
            for b in blocks.iter_mut().filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")) {
                if let Some(c) = b.get_mut("content") {
                    cut(c, max);
                }
            }
        }
    }
}

/// What a tool call card says: the tool and, for `execute` and `batch`, the commands.
pub(super) fn tool_title(name: &str, args: &Value) -> String {
    match name {
        "execute" => format!("execute: {}", args.get("command").and_then(Value::as_str).unwrap_or("?")),
        "batch" => {
            let ids: Vec<&str> = args
                .get("commands")
                .and_then(Value::as_array)
                .map(|c| c.iter().filter_map(|s| s.get("command").and_then(Value::as_str)).take(4).collect())
                .unwrap_or_default();
            format!("batch: {}", ids.join(", "))
        }
        _ => name.to_string(),
    }
}
