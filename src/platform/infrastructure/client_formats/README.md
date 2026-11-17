# Client formats

Shared infrastructure for Player Sessions and Accounts. It decodes Packed XML and
handles the Python pickle values carried in Account messages and compressed
garage caches. The [bundle reader](../../../../crates/wot-resources/README.md) supplies
the extracted files.

| Module | Responsibility |
| --- | --- |
| `pxml/` | Decode typed values and nested elements from Packed XML resources. |
| `pickle/` | Encode server payloads and decode bounded pickle data without executing it. |
| `util/` | Binary read/write primitives and wire diagnostics. |

Player Sessions owns the script ABI and Mercury framing. Accounts owns the garage cache
projection. The shared codecs are independent of both contexts and their domain
models. See [third-party notices](../../../../THIRD_PARTY_NOTICES.md) for provenance.
