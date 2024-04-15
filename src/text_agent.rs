//! The text agent is an `<input>` element used to trigger
//! mobile keyboard and IME input.

use std::{cell::Cell, rc::Rc};

#[allow(unused_imports)]
use bevy::log;
use bevy::{
    prelude::{EventWriter, Res, Resource, ResMut},
    window::RequestRedraw,
};
use crossbeam_channel::Sender;
use wasm_bindgen::prelude::*;

use crate::systems::{ContextSystemParams, TouchPos};

static AGENT_ID: &str = "egui_text_agent";

#[derive(Debug)]
pub enum TouchWebEvent {
    Fired,
}

#[derive(Resource)]
pub struct TextAgentChannel {
    pub sender: crossbeam_channel::Sender<(Option<egui::Event>, Option<TouchWebEvent>)>,
    pub receiver: crossbeam_channel::Receiver<(Option<egui::Event>, Option<TouchWebEvent>)>,
}

impl Default for TextAgentChannel {
    fn default() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self { sender, receiver }
    }
}

pub fn propagate_text(
    channel: Res<TextAgentChannel>,
    // pointer_touch_pos: ResMut<TouchPos>,
    mut context_params: ContextSystemParams,
    mut redraw_event: EventWriter<RequestRedraw>,
) {
    for mut contexts in context_params.contexts.iter_mut() {
        if contexts.egui_input.focused {
            let mut redraw = false;
            while let Ok(r) = channel.receiver.try_recv() {
                redraw = true;

                bevy::log::error!("in context handler {:?}", r);
                if let Some(TouchWebEvent::Fired) = r.1 {
                    move_text_cursor(contexts.egui_output.platform_output.ime);
                    let mut editing_text = false;
                    let platform_output = &contexts.egui_output.platform_output;
                    bevy::log::error!("platform_output ime {:?} and mutable text {:?}", platform_output.ime, platform_output.mutable_text_under_cursor);

                    if platform_output.ime.is_some() || platform_output.mutable_text_under_cursor {
                        editing_text = true;
                    }
                    // let maybe_touch_pos = *pointer_touch_pos;
                    // bevy::log::error!("click event, edit text {:?} and pos {:?}", editing_text, maybe_touch_pos);
                    // update_text_agent(editing_text, maybe_touch_pos.0);
                }

                if let Some(e) = r.0 {
                    contexts.egui_input.events.push(e);
                }
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

fn text_agent_hidden() -> bool {
    text_agent().hidden()
}

fn modifiers_from_event(event: &web_sys::KeyboardEvent) -> egui::Modifiers {
    egui::Modifiers {
        alt: event.alt_key(),
        ctrl: event.ctrl_key(),
        shift: event.shift_key(),

        // Ideally we should know if we are running or mac or not,
        // but this works good enough for now.
        mac_cmd: event.meta_key(),

        // Ideally we should know if we are running or mac or not,
        // but this works good enough for now.
        command: event.ctrl_key() || event.meta_key(),
    }
}

/// Text event handler,
pub fn install_text_agent(
    sender: Sender<(Option<egui::Event>, Option<TouchWebEvent>)>,
) -> Result<(), JsValue> {
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
                    let _ = sender_clone.send((Some(egui::Event::Text(text)), None));
                }
            }
        }) as Box<dyn FnMut(_)>);
        input.add_event_listener_with_callback("input", on_input.as_ref().unchecked_ref())?;
        on_input.forget();
    }

    body.append_child(&input)?;

    Ok(())
}

