# Offline binary structure mapping

`wg-binary-map` is an independent executable in the `wot-binary-map` package. It reads
an installed game directory (selecting `win64/WorldOfTanks.exe`) or an explicit
AMD64 executable and emits a new directory containing:

- `structures.rs`: standalone Rust source with message payload containers and
  opaque native type declarations.
- `mapping.json`: versioned evidence, SHA-256, preferred image base, message
  names/IDs/framing, registration and initializer addresses, callback functions,
  native descriptors, base displacements and vtable pointer runs.

```sh
cargo run --release -p wot-binary-map -- /path/to/game --output artifacts/client-map
rustc --edition=2024 --crate-type=lib artifacts/client-map/structures.rs --out-dir /tmp
```

The tool needs no game process, debugger, toolkit checkout, engine source or
Python decompiler at runtime. It never executes game instructions on the host.
Output paths must not exist, preventing accidental replacement of earlier
investigations. Identical input bytes produce identical Rust and JSON; the report
stores the executable filename rather than the machine's absolute input path.

## What the algorithm recovers

### PE image and address model

Validate DOS/PE signatures, AMD64 and PE32+ headers, raw section bounds and the
exception directory. Translate preferred VAs through file-backed sections;
zero-initialized virtual tails are not fabricated as bytes. The exception
directory supplies function boundaries. Absolute addresses in the report refer
to the preferred image, not process addresses after ASLR.

### Network registrations

1. Trace short runtime functions (up to 512 bytes) using a deliberately small
   subset of x64 instructions. Track constants, RIP-relative addresses and
   global loads for Windows x64 argument registers and the fifth stack argument.
   Stop at branches; invalidate volatile registers after calls.
2. Find the common call target receiving `createBasePlayer` and
   `createCellPlayer`. These semantic anchors locate `InterfaceMinder::add`
   without hard-coded build addresses.
3. Collect callers of that target and discover `addRange` calls referring to
   the stored registration results. Find each constructor's unique pointer in
   read-only data, require one contiguous executable-pointer table, and order
   registrations by those initializer slots. Function
   addresses alone do **not** give the correct order in this executable.
4. Read each minder's static name at offset `+24`, recover message names,
   length style/parameter and handler pointers. Assign sequential IDs within
   each minder. Replay `addRange` with
   `end = next + (254 - next) / portion`, preserving range-registration evidence.
5. Inspect callback handler vtable slot 3. A bounded evaluator supports integer
   moves, reads within 256 bytes of the static handler, basic arithmetic,
   zero/equality branches and returns. It allows at most 128 instructions
   within 512 bytes of the callback entry. Calls, writes, unsupported instructions,
   other memory accesses and unresolved values leave the length unknown.

Historical source support is
`docs/references/BigWorld-Engine-14.4.1/programming/bigworld/lib/network/interface_minder.cpp`
(`add`, `addRange`) and `interface_element.hpp` (length styles 0/1/2).
Applicability was checked against the supplied executable: the discovered `add`
is at `0x142d32220`, uses a 24-byte element stride and assigns the vector length
as message ID; `addRange` is at `0x142d322c0`. Static initialization layout and
callback vtable slot selection are build-dependent assumptions, validated for
this image rather than universal engine guarantees.

### Native type graph

Scan aligned read-only data for MSVC x64 complete-object locators. Require
signature 1, a matching self RVA, a valid type descriptor and a coherent bounded
class hierarchy whose first entry describes the complete type. Demangle type
names. Preserve each flattened base descriptor's `mdisp`, `pdisp`, `vdisp` and
attributes, including self entries; a virtual base's displacement is not a
simple final member offset.

Locate pointers to validated locators, then collect subsequent executable
pointers as vtable runs. These runs provide analysis entry points. Their lengths
are heuristic because stripped PE files do not delimit every vtable explicitly.
Only native types with validated locators are included; scanning name strings
alone would yield a larger, less strongly supported inventory.

## Verified supplied-client result

Inspected on 2026-11-11:

- Input: `wot-eu-production-2.4.1.5498-windows-x64-sd-en/win64/WorldOfTanks.exe`.
- SHA-256: `58325ded3e2db7134638971e33de1eab09737f8b66401554d9723a2bf246bb11`.
- Preferred image base: `0x140000000`.
- `version.xml`: `v.2.4.1.1 #993`, client `2653317`, overrides `2653578`, realm EU.
  The directory label alone does not establish the current executable's exact
  launcher build identity.
- 20,904 native types with validated RTTI hierarchies.
- 111 message registrations across four interfaces:

| Interface name from binary | Registrations | ID coverage |
| --- | ---: | --- |
| ClientInterface | 80 | `00..fe`, with method `4e..a6`, property `a7..fe` |
| BaseAppExtInterface | 18 | `00..fe`, with cell method `10..87`, base method `88..fe` |
| CellAppExtInterface | 8 | `00..07`, no range expansion |
| LoginInterface | 5 | `00..04` |

All 24 movement callback lengths were recovered. In ID order `29..40`, they are
`15,13,12,11,12,10,9,8,9,7,6,5,12,10,9,8,9,7,6,5,6,4,3,2` bytes.

The reference implementation is `docs/references/wg-toolkit-rs/wg-toolkit/src/app/{client,base,cell}/element.rs`.
Its client IDs and framing, base IDs and framing, cell IDs and framing, and the
24 movement lengths are used for comparison. The binary names the cell group
`CellAppExtInterface`; the toolkit's older comments call it `CellAppInterface`.
The toolkit cites older client `2.3.1.3` for movement disassembly; matching lengths
here establish agreement for these values, not universal compatibility.

## Unresolved structures and next passes

The generated source is a research artifact. Fixed payload containers preserve
size; variable and unresolved callback payloads use `Vec<u8>`. It does not infer
field names or field semantics. Native declarations have private zero-sized
opaque fields, no `repr(C)` and no asserted size: they are identities, **not**
usable C++ object layouts or FFI bindings. No packet codecs are generated.

An arbitrary stripped executable does not contain sufficient metadata to recover
all original structs, semantic field names or optimized-away definitions. This
first algorithm therefore maps surviving type and framing evidence; it does
not yet reproduce every toolkit structure. Additional passes need to:

- Analyze serializer/handler memory accesses and constructor allocation sizes
  to recover field candidates, explicitly separating wire offsets from native
  object offsets and recording conflicting interpretations.
- Load packed entity definitions/aliases from the matching game's resources for
  `Account`, `Avatar`, `Vehicle`, components, properties, method arguments and
  fixed dictionaries. The toolkit already has a resource-derived script model
  in `wg-toolkit/src/script/{load,parse,ty}.rs` and dispatch in `app/dispatch.rs`;
  those definitions are not recoverable solely from C++ RTTI.
- Correlate resource schemas and native Python bindings with decompiled behavior.
  Supplied `docs/references/wot-src/.version_name` is `2.4.1.5514`; version identity differs from
  the installed directory label, so matches need separate verification.
  `sources/res/scripts/client/{Account,Avatar,Vehicle}.py` provide method behavior;
  `NetworkReplicationPointComponent.py` delegates NRL data to native BigWorld
  objects, showing why Python alone does not describe their binary layouts.

The extractor is currently specific to the verified AMD64 MSVC registration
patterns. Missing/ambiguous anchors, initialization pointers, arguments or range
sources cause errors before output publication. Functions beyond the supported
tracing pattern can be missed; successful extraction is not a proof of complete
coverage for another executable. Unsupported RTTI candidates are skipped and
unsupported length callbacks remain unknown. A newer version needs renewed
comparison before its output becomes a compatibility requirement.
