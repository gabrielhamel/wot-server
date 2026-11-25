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

### Account creation crash after the EU production update

On 2026-11-25, the EU client `v.2.4.2.0 #1000` authenticated and then crashed
on `CreateBasePlayer`. Its `game.log` reported
`PyEntity::initComponentsFromStream: Wrong number of components in stream. Expected 18, got 20`
before the assertion in `bw_entities.cpp:1103`. The running
`win64/WorldOfTanks.exe` had SHA-256
`0547c40d8f55bb94437ba54603ddb7cec0075aca1a0d5a51cf181f1e044496f5`.

The [0.26.0 image build](https://github.com/gabrielhamel/wot-server/actions/runs/45574251379)
updated the runner's cached client. Its published bundle declared 20 static
Account components including `FortRushAccountComponent` and
`HalloweenAccountComponent`. An extraction from the matching Game Center
installation declared 18 and had neither extension. The extractor used to
append all installed packages after processing `paths.xml`, so obsolete
packages could contribute live protocol definitions. The extractor now
uses only the installed packages listed in `paths.xml` when it is present.
The server still derives the wire count from the resulting bundle.
Regenerate affected bundles before serving the updated client.
