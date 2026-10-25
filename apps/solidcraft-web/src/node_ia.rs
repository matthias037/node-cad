//! What NØDE IA needs from the browser: `fetch` to the site that served the page, and a place
//! for the account's token (the page's local storage: the browser has no keychain).

use std::rc::Rc;

use solidcraft_ui_egui::agent_chat::backend::{Fetch, HttpAnswer, HttpRequest, Secrets};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// The browser's `fetch`, answering through the callback and waking the window.
pub fn fetch(ctx: &egui::Context) -> Fetch {
    let ctx = ctx.clone();
    Rc::new(move |req: HttpRequest, done: Box<dyn FnOnce(HttpAnswer)>| {
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            done(send(req).await);
            ctx.request_repaint();
        });
    })
}

fn said(e: JsValue) -> String {
    e.as_string().or_else(|| e.dyn_ref::<js_sys::Error>().map(|e| String::from(e.message()))).unwrap_or_else(|| "the request failed".into())
}

async fn send(req: HttpRequest) -> HttpAnswer {
    let window = web_sys::window().ok_or("no window")?;
    let init = web_sys::RequestInit::new();
    init.set_method(req.method);
    // The account goes as its bearer token, never as the site's cookies.
    init.set_credentials(web_sys::RequestCredentials::Omit);
    let headers = web_sys::Headers::new().map_err(said)?;
    for (k, v) in &req.headers {
        headers.set(k, v).map_err(said)?;
    }
    init.set_headers(&headers);
    if let Some(body) = &req.body {
        init.set_body(&js_sys::Uint8Array::from(body.as_slice()));
    }
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str_and_init(&req.url, &init)).await.map_err(said)?.dyn_into().map_err(said)?;
    let status = response.status();
    let buffer = JsFuture::from(response.array_buffer().map_err(said)?).await.map_err(said)?;
    Ok((status, js_sys::Uint8Array::new(&buffer).to_vec()))
}

/// The account's token in the page's local storage, under the agent's id.
pub struct LocalSecrets;

fn storage() -> Result<web_sys::Storage, String> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten()).ok_or_else(|| "this browser keeps nothing for the page".into())
}

fn key(id: &str) -> String {
    format!("node-cad.secret.{id}")
}

impl Secrets for LocalSecrets {
    fn get(&self, id: &str) -> Result<Option<String>, String> {
        storage()?.get_item(&key(id)).map_err(said)
    }
    fn set(&self, id: &str, secret: &str) -> Result<(), String> {
        storage()?.set_item(&key(id), secret).map_err(said)
    }
    fn delete(&self, id: &str) -> Result<(), String> {
        storage()?.remove_item(&key(id)).map_err(said)
    }
}
