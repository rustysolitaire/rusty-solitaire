// Rusty Solitaire - web.rs
//
// The web build's link to the page it runs in (see web/README.md). The game
// draws into the page's <canvas id="canvas">, sized to fill the element
// around it: the whole page, or a frame on a website.
//
// A page with its own controls defines window.rustySolitaireState(state):
// the game then leaves out its menu bar, toolbar and status bar, sends its
// state there whenever it changes, and takes commands (the menu actions, such
// as "undo" or "game:freecell") from window.rustySolitaireAction(id).
//
// Copyright (C) 2026 The Rusty Solitaire authors (Rust port)
// SPDX-License-Identifier: GPL-3.0-or-later

use js_sys::Reflect;
use slint::ComponentHandle;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;

use crate::AppWindow;

/// Size the window to the canvas's parent element, now and whenever that
/// element changes size. Slint on its own keeps the window's preferred size.
pub fn follow_container_size(window: &AppWindow) {
    let Some(container) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("canvas"))
        .and_then(|canvas| canvas.parent_element())
    else {
        return;
    };
    let weak = window.as_weak();
    let fit = {
        let container = container.clone();
        move || {
            let (width, height) = (container.client_width(), container.client_height());
            if let (Some(window), true) = (weak.upgrade(), width > 0 && height > 0) {
                window.window().set_size(slint::LogicalSize::new(width as f32, height as f32));
            }
        }
    };
    fit();
    // The observer and its callback live as long as the page.
    let callback = Closure::<dyn FnMut()>::new(fit);
    if let Ok(observer) = web_sys::ResizeObserver::new(callback.as_ref().unchecked_ref()) {
        observer.observe(&container);
        std::mem::forget(observer);
    }
    callback.forget();
}

#[wasm_bindgen]
extern "C" {
    /// The page's own viewer for the rules (web/index.html), if it has one.
    #[wasm_bindgen(js_namespace = window, js_name = rustySolitaireShowHelp, catch)]
    fn page_show_help(html: &str) -> Result<(), JsValue>;

    /// The page's controls, if it has them.
    #[wasm_bindgen(js_namespace = window, js_name = rustySolitaireState, catch)]
    fn page_state(state: &JsValue) -> Result<(), JsValue>;
}

/// Whether the page has its own controls for the game.
pub fn page_has_controls() -> bool {
    web_sys::window()
        .and_then(|w| Reflect::get(&w, &"rustySolitaireState".into()).ok())
        .is_some_and(|f| f.is_function())
}

/// Run `action` for each command the page sends.
pub fn on_action(action: impl Fn(&str) + 'static) {
    let Some(window) = web_sys::window() else { return };
    let callback = Closure::<dyn Fn(String)>::new(move |id: String| action(&id));
    let _ = Reflect::set(&window, &"rustySolitaireAction".into(), callback.as_ref());
    callback.forget();
}

/// Part or all of the game's state, for the page's controls: a JavaScript
/// object, sent with send().
pub struct State(js_sys::Object);

impl State {
    pub fn new() -> Self {
        State(js_sys::Object::new())
    }

    pub fn set(self, key: &str, value: impl Into<JsValue>) -> Self {
        let _ = Reflect::set(&self.0, &key.into(), &value.into());
        self
    }

    pub fn send(self) {
        let _ = page_state(&self.0);
    }
}

impl From<State> for JsValue {
    fn from(state: State) -> JsValue {
        state.0.into()
    }
}

/// A JavaScript array.
pub fn list<T: Into<JsValue>>(items: impl IntoIterator<Item = T>) -> JsValue {
    items.into_iter().map(Into::into).collect::<js_sys::Array>().into()
}

/// A [value, label] pair, for the page's drop-down lists.
pub fn choice(value: &str, label: &str) -> JsValue {
    list([value, label])
}

/// Show the rules, a self-contained HTML page: in the page's own viewer, or
/// else in a new tab (which pop-up blockers may stop).
pub fn show_help(html: &str) -> Result<(), String> {
    if page_show_help(html).is_ok() {
        return Ok(());
    }
    let parts = js_sys::Array::of1(&html.into());
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("text/html");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &options)
        .map_err(|_| "The help page could not be made.".to_string())?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|_| "The help page could not be made.".to_string())?;
    let opened = web_sys::window().and_then(|w| w.open_with_url_and_target(&url, "_blank").ok().flatten());
    opened.map(|_| ()).ok_or_else(|| "The browser blocked the new tab. Allow pop-ups for this site.".to_string())
}
