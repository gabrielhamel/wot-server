# Resource Extraction

This offline context reads a World of Tanks installation and publishes the
versioned resource bundle consumed by Accounts, Player Sessions and Battles. It owns package
reading, source version provenance, input precedence and the resource selection for the implemented server.
It uses the [bundle contract](../../src/resources/README.md) and Platform's
Packed XML reader for the client's destruction definitions.

| Layer | Responsibility |
| --- | --- |
| `presentation/` | Parse input and output paths and report the extraction result. |
| `application/` | Select definitions, derive component class metadata and write a new bundle. |
| `infrastructure/` | Index client packages and loose files, copy selected bytes, and publish vehicle armor meshes, scenery collision meshes, structure dependencies and numbered SpeedTree placements. |

| Term | Meaning |
| --- | --- |
| Resource key | Slash-separated path relative to the game's resource root. |
| Extension | Named resource root with an `extension.xml` descriptor. |
| Component client class | Presence of a compiled client script used to identify a networked component. |
| Resource bundle | Selected definitions and the manifest describing their runtime metadata. |

See the [extraction guide](../../docs/resources.md) for commands and deployment.
