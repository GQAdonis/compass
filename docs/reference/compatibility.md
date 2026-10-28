# Compatibility and evolution

Compass is an independent native product. Its public behavior is defined by
Compass documentation, native tests, and versioned Compass formats. It has no
Graphify runtime or test dependency.

## Product contract

The supported identity is:

| Surface | Contract |
| --- | --- |
| executable | `compass` |
| artifact root | `compass-out/` |
| ignore file | `.compassignore` |
| project configuration | `.compass/` |
| environment | `COMPASS_*` |
| MCP | server `compass`, resources `compass://...` |

There is no alternate legacy frontend or alias. Legacy names and stores are
not read as fallback inputs.

Compass supports every published MCP revision: 2026-07-28, 2025-11-25,
2025-06-18, 2025-03-26, and 2024-11-05. Stdio and Streamable HTTP accept the
older revisions through `initialize`; 2026-07-28 uses `server/discover`. HTTP
also serves the original 2024-11-05 HTTP+SSE transport at `/sse` and
`/messages`. See the root compatibility and migration documents for the
`--session-timeout` 0.4.x deprecation and 0.5.0 removal schedule.

The typed navigation tools return `compass.mcp.tool-result/1` structured
content and advertise no raw output schema. Their `compass.query/1` response is
preserved under `result`, with `agentView` and `semanticResultDigest` beside it;
clients that consumed the withdrawn `compass.code_context.v1` envelope must
follow the root migration guide. MCP `resultType` is
the separate protocol discriminator. Remaining text-only results are marked
deprecated from 0.4.0 in discovery but remain callable; removal is not
scheduled before typed replacements ship.

## Native evidence

Changes are checked by native unit, integration, CLI, protocol, format,
cross-platform, fuzz, sanitizer, and mutation tests. The root
[`COMPATIBILITY.md`](../../COMPATIBILITY.md) lists the standard verification
commands.

## Intentional hard cutovers

Compass is early in its lifecycle and currently prefers a clean contract over
backward-compatibility branches. When a format or schema is cut over:

1. old data must be archived or removed;
2. a fresh Compass build creates current artifacts;
3. unknown major versions are rejected;
4. migration documentation states the exact operator action.

See [`MIGRATION.md`](../../MIGRATION.md) for the current cutovers.

## Project lineage

Compass was inspired by
[Graphify](https://github.com/Graphify-Labs/graphify). The projects now evolve
independently; that attribution does not make Graphify part of Compass.
