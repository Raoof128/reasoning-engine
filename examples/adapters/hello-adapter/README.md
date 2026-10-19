# Writing a source adapter

A source adapter is a WebAssembly core module the engine runs in a sandbox
(`wasm-adapters` feature). Put `<name>.wasm` (or `<name>.wat`) in
`~/.reasoning-engine/adapters/` (or `REASONING_ENGINE_ADAPTERS_DIR`) and add
`<name>` to `REASONING_ENGINE_SOURCES`.

## Exports

| Export | Signature | Returns |
|---|---|---|
| `memory` | linear memory | |
| `alloc` | `(len: i32) -> i32` | room for `len` bytes the host will write |
| `allowed_hosts` | `() -> i64` | a JSON array of host names it may fetch |
| `search` | `(query_ptr: i32, query_len: i32, limit: i32) -> i64` | a JSON array of records, or `{"error": "..."}` |

An `i64` packs a pointer and a length: `ptr << 32 | len`. A record has
`title` (required), `snippet`, `url`, `doi`, `year`, `authors`, `venue` and
`score` (0 to 1).

## Imports (module `host`, nothing else)

| Import | Signature | Does |
|---|---|---|
| `http_get` | `(url_ptr: i32, url_len: i32) -> i64` | GETs an `https` URL on a declared, public host; the body is written through `alloc`. `-1`: host not allowed. `-2`: request failed. |
| `log` | `(ptr: i32, len: i32)` | writes a line to stderr |

There is no WASI: no files, environment, clock or sockets. Each search gets
10 million units of fuel and 64 MiB of memory; running out ends the search
with an error, never the engine.

`hello.wat` is a complete adapter. Any language that targets
`wasm32-unknown-unknown` works the same way: export the four functions
above and import the two host functions.
