# Naming bounded contexts

A bounded context defines where a model and its vocabulary have a consistent
meaning. Start by identifying the decisions the context owns and the terms used
by the people who work with those decisions. Choose a name that makes those
responsibilities recognizable to a player, operator, or domain specialist.

- Name the responsibility. **Battle Allocation** describes deciding which
  players enter which battle. A queue or worker is one way to carry out that decision.
- Check that the name survives an implementation change. **Social** still
  describes contacts and private conversations if the chat protocol changes.
  Protocols, transports, databases, UI frameworks, and architecture patterns
  belong in the names of adapters or in the context's implementation guide.
- Write one sentence describing what the context owns. Check that the name,
  glossary, and public contracts describe the same responsibility. If that
  sentence needs unrelated responsibilities, review the boundary before renaming.
- Distinguish player and operator contexts from supporting tools. A technical
  discipline can have its own vocabulary and model. Use that vocabulary for a
  tool's scope and label its role in the map.

## Review of the current names

The [context map](../CONTEXT_MAP.md) uses the names found in the code today.
The alternatives below are suggestions for future boundary work.

| Current name | Assessment |
| --- | --- |
| Accounts | Familiar domain language for identity and account ownership. The context also owns much of the garage and progression. Revisit that boundary if those rules grow into a separate model. |
| Social | Fits contacts, presence as seen by other players, and private conversations. |
| Client Gateway | Describes the current integration role. **Player Sessions** would fit a boundary centered on admission, handoff, authenticated play, and disconnect. Mercury, XMPP, and garage command translation would describe adapter responsibilities. |
| Admin | Familiar shorthand for an audience. **Player Administration** better describes the operator's current tasks of provisioning players, inspecting garages, and granting resources or vehicles. |
| Resource Extraction | Clear name for an offline compatibility tool that publishes a resource bundle for the runtime. Its language comes from the tool's users. |
| Binary Mapping | Clear name for the offline research workflow that maps executable structures and preserves the evidence for those mappings. |
| Game Client | The external system on the map. Its name identifies the integration partner. |
