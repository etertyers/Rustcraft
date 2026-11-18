//! A browser `MessagePort` as a [`Port`] (M76): the page and the server's
//! Web Worker exchange the same byte frames as `net::worker` (bincode + lz4),
//! each moved (not copied) as a transferred `ArrayBuffer`.

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use wasm_bindgen::{JsCast, prelude::*};
use web_sys::{MessageEvent, MessagePort};

use crate::net::worker::Port;

pub struct MessagePortPort {
    port: MessagePort,
    inbox: Rc<RefCell<VecDeque<Vec<u8>>>>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
}

// SAFETY: the browser build runs the game on one thread per wasm instance (the
// page, or the server's worker); a port never leaves the thread that made it.
unsafe impl Send for MessagePortPort {}

impl MessagePortPort {
    /// Listens on `port`; frames arrive in order.
    pub fn new(port: MessagePort) -> Self {
        let inbox = Rc::new(RefCell::new(VecDeque::new()));
        let queue = inbox.clone();
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let data = event.data();
            let bytes = if let Ok(buffer) = data.clone().dyn_into::<js_sys::ArrayBuffer>() {
                js_sys::Uint8Array::new(&buffer).to_vec()
            } else {
                js_sys::Uint8Array::new(&data).to_vec()
            };
            queue.borrow_mut().push_back(bytes);
        });
        // Setting `onmessage` also starts the port.
        port.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        Self {
            port,
            inbox,
            _on_message: on_message,
        }
    }
}

impl Port for MessagePortPort {
    fn post(&mut self, frame: Vec<u8>) -> bool {
        let array = js_sys::Uint8Array::from(frame.as_slice());
        let buffer = array.buffer();
        let transfer = js_sys::Array::of1(&buffer);
        self.port
            .post_message_with_transferable(&buffer, &transfer)
            .is_ok()
    }

    fn poll(&mut self) -> Option<Vec<u8>> {
        self.inbox.borrow_mut().pop_front()
    }

    fn is_open(&self) -> bool {
        true
    }
}
