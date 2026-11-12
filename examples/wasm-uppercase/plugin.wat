;; Uppercase — the smallest useful WASM plugin for Rustcord.
;;
;; Exports the documented ABI:
;;   memory                      linear memory
;;   alloc(len) -> ptr           bump allocator for the host's input
;;   run(ptr, len) -> ptr        ASCI-uppercases the input into a fixed output area
;;   result_len() -> len         bytes written by the last run
;;
;; No imports, no start function, no table: it cannot reach outside this module.
(module
  (memory (export "memory") 2)

  ;; Bump pointer; the host calls `alloc` once per message.
  (global $bump (mut i32) (i32.const 1024))
  ;; Output length of the last `run`, read straight afterwards by the host.
  (global $out_len (mut i32) (i32.const 0))

  (func (export "alloc") (param $len i32) (result i32)
    (local $ptr i32)
    (local.set $ptr (global.get $bump))
    (global.set $bump (i32.add (global.get $bump) (local.get $len)))
    (local.get $ptr))

  (func (export "run") (param $ptr i32) (param $len i32) (result i32)
    (local $i i32)
    (local $byte i32)
    (global.set $out_len (local.get $len))
    (block $done
      (loop $each
        (br_if $done (i32.ge_u (local.get $i) (local.get $len)))
        (local.set $byte
          (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
        ;; ASCII a-z -> A-Z; every other byte is copied untouched.
        (if (i32.and
              (i32.ge_u (local.get $byte) (i32.const 97))
              (i32.le_u (local.get $byte) (i32.const 122)))
          (then
            (local.set $byte (i32.sub (local.get $byte) (i32.const 32)))))
        (i32.store8
          (i32.add (i32.const 4096) (local.get $i))
          (local.get $byte))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $each)))
    (i32.const 4096))

  (func (export "result_len") (result i32)
    (global.get $out_len)))
