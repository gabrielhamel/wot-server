# Client breaking changes

## EU 2.4.1 to 2.4.2

The Account version field in `scripts/entity_defs/interfaces/AccountVersion.def`
changed from `requiredVersion_2410` with default `eu_2.4.1_3` to
`requiredVersion_2420` with default `eu_2.4.2`. The server exited at startup
because it looked for the previous property by name. Both images carried the
same launcher build label, but their bundled manifests reported different
client versions.

The [failed build](https://github.com/gabrielhamel/wot-server/actions/runs/44724685373)
and [preceding build](https://github.com/gabrielhamel/wot-server/actions/runs/44645998404)
provided the bundles used to confirm the change. The server now finds the
version field in the installed client's definitions and checks the complete
Account creation layout before accepting connections.
