#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| idecraft_hardening::targets::sql_split(data));
