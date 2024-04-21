//! The text agent is an `<input>` element used to trigger
//! mobile keyboard and IME input.

use std::{cell::Cell, rc::Rc, sync::Mutex};

#[allow(unused_imports)]
use bevy::log;
use bevy::{
    prelude::{EventWriter, Res, Resource},
    window::RequestRedraw,
};
use crossbeam_channel::Sender;

use egui::Pos2;
use once_cell::sync::Lazy;
use wasm_bindgen::prelude::*;

use crate::systems::ContextSystemParams;

static AGENT_ID: &str = "egui_text_agent";

#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VirtualTouchInfo {
    pub editing_text: bool,
    pub touch_pos: Option<Pos2>,
}

pub static VIRTUAL_KEYBOARD_GLOBAL: Lazy<Mutex<VirtualTouchInfo>> =
    Lazy::new(|| Mutex::new(VirtualTouchInfo::default()));

#[derive(Resource)]
pub struct TextAgentChannel {
    pub sender: crossbeam_channel::Sender<egui::Event>,
    pub receiver: crossbeam_channel::Receiver<egui::Event>,
}

impl Default for TextAgentChannel {
    fn default() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self { sender, receiver }
    }
}

pub fn propagate_text(
    channel: Res<TextAgentChannel>,
    mut context_params: ContextSystemParams,
    mut redraw_event: EventWriter<RequestRedraw>,
) {
    for mut contexts in context_params.contexts.iter_mut() {
        if contexts.egui_input.focused {
            let mut redraw = false;
            while let Ok(r) = channel.receiver.try_recv() {
                redraw = true;
                contexts.egui_input.events.push(r);
            }
            if redraw {
                redraw_event.send(RequestRedraw);
            }
            break;
        }
    }
}

fn text_agent() -> web_sys::HtmlInputElement {
    web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .get_element_by_id(AGENT_ID)
        .unwrap()
        .dyn_into()
        .unwrap()
}

/// Text event handler,
pub fn install_text_agent(sender: Sender<egui::Event>) -> Result<(), JsValue> {
    let window = web_sys::window().unwrap();
    let document = window.document().unwrap();
    let body = document.body().expect("document should have a body");
    let input = document
        .create_element("input")?
        .dyn_into::<web_sys::HtmlInputElement>()?;
    let input = std::rc::Rc::new(input);
    input.set_id(AGENT_ID);
    let is_composing = Rc::new(Cell::new(false));
    {
        let style = input.style();
        // Transparent
        style.set_property("opacity", "0").unwrap();
        // Hide under canvas
        style.set_property("z-index", "-1").unwrap();

        style.set_property("position", "absolute")?;
        style.set_property("top", "0px")?;
        style.set_property("left", "0px")?;
    }
    // Set size as small as possible, in case user may click on it.
    input.set_size(1);
    input.set_autofocus(true);
    input.set_hidden(true);

    {
        // When IME is off
        let input_clone = input.clone();
        let sender_clone = sender.clone();
        let is_composing = is_composing.clone();
        let on_input = Closure::wrap(Box::new(move |_event: web_sys::InputEvent| {
            let text = input_clone.value();
            if !text.is_empty() && !is_composing.get() {
                input_clone.set_value("");
                if text.len() == 1 {
                    let _ = sender_clone.send(egui::Event::Text(text));
                }
            }
        }) as Box<dyn FnMut(_)>);
        input.add_event_listener_with_callback("input", on_input.as_ref().unchecked_ref())?;
        on_input.forget();
    }

    body.append_child(&input)?;

    Ok(())
}

pub fn virtual_keyboard_handler() {
    let document = web_sys::window().unwrap().document().unwrap();
    {
        let closure = Closure::wrap(Box::new(move |_event: web_sys::TouchEvent| {
            let touch_info = VIRTUAL_KEYBOARD_GLOBAL.lock().unwrap();
            update_text_agent(touch_info.editing_text, touch_info.touch_pos);
        }) as Box<dyn FnMut(_)>);
        document
            .add_event_listener_with_callback("touchstart", closure.as_ref().unchecked_ref())
            .unwrap();
        closure.forget();
    }
}

/// Focus or blur text agent to toggle mobile keyboard.
fn update_text_agent(editing_text: bool, maybe_touch_pos: Option<egui::Pos2>) {
    use web_sys::HtmlInputElement;

    let window = match web_sys::window() {
        Some(window) => window,
        None => {
            bevy::log::error!("No window found");
            return;
        }
    };
    let document = match window.document() {
        Some(doc) => doc,
        None => {
            bevy::log::error!("No document found");
            return;
        }
    };
    let input: HtmlInputElement = match document.get_element_by_id(AGENT_ID) {
        Some(ele) => ele,
        None => {
            bevy::log::error!("Agent element not found");
            return;
        }
    }
    .dyn_into()
    .unwrap();

    if editing_text {
        let is_already_editing = input.hidden();

        if is_already_editing && maybe_touch_pos.is_some() {
            input.set_hidden(false);
            match input.focus().ok() {
                Some(_) => {}
                None => {
                    bevy::log::error!("Unable to set focus");
                }
            }
        } else {
            if input.blur().is_err() {
                bevy::log::error!("Agent element not found");
                return;
            }

            input.set_hidden(true);
        }
    } else {
        if input.blur().is_err() {
            bevy::log::error!("Agent element not found");
            return;
        }

        input.set_hidden(true);
    }
}

/// If context is running under mobile device?
fn is_mobile() -> Option<bool> {
    const MOBILE_DEVICE: [&str; 6] = ["Android", "iPhone", "iPad", "iPod", "webOS", "BlackBerry"];

    let user_agent = web_sys::window()?.navigator().user_agent().ok()?;
    let is_mobile = MOBILE_DEVICE.iter().any(|&name| user_agent.contains(name));
    Some(is_mobile)
}

// Move text agent to text cursor's position, on desktop/laptop,
// candidate window moves following text element (agent),
// so it appears that the IME candidate window moves with text cursor.
// On mobile devices, there is no need to do that.
pub fn move_text_cursor(ime: Option<egui::output::IMEOutput>) -> Option<()> {
    let style = text_agent().style();
    // Note: moving agent on mobile devices will lead to unpredictable scroll.
    if is_mobile() == Some(false) {
        ime.as_ref().and_then(|ime| {
            let egui::Pos2 { x, y } = ime.cursor_rect.left_top();
            let document = web_sys::window()?.document()?;
            let canvas = match document.query_selector("canvas") {
                Ok(Some(canvas)) => canvas,
                _ => {
                    bevy::log::error!("No canvas found");
                    return None;
                }
            };
            let canvas = canvas.dyn_into::<web_sys::HtmlCanvasElement>().ok()?;
            let bounding_rect = text_agent().get_bounding_client_rect();
            let y = (y + (canvas.scroll_top() + canvas.offset_top()) as f32)
                .min(canvas.client_height() as f32 - bounding_rect.height() as f32);
            let x = (x + (canvas.scroll_left() + canvas.offset_left()) as f32)
                .min(canvas.client_width() as f32 - bounding_rect.width() as f32);
            style.set_property("position", "absolute").ok()?;
            style.set_property("top", &format!("{}px", y)).ok()?;
            style.set_property("left", &format!("{}px", x)).ok()
        })
    } else {
        style.set_property("position", "absolute").ok()?;
        style.set_property("top", "0px").ok()?;
        style.set_property("left", "0px").ok()
    }
}
