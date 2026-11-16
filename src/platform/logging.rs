//! Logging. Native: `env_logger` (`RUST_LOG=rustcraft=debug`). Browser: the
//! developer console, through `web-sys`.

#[cfg(not(target_arch = "wasm32"))]
pub fn init() {
    let _ = env_logger::try_init();
}

#[cfg(target_arch = "wasm32")]
pub fn init() {
    static LOGGER: ConsoleLogger = ConsoleLogger;
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

#[cfg(target_arch = "wasm32")]
struct ConsoleLogger;

#[cfg(target_arch = "wasm32")]
impl log::Log for ConsoleLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = wasm_bindgen::JsValue::from_str(&format!(
            "[{} {}] {}",
            record.level(),
            record.target(),
            record.args()
        ));
        match record.level() {
            log::Level::Error => web_sys::console::error_1(&line),
            log::Level::Warn => web_sys::console::warn_1(&line),
            _ => web_sys::console::log_1(&line),
        }
    }

    fn flush(&self) {}
}
