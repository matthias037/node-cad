//! Mistral, NØDE's AI provider, through its OpenAI-compatible chat endpoint, and its OCR for the
//! documents a person attaches: a PDF, a Word or PowerPoint file goes to `POST /v1/ocr`, and the
//! agent receives the pages as Markdown plus the pictures found in them (a scanned plan, a
//! drawing), which the chat model then looks at like any attached picture.
use serde_json::{Value, json};

use super::backend::{HttpFn, HttpRequest, PromptPart};

/// Mistral's API, as the agent settings offer it.
pub const BASE_URL: &str = "https://api.mistral.ai/v1";
/// The chat model a new Mistral agent starts with: it reads pictures and calls tools.
pub const DEFAULT_MODEL: &str = "mistral-large-latest";
/// Mistral's document reader.
pub const OCR_MODEL: &str = "mistral-ocr-latest";
/// A document is read page by page: a long one takes a while.
const OCR_TIMEOUT_S: u64 = 180;
/// Most characters of one document's text handed to the agent.
pub const MAX_OCR_CHARS: usize = 60_000;
/// Most pictures taken out of one document and shown to the agent.
pub const MAX_OCR_PICTURES: usize = 4;
/// A picture from a document larger than this (base64) is left out.
const MAX_OCR_PICTURE_B64: usize = 5_000_000;

/// Whether `base_url` is Mistral's API, whose OCR reads documents.
pub fn is_mistral(base_url: &str) -> bool {
    let u = base_url.trim().to_ascii_lowercase();
    let host = u.trim_start_matches("https://").trim_start_matches("http://");
    host == "api.mistral.ai" || host.starts_with("api.mistral.ai/")
}

/// The OCR request for one document.
pub fn ocr_request(base_url: &str, key: Option<&str>, mime: &str, base64: &str) -> HttpRequest {
    let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
    if let Some(k) = key {
        headers.push(("Authorization".to_string(), format!("Bearer {k}")));
    }
    let body = json!({
        "model": OCR_MODEL,
        "document": {"type": "document_url", "document_url": format!("data:{mime};base64,{base64}")},
        "include_image_base64": true,
    });
    HttpRequest {
        method: "POST",
        url: format!("{}/ocr", base_url.trim_end_matches('/')),
        headers,
        body: Some(body.to_string().into_bytes()),
        timeout_s: OCR_TIMEOUT_S,
    }
}

/// Reads one document with Mistral's OCR: its text, then the pictures in it, as parts of the
/// message. A failure becomes a sentence for the agent, which then tells the person. Desktop only,
/// like the API agents that call it.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_document(http: &HttpFn, base_url: &str, key: Option<&str>, name: &str, mime: &str, base64: &str) -> Vec<PromptPart> {
    let answer = http(&ocr_request(base_url, key, mime, base64)).and_then(|(status, body)| {
        let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        if status == 200 { Ok(v) } else { Err(super::api_client::http_error(status, &v)) }
    });
    match answer {
        Ok(v) => ocr_parts(name, &v),
        Err(e) => vec![PromptPart::Text(format!("\n\nLe document `{name}` n'a pas pu être lu par l'OCR de Mistral : {e}."))],
    }
}

/// The parts for an OCR answer: the pages as Markdown (cut past [`MAX_OCR_CHARS`]), then up to
/// [`MAX_OCR_PICTURES`] of the pictures found in them.
pub fn ocr_parts(name: &str, answer: &Value) -> Vec<PromptPart> {
    let pages = answer.get("pages").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut text = String::new();
    let mut pictures = Vec::new();
    let mut skipped = 0usize;
    for (n, page) in pages.iter().enumerate() {
        let index = page.get("index").and_then(Value::as_u64).map_or(n + 1, |i| i as usize + 1);
        let markdown = page.get("markdown").and_then(Value::as_str).unwrap_or("").trim();
        text += &format!("\n\n### Page {index}\n\n{markdown}");
        for img in page.get("images").and_then(Value::as_array).into_iter().flatten() {
            let Some(data) = img.get("image_base64").and_then(Value::as_str) else { continue };
            match picture(data) {
                Some(part) if pictures.len() < MAX_OCR_PICTURES => pictures.push(part),
                _ => skipped += 1,
            }
        }
    }
    let total = text.chars().count();
    let text = if total > MAX_OCR_CHARS {
        let kept: String = text.chars().take(MAX_OCR_CHARS).collect();
        format!("{kept}\n\n(… {} caractères de plus non transmis)", total - MAX_OCR_CHARS)
    } else {
        text
    };
    let mut head = format!("\n\nDocument `{name}` lu par l'OCR de Mistral ({} page{}).", pages.len(), if pages.len() > 1 { "s" } else { "" });
    if !pictures.is_empty() {
        head += &format!(" Les {} image(s) qu'il contient suivent ce texte.", pictures.len());
    }
    if skipped > 0 {
        head += &format!(" {skipped} autre(s) image(s) non transmise(s).");
    }
    let mut parts = vec![PromptPart::Text(format!("{head}{text}"))];
    parts.extend(pictures);
    parts
}

