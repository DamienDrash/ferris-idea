(module
  ;; Status bar sample (SPEC-0210 behaviour 4): shows the number of lines of the active file.
  ;; Built with `FERRIS_IDEA_BLESS=1 cargo test -p ferris-idea-plugins --test samples` into the `.wasm`
  ;; next to this file; install that file in Settings › Plugins (SPEC-0126, SPEC-0211).
  (memory (export "memory") 2)
  (global $next (mut i32) (i32.const 32768))
  (data (i32.const 1024) "{\"name\":\"Line Count\",\"version\":\"1.0\",\"description\":\"Status bar sample: lines of the file\",\"actions\":[{\"id\":\"lines\",\"title\":\"Lines\",\"kind\":\"status\"}]}\00")
  (data (i32.const 8192) "{\"text\":\"Lines: ")
  (data (i32.const 8448) "\",\"tooltip\":\"Lines of the file (status sample)\"}\00")
  ;; Length of the NUL-terminated text at $p.
  (func $len (param $p i32) (result i32)
    (local $n i32)
    (block $done
      (loop $next
        (br_if $done (i32.eqz (i32.load8_u (i32.add (local.get $p) (local.get $n)))))
        (local.set $n (i32.add (local.get $n) (i32.const 1)))
        (br $next)))
    (local.get $n))
  ;; Pointer (high 32 bits) and length (low 32 bits) of the text at $p.
  (func $pack (param $p i32) (result i64)
    (i64.or
      (i64.shl (i64.extend_i32_u (local.get $p)) (i64.const 32))
      (i64.extend_i32_u (call $len (local.get $p)))))
  ;; Memory for the host to write into; grows the memory as needed.
  (func (export "ferris_idea_alloc") (param $len i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get $len)))
    (block $ok
      (loop $grow
        (br_if $ok (i32.le_u (global.get $next) (i32.mul (memory.size) (i32.const 65536))))
        (if (i32.eq (memory.grow (i32.const 1)) (i32.const -1)) (then unreachable))
        (br $grow)))
    (local.get $at))
  (func (export "ferris_idea_manifest") (result i64)
    (call $pack (i32.const 1024)))
  ;; Counts the lines of the context's text: escaped line breaks (`\\n`) in the JSON input, plus one.
  (func (export "ferris_idea_run") (param $a i32) (param $al i32) (param $p i32) (param $l i32) (result i64)
    (local $i i32) (local $lines i32) (local $c i32) (local $at i32) (local $digits i32) (local $n i32)
    (local.set $lines (i32.const 1))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (local.get $l)))
        (if (i32.eq (i32.load8_u (i32.add (local.get $p) (local.get $i))) (i32.const 92))
          (then
            (local.set $i (i32.add (local.get $i) (i32.const 1)))
            (if (i32.eq (i32.load8_u (i32.add (local.get $p) (local.get $i))) (i32.const 110))
              (then (local.set $lines (i32.add (local.get $lines) (i32.const 1)))))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))
    ;; The digits, written backwards from 8999.
    (local.set $at (i32.const 9000))
    (local.set $n (local.get $lines))
    (loop $digit
      (local.set $at (i32.sub (local.get $at) (i32.const 1)))
      (i32.store8 (local.get $at) (i32.add (i32.const 48) (i32.rem_u (local.get $n) (i32.const 10))))
      (local.set $n (i32.div_u (local.get $n) (i32.const 10)))
      (br_if $digit (i32.ne (local.get $n) (i32.const 0))))
    ;; Output at 10000: prefix (8192), digits, suffix (8448), NUL.
    (local.set $digits (i32.sub (i32.const 9000) (local.get $at)))
    (memory.copy (i32.const 10000) (i32.const 8192) (i32.const 16))
    (memory.copy (i32.add (i32.const 10000) (i32.const 16)) (local.get $at) (local.get $digits))
    (memory.copy
      (i32.add (i32.add (i32.const 10000) (i32.const 16)) (local.get $digits))
      (i32.const 8448) (i32.const 49))
    (call $pack (i32.const 10000)))
)
