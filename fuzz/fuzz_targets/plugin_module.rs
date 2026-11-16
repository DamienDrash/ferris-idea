#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| idecraft_hardening::targets::plugin_module(data));
