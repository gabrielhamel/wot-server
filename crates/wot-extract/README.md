# Resource Extraction

This offline context reads a World of Tanks installation and publishes the
versioned resource bundle consumed by Accounts and Client Gateway. It owns package
reading, source version provenance, input precedence and the resource selection for the implemented server.
Its only shared code with the server is the [bundle contract](../wot-resources/README.md).

| Layer | Responsibility |
| --- | --- |
| `presentation/` | Parse input and output paths and report the extraction result. |
| `application/` | Select definitions, derive component class metadata and write a new bundle. |
| `infrastructure/` | Index client packages and loose files and copy selected bytes. |

| Term | Meaning |
| --- | --- |
| Resource key | Slash-separated path relative to the game's resource root. |
| Extension | Named resource root with an `extension.xml` descriptor. |
| Component client class | Presence of a compiled client script used to identify a networked component. |
| Resource bundle | Selected definitions and the manifest describing their runtime metadata. |

See the [extraction guide](../../docs/resources.md) for commands and deployment.
