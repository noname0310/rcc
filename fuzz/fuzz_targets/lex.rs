#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Accept any bytes; enforce UTF-8 by dropping invalid inputs.
    let Ok(s) = std::str::from_utf8(data) else { return };
    let file = rcc_span::FileId(0);
    // Bound the number of iterations so we don't spend the whole budget on
    // a single pathological input.
    let mut n: u32 = 0;
    for _tok in rcc_lexer::tokenize(file, s) {
        n += 1;
        if n > 1_000_000 {
            break;
        }
    }
});
