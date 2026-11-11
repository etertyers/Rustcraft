//! Talking between the server and its clients: the messages and how they
//! travel.

#[cfg(not(target_arch = "wasm32"))]
pub mod lan;
#[cfg(target_arch = "wasm32")]
#[path = "lan_web.rs"]
pub mod lan;
pub mod protocol;
pub mod transport;
pub mod worker;
