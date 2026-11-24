#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| ferris_idea_hardening::targets::keymap(data));
