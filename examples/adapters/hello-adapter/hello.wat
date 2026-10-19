;; The smallest useful reasoning-engine source adapter, in WebAssembly text.
;; Copy it to ~/.reasoning-engine/adapters/hello.wat and add `hello` to
;; REASONING_ENGINE_SOURCES. See README.md beside it for the interface.
(module
  (import "host" "http_get" (func $http_get (param i32 i32) (result i64)))
  (import "host" "log" (func $log (param i32 i32)))
  (memory (export "memory") 1)

  ;; A bump allocator: the host asks for room before writing into memory.
  (global $next (mut i32) (i32.const 4096))
  (func (export "alloc") (param $len i32) (result i32)
    (local $ptr i32)
    (local.set $ptr (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get $len)))
    (if (i32.gt_u (global.get $next) (i32.mul (memory.size) (i32.const 65536)))
      (then
        (drop (memory.grow
          (i32.add (i32.const 1)
            (i32.div_u (global.get $next) (i32.const 65536)))))))
    (local.get $ptr))

  ;; The hosts this adapter may fetch: 15 bytes at offset 0.
  (data (i32.const 0) "[\"example.org\"]")
  (func (export "allowed_hosts") (result i64)
    (i64.const 15))

  ;; One canned record: 119 bytes at offset 256.
  (data (i32.const 256) "[{\"title\":\"Hello from WASM\",\"snippet\":\"A sandboxed adapter found this.\",\"url\":\"https://example.org/hello\",\"year\":2026}]")
  (data (i32.const 1024) "hello adapter searched")
  (func (export "search") (param $query i32) (param $len i32) (param $limit i32) (result i64)
    (call $log (i32.const 1024) (i32.const 22))
    (i64.or
      (i64.shl (i64.const 256) (i64.const 32))
      (i64.const 119)))
)
