# Architecture: file map

Moved word for word from the root CLAUDE.md §4 (paths corrected where noted). The root file has the short module map. This list is not exhaustive; `ls src/<folder>` is the source of truth.

```
src/
  main.rs            # entry: create window, run App
  app.rs             # winit event loop, owns State, fixed-timestep loop
  config.rs          # constants (chunk size, world height, render distance, physics values)
  render/            # wgpu device, pipelines, camera uniforms, texture atlas, shaders (*.wgsl)
  world/
    block.rs         # BlockId (u16) + BlockRegistry (data-driven properties)
    chunk.rs         # Chunk storage (16x16x16 sections), get/set
    mesh.rs          # chunk → vertex buffer (face culling, then greedy meshing)
    worldgen/        # terrain, biomes, caves, ores, trees (`gen` is reserved in Rust 2024)
    manager.rs       # load/unload around player, worker threads, dirty tracking
    light.rs         # sky light + block light propagation
    fluid.rs         # water/lava flow (M12)
    save.rs          # region files
  player/            # controller, physics, collision (AABB), raycast, interaction, stats
  entity/            # mobs, item drops
  inventory/         # items, stacks, inventory, crafting recipes
  ui/                # HUD, hotbar, inventory screen, menus (egui)
  pack/              # resource packs: layers, animations, block and item models
  look.rs            # everything texture-derived, rebuilt when the packs change
  figure.rs          # the player as drawn: the hand, or the model and what it holds
  game/view.rs       # swing, hand dip, bobbing, body turn, F5 views
  game/combat.rs     # cooldowns, damage, crits, sweeps, the shield (constants in config::combat)
  render/{player,hand,held}.rs   # player model and poses, first-person hand, held item geometry
  world/{redstone,piston,rails,explosion}.rs   # signals and ticks, what pistons move, rail joining, blast rays
  game/{redstone,logistics,minecarts,explosion}.rs   # carrying out redstone events, hoppers and droppers, carts, TNT
  entity/{minecart,tnt}.rs   # rail physics, primed TNT
  entity/{mobs,mob,ai}.rs   # mob definitions (mobs.ron), a living mob, what each kind decides
  game/{mobs,mob_acts,animals}.rs   # mob ticks and spawning, what mobs do to the world, the player and animals
  world/worldgen/{climate,terrain,underground,features,ores}.rs   # climate noise and the biome table, land, surfaces and caves, cave biomes and geodes, trees and plants, veins
  inventory/{enchant,anvil,grindstone}.rs   # enchantments and the table's offers, the anvil's rules, the grindstone's
  player/experience.rs, entity/orb.rs   # levels and the bar, experience orbs
  game/{experience,benches,gear}.rs   # orbs and where they come from, enchanting table / anvil / grindstone screens, what enchantments do in hand
  ui/glint.rs   # the enchantment glint on icons (held and dropped items glint in shader.wgsl)
  world/structure/   # structure placement and per-chunk writing, templates (RON) and jigsaw joining, loot; designs/ draws the shipped templates (--write-structures)
  entity/villager.rs, game/villagers.rs   # professions, levels and trades; job sites, beds, schedule, breeding, golems, trading
  game/{spawners,capture}.rs   # spawners spawning, the creative capture tool (F8/F9 corners, F10 save)
  world/chorus.rs   # chorus plants: how one grows, joins its neighbors and what holds it up
  game/spawn_test.rs, world/worldgen/flower_stats.rs   # --spawn-test and --flower-test
  world/{dimension,portal}.rs, world/worldgen/nether.rs   # dimensions, portal frames, the Nether's terrain, biomes and features
  game/{dimensions,portals,nether_mobs}.rs, entity/fireball.rs   # switching dimensions, lighting and travelling through portals, ghasts' and blazes' fireballs, bartering, striders
  world/structure/{nether,fortress,bastion}.rs, inventory/smithing.rs   # the Nether's structures (placement, fortresses, bastions and its ruined portals), the netherite upgrade
  player/effects.rs, inventory/{potion,brewing}.rs, entity/thrown.rs, game/potions.rs   # status effects, potions and the brewing tree, the brewing stand, splash potions, drinking/cauldrons/food effects
  world/{end_portal,worldgen/end}.rs, entity/{dragon,crystal,breath,eye}.rs   # the End portal's ring of frames, the End's island/pillars/podium, the ender dragon (phases and parts), crystals, fireballs and breath clouds, eyes of ender
  game/{dragon,end_portal}.rs, render/dragon.rs, credits.rs   # the fight (breaking blocks, healing, death, exit portal), eyes/portal travel/credits, the dragon and crystal models, the poem and credits text
  inventory/armor.rs, game/{armor,elytra,gateways,shulkers}.rs, entity/bullet.rs, world/structure/end.rs   # armor slots and protection, elytra gliding and rockets, End gateways, shulkers' bullets and shulker boxes, End cities and ships
  cli.rs, worldlist.rs   # the command line (flags in §3, usage text), the Select World list read on a background thread from each world's small world.meta.ron
  ui/{stack,geometry,doll,creative,recipe_book}.rs   # the open screens that own the mouse, where panels and slots are, the player's picture in the inventory, the tabbed creative screen, the recipe book panel
  inventory/{creative,book}.rs, assets/data/creative_tabs.ron   # creative tabs' contents, search and click rules; the recipe book's recipes, categories and placing
  entity/boat.rs, game/boats.rs, render/{boat,lightmap}.rs, world/plants.rs   # boats and rafts (floating, rowing, seats), placing/riding/breaking them, their models; the lightmap formula (tested copy of the shader's); where sugar cane may stand
  world/worldgen/{caves,normal_noise,cave_stats}.rs   # vanilla's final density (cheese, spaghetti, noodle, pillars, entrances), its aquifers, worm tunnels and canyons, vanilla's normal noise, --cave-test and --cave-slice
  net/lan.rs, lantest.rs   # Open to LAN's listener and announcements, the Multiplayer screen's finder; --lan-test's bots
  server.rs, client.rs, net/{protocol,transport}.rs, game/sync.rs   # the server (the world and every player; chunks, edits, snapshots and sounds to each client), the client link (prediction and replay), the messages and connections, a game as a client's copy of the world
  bindings.rs, ui/options.rs   # key bindings (actions, categories, vanilla defaults and options.txt ids), the Options screens and Key Binds list
  ui/{font,text_field}.rs   # the Minecraft font (bitmap glyphs from the packs made into a TrueType font for egui, § codes, text shadow), the one-line text box (anvil names, chat)
  entity/bobber.rs, game/fishing.rs   # the bobber's physics, bites and the loot roll (tables in world/structure/loot.rs), casting and reeling in
  game/farming.rs   # crops on random ticks (vanilla's growth formula), farmland, hoes, bone meal, the composter, saplings, mushrooms; breeding is in game/animals.rs
  mods/{mod,lua,client}.rs, game/{modding,random_ticks}.rs, ui/mods.rs, mods/ examples/mods/   # (repo-root mods/ holds the bundled fullbright; examples/mods/ the ruby example) mod folders and the set chosen at startup, the sandboxed Lua runtime and its API, the client's Lua (keys, HUD text, brightness), the server's mod host and events, random ticks, the Mods screen, the bundled Fullbright and the Ruby example (docs/modding.md)
  ui/chat.rs, game/commands.rs   # the chat lines, box and Chat Settings; the commands (table, selectors, Tab completion), run on the server
  game/commands/{extra,world,components}.rs, game/{selector,modes,admin,weather}.rs, ui/modes.rs
  game/commands/{execute,board_cmds,text_cmds,data_cmds}.rs, game/{board,board_hooks,text,nbt,predicate}.rs, ui/board.rs   # (M44) /execute; /scoreboard, /team, /tag, /trigger; /tellraw, /title, /bossbar; /data, /item, /loot; the scoreboard, its criteria hooks and views; JSON text; NBT and paths; predicates; the sidebar, bars and titles drawn   # (M43) the everyday, block-editing and item-component commands; selectors in full; spectator and adventure rules and spectating; operators, bans and the whitelist; the weather state; the F3+F4 switcher and spectator menu
  game/commands/{function_cmds,server_cmds}.rs, game/{datapack,border,command_blocks}.rs, ui/command_block.rs   # (M45) /function, /return, /schedule, /datapack, /reload; /tick, /worldborder, /forceload; reading data packs and the packs enabled; the world border; command blocks (modes, chains, power, the minecart); the editor screen
  advancements.rs, game/advance.rs, assets/data/advancements.ron, inventory/sweep.rs   # the advancement tree and what meets its criteria, the events and toasts in play (L opens the screen), the recipe sweep test
  audio/             # sound events: the merged sounds.json table, kira playback, block and mob sound names
  placeholders.rs    # generates the placeholder blocks and items from vanilla's data (--gen-placeholders)
  game/sounds.rs     # the player's, mobs' and surroundings' sounds, queued as events
assets/
  textures/          # Kenzo's PNGs (the base resource layer, 1.12 layout) → packed into an atlas at startup
  # (no assets/sounds/ or assets/sounds.json in the repo today: sounds come from assets/vanilla; if added they are laid out like a pack's assets/minecraft and win over vanilla's)
  structures/        # structure templates, one folder per pool (village/<style>/{centers,streets,houses}, desert_temple...)
  skins/             # alex.png, steve-skin.png
  vanilla/           # git-ignored: vanilla 26.3 assets, the lowest layer (scripts/fetch-vanilla-assets.sh)
  data/              # blocks.ron, items.ron, recipes.ron, placeholder_blocks.ron, placeholder_items.ron
resourcepacks/       # beside saves/: Minecraft resource packs, picked in the Resource Packs screen
docs/
  blocks-and-items.md
  placeholders.md    # every placeholder block and item (generated)
  DECISIONS.md
  screenshots/
```

Corrections made when this list moved out of CLAUDE.md: `assets/structures/` was drawn inside `src/` and now sits under `assets/`; `assets/sounds/` and `assets/sounds.json` do not exist in the repo and are noted as such; `assets/skins/` was added; `docs/sounds.md` (named in M0's entry) does not exist either.
