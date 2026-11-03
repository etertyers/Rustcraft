# Game logic rules

Nested file: only rules that also appear in the root CLAUDE.md or in a milestone entry (`docs/milestones/`).

## Tick model
- Game logic runs at a fixed 20 ticks/second, like Minecraft. Entity positions are interpolated on the client.
- Single player runs an in-process server with one client, so there is one code path (M36).

## Server-side rules (from M36)
- The server owns the world, chunks, ticks, mobs, redstone, fluids, dimensions, inventories and saving. The client owns rendering, input, UI, sound and prediction.
- Every new feature runs on the server and syncs to clients. Each such milestone includes at least one test with a server and two in-process clients that checks they agree.
- Client-side prediction covers only the local player's movement and block breaking/placing, corrected by the server.
- Per-player data (position, inventory, ender chest, XP, effects, spawn point, advancements) saves in `saves/<world>/playerdata/<name>.ron`.
- Cheat commands need Allow Cheats (or an operator on LAN). Commands run on the server and use vanilla Java 1.21 syntax with Tab completion and vanilla error wording.
- Mods run in the server's mod host (`modding.rs`) and the client's Lua; both are sandboxed.

## Sounds
- Sounds are events: game code queues them (`sounds.rs`) and the server sends them to clients; `audio/` plays them.

## Values
- Vanilla values from the milestone entry are used as given. If one isn't given, use the best-known modern Java value, record it in `docs/DECISIONS.md`, and keep it as a named constant (`config.rs`; combat in `config::combat`).
- Test pure logic without a GPU.