pub fn install_document_events(
    sender: Sender<(Option<egui::Event>, Option<TouchWebEvent>)>,
) -> Result<(), JsValue> {
    let document = web_sys::window().unwrap().document().unwrap();

    {
        // keydown
        let sender_clone = sender.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
            if event.is_composing() || event.key_code() == 229 {
                // https://www.fxsitecompat.dev/en-CA/docs/2018/keydown-and-keyup-events-are-now-fired-during-ime-composition/
                return;
            }

            let modifiers = modifiers_from_event(&event);
            let key = event.key();

            if let Some(key) = translate_key(&key) {
                let _ = sender_clone.send((
                    Some(egui::Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed: true,
                        modifiers,
                        repeat: false,
                    }),
                    None,
                ));
            }
            if !modifiers.ctrl
                && !modifiers.command
                && !should_ignore_key(&key)
                // When text agent is shown, it sends text event instead.
                && text_agent_hidden()
            {
                let _ = sender_clone.send((Some(egui::Event::Text(key)), None));
            }
        }) as Box<dyn FnMut(_)>);
        document.add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        // keyup
        let sender_clone = sender.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
            let modifiers = modifiers_from_event(&event);
            if let Some(key) = translate_key(&event.key()) {
                let _ = sender_clone.send((
                    Some(egui::Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed: false,
                        modifiers,
                        repeat: false,
                    }),
                    None,
                ));
            }
        }) as Box<dyn FnMut(_)>);
        document.add_event_listener_with_callback("keyup", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        // touch
        let sender_clone = sender.clone();
        let closure = Closure::wrap(Box::new(move |_event: web_sys::TouchEvent| {
            use web_sys::HtmlInputElement;
            // touch experiment
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

            input.set_hidden(false);
            match input.focus().ok() {
                Some(_) => {}
                None => {
                    bevy::log::error!("Unable to set focus");
                    // return;
                }
            }

            let _ = sender_clone.send((None, Some(TouchWebEvent::Fired)));
        }) as Box<dyn FnMut(_)>);
        document
            .add_event_listener_with_callback("touchend", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    Ok(())
}

/// Focus or blur text agent to toggle mobile keyboard.
pub fn update_text_agent(editing_text: bool, maybe_touch_pos: Option<egui::Pos2>) {
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
    let canvas = match document.query_selector("canvas") {
        Ok(Some(canvas)) => canvas,
        _ => {
            bevy::log::error!("No canvas found");
            return;
        }
    };
    let canvas_style = match canvas.dyn_into::<web_sys::HtmlCanvasElement>().ok() {
        Some(c) => c,
        None => {
            bevy::log::error!("Unable to make element into canvas");
            return;
        }
    }
    .style();

    if editing_text {
        let is_already_editing = input.hidden();

        if is_already_editing && maybe_touch_pos.is_some() {
            bevy::log::error!("probably failing to set the touch handler things");
            input.set_hidden(false);
            match input.focus().ok() {
                Some(_) => {}
                None => {
                    bevy::log::error!("Unable to set focus");
                    return;
                }
            }

            // Move up canvas so that text edit is shown at ~30% of screen height.
            // Only on touch screens, when keyboard popups.
            let latest_touch_pos = maybe_touch_pos.unwrap();
            let window_height = window.inner_height().unwrap().as_f64().unwrap() as f32;
            let current_rel = latest_touch_pos.y / window_height;

            // estimated amount of screen covered by keyboard
            let keyboard_fraction = 0.4;

            if current_rel > keyboard_fraction && is_mobile() == Some(true) {
                // below the keyboard
                let target_rel = 0.3;

                // Note: `delta` is negative, since we are moving the canvas UP
                let delta: f32 = target_rel - current_rel;

                let delta = delta.max(-keyboard_fraction); // Don't move it crazy much

                let new_pos_percent = format!("{}%", (delta * 100.0).round());

                match canvas_style.set_property("position", "absolute").ok() {
                    Some(_) => {}
                    None => {
                        bevy::log::error!("Unable to set canvas position");
                        return;
                    }
                }
                match canvas_style.set_property("top", &new_pos_percent).ok() {
                    Some(_) => {}
                    None => {
                        bevy::log::error!("Unable to set canvas position");
                    }
                }
            }
        }
    } else {
        if input.blur().is_err() {
            bevy::log::error!("Agent element not found");
            return;
        }

        input.set_hidden(true);
        match canvas_style.set_property("position", "absolute").ok() {
            Some(_) => {}
            None => {
                bevy::log::error!("Unable to set canvas position");
                return;
            }
        }
        match canvas_style.set_property("top", "0%").ok() {
            Some(_) => {}
            None => {
                bevy::log::error!("Unable to set canvas position");
            }
        } // move back to normal position
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

/// Web sends all all keys as strings, so it is up to us to figure out if it is
/// a real text input or the name of a key.
pub fn translate_key(key: &str) -> Option<egui::Key> {
    match key {
        "ArrowDown" => Some(egui::Key::ArrowDown),
        "ArrowLeft" => Some(egui::Key::ArrowLeft),
        "ArrowRight" => Some(egui::Key::ArrowRight),
        "ArrowUp" => Some(egui::Key::ArrowUp),

        "Esc" | "Escape" => Some(egui::Key::Escape),
        "Tab" => Some(egui::Key::Tab),
        "Backspace" => Some(egui::Key::Backspace),
        "Enter" => Some(egui::Key::Enter),
        "Space" | " " => Some(egui::Key::Space),

        "Help" | "Insert" => Some(egui::Key::Insert),
        "Delete" => Some(egui::Key::Delete),
        "Home" => Some(egui::Key::Home),
        "End" => Some(egui::Key::End),
        "PageUp" => Some(egui::Key::PageUp),
        "PageDown" => Some(egui::Key::PageDown),

        "0" => Some(egui::Key::Num0),
        "1" => Some(egui::Key::Num1),
        "2" => Some(egui::Key::Num2),
        "3" => Some(egui::Key::Num3),
        "4" => Some(egui::Key::Num4),
        "5" => Some(egui::Key::Num5),
        "6" => Some(egui::Key::Num6),
        "7" => Some(egui::Key::Num7),
        "8" => Some(egui::Key::Num8),
        "9" => Some(egui::Key::Num9),

        "a" | "A" => Some(egui::Key::A),
        "b" | "B" => Some(egui::Key::B),
        "c" | "C" => Some(egui::Key::C),
        "d" | "D" => Some(egui::Key::D),
        "e" | "E" => Some(egui::Key::E),
        "f" | "F" => Some(egui::Key::F),
        "g" | "G" => Some(egui::Key::G),
        "h" | "H" => Some(egui::Key::H),
        "i" | "I" => Some(egui::Key::I),
        "j" | "J" => Some(egui::Key::J),
        "k" | "K" => Some(egui::Key::K),
        "l" | "L" => Some(egui::Key::L),
        "m" | "M" => Some(egui::Key::M),
        "n" | "N" => Some(egui::Key::N),
        "o" | "O" => Some(egui::Key::O),
        "p" | "P" => Some(egui::Key::P),
        "q" | "Q" => Some(egui::Key::Q),
        "r" | "R" => Some(egui::Key::R),
        "s" | "S" => Some(egui::Key::S),
        "t" | "T" => Some(egui::Key::T),
        "u" | "U" => Some(egui::Key::U),
        "v" | "V" => Some(egui::Key::V),
        "w" | "W" => Some(egui::Key::W),
        "x" | "X" => Some(egui::Key::X),
        "y" | "Y" => Some(egui::Key::Y),
        "z" | "Z" => Some(egui::Key::Z),

        _ => None,
    }
}

fn should_ignore_key(key: &str) -> bool {
    let is_function_key = key.starts_with('F') && key.len() > 1;
    is_function_key
        || matches!(
            key,
            "Alt"
                | "ArrowDown"
                | "ArrowLeft"
                | "ArrowRight"
                | "ArrowUp"
                | "Backspace"
                | "CapsLock"
                | "ContextMenu"
                | "Control"
                | "Delete"
                | "End"
                | "Enter"
                | "Esc"
                | "Escape"
                | "Help"
                | "Home"
                | "Insert"
                | "Meta"
                | "NumLock"
                | "PageDown"
                | "PageUp"
                | "Pause"
                | "ScrollLock"
                | "Shift"
                | "Tab"
        )
}
