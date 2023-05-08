//! The text agent is an `<input>` element used to trigger
//! mobile keyboard and IME input.

use wasm_bindgen::prelude::*;

use crate::systems::{ContextSystemParams, InputResources};

static AGENT_ID: &str = "egui_text_agent";

pub fn text_agent() -> web_sys::HtmlInputElement {
    use wasm_bindgen::JsCast;
    web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .get_element_by_id(AGENT_ID)
        .unwrap()
        .dyn_into()
        .unwrap()
}

pub fn install() {
    install_text_agent().unwrap();
}

/// Text event handler,
pub fn install_text_agent() -> Result<(), JsValue> {
    use wasm_bindgen::JsCast;
    let window = web_sys::window().unwrap();
    let document = window.document().unwrap();
    let body = document.body().expect("document should have a body");
    let input = document
        .create_element("input")?
        .dyn_into::<web_sys::HtmlInputElement>()?;
    let input = std::rc::Rc::new(input);
    input.set_id(AGENT_ID);
    {
        let style = input.style();
        // Transparent
        style.set_property("opacity", "0").unwrap();
        // Hide under canvas
        style.set_property("z-index", "-1").unwrap();
    }
    // Set size as small as possible, in case user may click on it.
    input.set_size(1);
    input.set_autofocus(true);
    input.set_hidden(true);

    bevy::log::info!("Text Agent Installed");

    /* // When IME is off
    runner_container.add_event_listener(&input, "input", {
        let input_clone = input.clone();
        let is_composing = is_composing.clone();

        move |_event: web_sys::InputEvent, mut runner_lock| {
            let text = input_clone.value();
            if !text.is_empty() && !is_composing.get() {
                input_clone.set_value("");
                runner_lock.input.raw.events.push(egui::Event::Text(text));
                runner_lock.needs_repaint.repaint_asap();
            }
        }
    })?;

    {
        // When IME is on, handle composition event
        runner_container.add_event_listener(&input, "compositionstart", {
            let input_clone = input.clone();
            let is_composing = is_composing.clone();

            move |_event: web_sys::CompositionEvent, mut runner_lock: MutexGuard<'_, AppRunner>| {
                is_composing.set(true);
                input_clone.set_value("");

                runner_lock
                    .input
                    .raw
                    .events
                    .push(egui::Event::CompositionStart);
                runner_lock.needs_repaint.repaint_asap();
            }
        })?;

        runner_container.add_event_listener(
            &input,
            "compositionupdate",
            move |event: web_sys::CompositionEvent, mut runner_lock: MutexGuard<'_, AppRunner>| {
                if let Some(event) = event.data().map(egui::Event::CompositionUpdate) {
                    runner_lock.input.raw.events.push(event);
                    runner_lock.needs_repaint.repaint_asap();
                }
            },
        )?;

        runner_container.add_event_listener(&input, "compositionend", {
            let input_clone = input.clone();

            move |event: web_sys::CompositionEvent, mut runner_lock: MutexGuard<'_, AppRunner>| {
                is_composing.set(false);
                input_clone.set_value("");

                if let Some(event) = event.data().map(egui::Event::CompositionEnd) {
                    runner_lock.input.raw.events.push(event);
                    runner_lock.needs_repaint.repaint_asap();
                }
            }
        })?;
    }

    // When input lost focus, focus on it again.
    // It is useful when user click somewhere outside canvas.
    runner_container.add_event_listener(
        &input,
        "focusout",
        move |_event: web_sys::MouseEvent, _| {
            // Delay 10 ms, and focus again.
            let func = js_sys::Function::new_no_args(&format!(
                "document.getElementById('{}').focus()",
                AGENT_ID
            ));
            window
                .set_timeout_with_callback_and_timeout_and_arguments_0(&func, 10)
                .unwrap();
        },
    )?; */

    body.append_child(&input)?;

    Ok(())
}

/// Focus or blur text agent to toggle mobile keyboard.
pub fn update_text_agent(input_resources: &InputResources, context_params: &ContextSystemParams) {
    use wasm_bindgen::JsCast;
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

    let mut focus = false;

    for contexts in context_params.contexts.iter() {
        if contexts.egui_input.has_focus {
            focus = true;
            break;
        }
    }

    if focus {
        let is_already_editing = input.hidden();
        if is_already_editing {
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
            /* if let Some(latest_touch_pos) = runner.input.latest_touch_pos {
                let window_height = window.inner_height().ok()?.as_f64()? as f32;
                let current_rel = latest_touch_pos.y / window_height;

                // estimated amount of screen covered by keyboard
                let keyboard_fraction = 0.5;

                if current_rel > keyboard_fraction {
                    // below the keyboard

                    let target_rel = 0.3;

                    // Note: `delta` is negative, since we are moving the canvas UP
                    let delta = target_rel - current_rel;

                    let delta = delta.max(-keyboard_fraction); // Don't move it crazy much

                    let new_pos_percent = format!("{}%", (delta * 100.0).round());

                    canvas_style.set_property("position", "absolute").ok()?;
                    canvas_style.set_property("top", &new_pos_percent).ok()?;
                }
            } */
        }
    } else {
        // Holding the runner lock while calling input.blur() causes a panic.
        // This is most probably caused by the browser running the event handler
        // for the triggered blur event synchronously, meaning that the mutex
        // lock does not get dropped by the time another event handler is called.
        //
        // Why this didn't exist before #1290 is a mystery to me, but it exists now
        // and this apparently is the fix for it
        //
        // ¯\_(ツ)_/¯ - @DusterTheFirst
        if let Err(e) = input.blur() {
            bevy::log::error!("Agent element not found");
            return;
        }

        input.set_hidden(true);
        /* canvas_style.set_property("position", "absolute").ok()?;
        canvas_style.set_property("top", "0%").ok()?; // move back to normal position */
    }
}

/* /// If context is running under mobile device?
pub(crate) fn is_mobile() -> Option<bool> {
    const MOBILE_DEVICE: [&str; 6] = ["Android", "iPhone", "iPad", "iPod", "webOS", "BlackBerry"];

    let user_agent = web_sys::window()?.navigator().user_agent().ok()?;
    let is_mobile = MOBILE_DEVICE.iter().any(|&name| user_agent.contains(name));
    Some(is_mobile)
}
 */