/// A picture of the OCR answer (a `data:` URL, or bare base64 taken as JPEG) as a message part.
fn picture(data: &str) -> Option<PromptPart> {
    let (mime, base64) = match data.strip_prefix("data:").and_then(|rest| rest.split_once(";base64,")) {
        Some((mime, b64)) => (mime.to_string(), b64),
        None => ("image/jpeg".to_string(), data),
    };
    (mime.starts_with("image/") && !base64.is_empty() && base64.len() <= MAX_OCR_PICTURE_B64)
        .then(|| PromptPart::Image { mime, base64: base64.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mistral_is_recognised_by_its_host_only() {
        assert!(is_mistral("https://api.mistral.ai/v1"));
        assert!(is_mistral("https://API.mistral.ai/v1/"));
        assert!(!is_mistral("https://api.openai.com/v1"));
        assert!(!is_mistral("https://api.mistral.ai.evil.example/v1"));
    }

    #[test]
    fn the_ocr_request_sends_the_document_as_a_data_url() {
        let r = ocr_request("https://api.mistral.ai/v1/", Some("k"), "application/pdf", "QUJD");
        assert_eq!(r.url, "https://api.mistral.ai/v1/ocr");
        assert!(r.headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer k"));
        let body: Value = serde_json::from_slice(r.body.as_deref().unwrap_or_default()).unwrap_or(Value::Null);
        assert_eq!(body["model"], OCR_MODEL);
        assert_eq!(body["document"]["document_url"], "data:application/pdf;base64,QUJD");
        assert_eq!(body["include_image_base64"], true);
    }

    #[test]
    fn pages_become_text_and_their_pictures_follow() {
        let answer = json!({"pages": [
            {"index": 0, "markdown": "# Plan 12-A\nÉpaisseur 8 ±0,1", "images": [{"id": "img-0", "image_base64": "data:image/png;base64,iVBOR"}]},
            {"index": 1, "markdown": "Matière : S235", "images": []},
        ]});
        let parts = ocr_parts("plan.pdf", &answer);
        let PromptPart::Text(t) = &parts[0] else { panic!("text first") };
        assert!(t.contains("`plan.pdf`") && t.contains("2 pages") && t.contains("### Page 1") && t.contains("Épaisseur 8 ±0,1"));
        assert!(t.contains("### Page 2") && t.contains("S235"));
        assert!(matches!(&parts[1], PromptPart::Image { mime, base64 } if mime == "image/png" && base64 == "iVBOR"));
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn a_long_document_is_cut_and_pictures_are_capped() {
        let images: Vec<Value> = (0..7).map(|i| json!({"id": format!("i{i}"), "image_base64": "data:image/jpeg;base64,/9j/"})).collect();
        let answer = json!({"pages": [{"index": 0, "markdown": "x".repeat(MAX_OCR_CHARS + 500), "images": images}]});
        let parts = ocr_parts("long.pdf", &answer);
        let PromptPart::Text(t) = &parts[0] else { panic!("text first") };
        assert!(t.contains("non transmis") && t.contains("3 autre(s) image(s)"));
        assert_eq!(parts.len(), 1 + MAX_OCR_PICTURES);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_refused_document_says_why_to_the_agent() {
        let http: HttpFn = std::sync::Arc::new(|_| Ok((401, br#"{"message":"Unauthorized"}"#.to_vec())));
        let parts = read_document(&http, BASE_URL, Some("bad"), "devis.pdf", "application/pdf", "QUJD");
        let PromptPart::Text(t) = &parts[0] else { panic!("text") };
        assert!(t.contains("`devis.pdf`") && t.contains("HTTP 401"), "{t}");
    }
}
