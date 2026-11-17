# Binary Mapping

Recover inspectable structure metadata from a versioned game executable to
reduce repeated manual reverse engineering. This context owns static binary
inspection and evidence-bearing Rust/JSON generation. It does not own network
transport, server gameplay, game-resource schema loading or client execution.

## Layers and source map

| Layer / module | Responsibility |
| --- | --- |
| `domain/mod.rs` | Investigation, native types, base relationships, vtables, interfaces, messages and framing; independent of PE and instruction libraries. |
| `application/mod.rs` | Investigation use case, limitations and the `BinaryInvestigation` input port. |
| `infrastructure/mod.rs` | Implements the input port using file IO, SHA-256 and the static passes. |
| `infrastructure/pe.rs` | Bounds-checked AMD64 PE32+ section/address translation and runtime-function ranges. |
| `infrastructure/registrations.rs` | Conservative startup instruction tracing, registration function discovery, initializer ordering and range expansion. |
| `infrastructure/callbacks.rs` | Bounded concrete evaluation of supported read-only length callbacks against static handler data. |
| `infrastructure/rtti.rs` | Validated MSVC complete-object locators, type descriptors, inheritance and vtable pointer runs. |
| `infrastructure/rust.rs` | Rust source rendering from investigation models. |
| `presentation/mod.rs` | CLI arguments, input selection, output publication and summary. |
| `../main.rs` | Minimal executable bootstrap for `wg-binary-map`. |

The application defines a port; the static implementation depends on that port
and the domain. Presentation composes the implementation and use case. Other
future contexts must consume an explicit exported contract rather than PE or
instruction internals. The executable lives in `wot-binary-map` and builds
independently of the server.

The external game client is upstream; Binary Mapping acts as an anti-corruption
layer translating implementation-specific metadata into investigation concepts.
No other server context currently consumes the artifacts. Reference sources and
wg-toolkit are research inputs, not runtime dependencies or implemented contexts.

## Shared language

| Term | Meaning |
| --- | --- |
| Investigation | A deterministic result tied to an executable SHA-256. |
| VA / RVA | Preferred-image virtual address / offset from the PE image base; not a file offset or an ASLR-adjusted runtime pointer. |
| Native type | A C++ type identified by validated RTTI, with an unknown complete member layout. |
| Type descriptor | MSVC metadata containing a decorated type name. |
| Base relationship | A flattened RTTI inheritance entry with displacement metadata, including virtual-base indirection. |
| Vtable run | Consecutive executable pointers after a validated complete-object locator; its end is a heuristic. |
| Interface | A named Mercury message registration group. |
| Message | A registration with its ID or ID range, framing and handler evidence. |
| Framing | Fixed payload size, variable length-prefix width or handler callback. |
| Initializer slot | A pointer to a startup function; ordering these slots determines registration order in the supported build. |
| Opaque declaration | A Rust name/identity placeholder with no claim about C++ layout, size or ABI compatibility. |

See [algorithm and findings](../../../../docs/binary-mapping.md),
[tests](../../../../docs/testing.md) and the [context map](../../../../CONTEXT_MAP.md).
