# Modding Rustcraft

Mods add blocks, items, recipes, commands, key bindings and behavior with data files and Lua 5.4 scripts. There are no Java mods. A working example is in [`examples/mods/ruby/`](../examples/mods/ruby); the bundled [`mods/fullbright/`](../mods/fullbright) is a client-side one.

## Where mods live

`mods/` beside `saves/` (in the app: `~/Library/Application Support/Rustcraft/mods/`), as a folder or a `.zip`. The Mods screen (title and pause menu) lists them, turns them on and off, sets the load order and shows details and Lua errors. **Changes apply the next time Rustcraft starts**, because blocks and items get their IDs from the mods that are on.

```
mods/ruby/
  mod.ron          required
  main.lua         optional script
  data/            optional: blocks.ron items.ron recipes.ron creative_tabs.ron mob_drops.ron loot.ron
  assets/minecraft/    optional resource-pack layer: textures/, models/, sounds/, lang/, font/
```

### `mod.ron`

```ron
(
    id: "ruby",                  // a-z, 0-9, _ ; the namespace of everything it adds
    name: "Ruby",
    version: "1.0.0",
    authors: ["You"],
    description: "Shown in the Mods screen.",
    rustcraft: "1.2",            // the oldest game version it works with (optional)
    dependencies: ["other_mod"], // loaded before this one (optional)
    client_only: false,          // only changes the player's own screen (optional)
)
```

Everything a mod adds is named `<id>:<name>` (`ruby:ruby_block`); a mod can't add names in another namespace. Block and item IDs are handed out after the built-in ones, in load order, and saves store names, so worlds survive mods being reordered.

## Data files

The same RON formats as `assets/data/`, read the same way (see the header comments of `blocks.ron` and `items.ron` for every field):

| File | Contents |
|---|---|
| `blocks.ron` | a list of blocks (each gets an item unless `item: false`) |
| `items.ron` | a list of items |
| `recipes.ron` | `tags`, `shaped`, `shapeless`, `smelting`, `stonecutting`, `fuels`; every part optional, tags can be extended |
| `creative_tabs.ron` | tabs; items go at the end of the tab with the same `title`, other titles make a new tab |
| `mob_drops.ron` | `{"zombie": [(item: "ruby:ruby", min: 1, max: 1, chance: 0.05)]}` extra drops by mob |
| `loot.ron` | `{"table": [(rolls: (1, 3), entries: [(item: "...", weight: 5, min: 1, max: 2)])]}`; a built-in table's name (`simple_dungeon`) adds a pool to it, a new name (`ruby:treasure`) makes a table for `rc.loot` |

Textures are looked up by name in the mod's `assets/minecraft/textures/block/` and `item/` (then in the game's own, so a mod can reuse `redstone_block`). Use names nobody else uses (`ruby_block`).

## `main.lua`

Each mod runs in its own environment with one global, `rc`. The script runs **three times**: once at startup to collect what it registers, then once for the world (the server, host side) and once for the player's own screen (the client). Register at the top level, not inside callbacks. `rc.is_server` / `rc.is_client` tell the runs apart.

### Registering

```lua
rc.register_block("gem_block", { hardness = 3, textures = { all = "redstone_block" },   -- BlockDef fields
    on_use = function(ctx) end,          -- right-click (ctx: x y z player block)
    on_place = function(ctx) end,        -- placed by a player
    on_break = function(ctx) end,        -- broken by a player
    on_random_tick = function(ctx) end,  -- random ticks (randomTickSpeed per 16³ section per tick)
})
rc.register_item("wand", { texture = "stick", max_stack = 1,                           -- ItemDef fields
    on_use = function(ctx) end })        -- right-click (ctx: player item, x y z of the block aimed at)
rc.register_command("hello", { usage = "/hello [name]", cheat = false, host = false,
    run = function(ctx, args)            -- ctx: player command; args: list of strings
        return "Hello " .. (args[1] or "you")          -- a string, or a list of strings
        -- or: return nil, "an error shown in red"
    end })
rc.register_key("toggle", { name = "Toggle Thing", default = "G",     -- Options > Controls > Key Binds > Mods
    on_press = function() end })         -- runs on the client; at most 8 keys across all mods
```

Block and item fields are the fields of `blocks.ron` / `items.ron` as Lua tables (enums as strings: `tool = "Pickaxe"`). Leave out empty tables; lists are Lua arrays. Names without a namespace get your mod's.

### Events

`rc.on(name, function(event) ... end)` on the server:

| Event | Fields |
|---|---|
| `server_tick` | `tick` |
| `player_join`, `player_leave` | `player` (index), `name` |
| `block_break`, `block_place` | `player`, `block`, `x`, `y`, `z` |
| `entity_damage` | `entity`, `amount`, `source` |
| `entity_death` | `entity`, `source` |
| `chat` | `player`, `name`, `message` |
| `item_crafted` | `player`, `item`, `count` |

Handlers can't cancel the event; they run after it happened. Players are numbered from 0, the host first.

### The world (server side, inside callbacks, events, timers and commands)

| Call | |
|---|---|
| `rc.get_block(x, y, z)` | state text (`"minecraft:stone"`, `"minecraft:oak_stairs[facing=east,...]"`), `nil` if not loaded |
| `rc.set_block(x, y, z, state)` | `"ruby:ruby_block"` or with properties; raises an error for an unknown block |
| `rc.give(player, item, count)` | into the inventory; what doesn't fit is thrown |
| `rc.say(text [, player])` | chat line to everyone or one player |
| `rc.spawn(kind, x, y, z)` | a mob (`"zombie"`) |
| `rc.players()` | list of `{index, name, x, y, z, dimension}` |
| `rc.loot(table)` | rolls a loot table: list of `{item, count}` |
| `rc.tick()` | the world's tick count |
| `rc.after(ticks, f)`, `rc.every(ticks, f)` | timers, return an id; `rc.cancel(id)` |
| `rc.log(...)` | to the log |

### The client (`rc.client`, in key handlers and client timers)

| Call | |
|---|---|
| `rc.client.hud_text(id, text [, seconds])` | a line above the hotbar; no `seconds` keeps it until `hud_text(id, nil)` |
| `rc.client.set_brightness_override(level)` | light every block at least this much (0–15); `nil` is off |

### Sandbox

Only `table`, `string`, `math`, `utf8` and `coroutine` exist: no `os`, `io`, `debug`, `dofile` or `load` from files. `require("name")` loads `name.lua` from the mod's own folder and nowhere else. One call (a callback, event, command or timer) may run 2,000,000 Lua instructions and a Lua state 64 MiB; past that it is stopped with an error. An error in a callback is caught, logged and shown in the Mods screen; the game and the other mods keep running.

## Worlds and multiplayer

- Each world remembers the mods it was last played with. Opening it without one shows "Mods are missing" first. The blocks of a missing mod appear as unknown blocks (a glowstone-textured block) and **stay in the save with their real names**, coming back when the mod is installed; unknown items in chests read as unknown items.
- Server-side Lua runs on the LAN host. Guests need the same mods (same ids, versions and load order); otherwise the join is refused with the list of differences. `client_only` mods (Fullbright) are not compared, and can't add blocks or items.

## Fullbright

`mods/fullbright/` is an ordinary client-side mod, on by default. **G** (rebindable under Key Binds > Mods) lights everything at full brightness and shows "Fullbright: ON" above the hotbar; G again turns it off. It works in single player and on LAN.
