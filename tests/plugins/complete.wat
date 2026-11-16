(module
  ;; Completion sample (SPEC-0210 behaviour 1): `complete` items for `.txt` and `.conf` files.
  ;; Built with `IDECRAFT_BLESS=1 cargo test -p idecraft-plugins --test samples` into the `.wasm`
  ;; next to this file; install that file in Settings › Plugins (SPEC-0126, SPEC-0211).
  (memory (export "memory") 2)
  (global $next (mut i32) (i32.const 32768))
  (data (i32.const 1024) "{\"name\":\"Words\",\"version\":\"1.0\",\"description\":\"Completion sample: suggests two words\",\"actions\":[{\"id\":\"words\",\"title\":\"Words\",\"kind\":\"complete\",\"files\":[\"txt\",\"conf\"]}]}\00")
  (data (i32.const 8192) "{\"items\":[{\"label\":\"plugin_hello\",\"detail\":\"greeting\"},{\"label\":\"plugin_world\",\"insert\":\"plugin_world()\"}]}\00")
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
  (func (export "idecraft_alloc") (param $len i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get $len)))
    (block $ok
      (loop $grow
        (br_if $ok (i32.le_u (global.get $next) (i32.mul (memory.size) (i32.const 65536))))
        (if (i32.eq (memory.grow (i32.const 1)) (i32.const -1)) (then unreachable))
        (br $grow)))
    (local.get $at))
  (func (export "idecraft_manifest") (result i64)
    (call $pack (i32.const 1024)))
  ;; Every run answers the same JSON.
  (func (export "idecraft_run") (param $a i32) (param $al i32) (param $p i32) (param $l i32) (result i64)
    (call $pack (i32.const 8192)))
)
