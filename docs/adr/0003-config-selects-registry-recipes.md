# Config selects tools; the registry owns recipes

The user config declares which tools are wanted (`[tools]`), never how to generate their completions. Completion recipes live in registry items, one file per workspace, which can be embedded, dropped into the user registry directory, or written inline with `[[registry]]`. This separates "what I use" from "how this CLI emits completion", lets a registry item serve many users, and keeps a config diff from silently changing executable commands.
