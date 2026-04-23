#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else { return };
    let mut session = rcc_session::Session::new(rcc_session::Options::default());
    let file = session.source_map.write().unwrap().add_file("<fuzz>".into(), std::sync::Arc::from(s.to_owned()));
    let _ = rcc_preprocess::preprocess(&mut session, file);
});
