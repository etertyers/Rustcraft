# Rustcraft: blocks and items (Minecraft Java Edition 1.0.0)

Source version: **Java Edition 1.0.0** (18 Nov 2011), the first official "Release". The very first public build (rd-132211, May 2009) had only grass and cobblestone, so it's too sparse to plan from. Numbers are the original numeric IDs; these can be reused as the game's `BlockId`/`ItemId` values so the list maps straight into `blocks.ron` and `items.ron`.

**Scope:** every block and item below is registered in the game (CLAUDE.md M2 and M7). Tags say when its behavior gets built: ✅ in scope (milestone tags are approximate; CLAUDE.md §7 is authoritative) · ⏸ later / optional · ❌ registered and placeable, but special behavior (redstone, Nether, End, enchanting, brewing) is deferred until after M13.

## Blocks (IDs 0–122)

**Terrain and natural** ✅ M2–M3
0 Air · 1 Stone · 2 Grass · 3 Dirt · 4 Cobblestone · 7 Bedrock · 12 Sand · 13 Gravel · 17 Log (oak, spruce, birch) · 18 Leaves (oak, spruce, birch) · 24 Sandstone · 48 Moss Stone · 49 Obsidian · 78 Snow (layer) · 79 Ice · 80 Snow Block · 82 Clay · 110 Mycelium ⏸

**Ores and mineral blocks** ✅ M3 (blocks), M7 (drops)
14 Gold Ore · 15 Iron Ore · 16 Coal Ore · 21 Lapis Lazuli Ore · 56 Diamond Ore · 73 Redstone Ore · 74 Glowing Redstone Ore · 22 Lapis Block · 41 Gold Block · 42 Iron Block · 57 Diamond Block

**Fluids** ✅ M9
8 Water (flowing) · 9 Water (still) · 10 Lava (flowing) · 11 Lava (still)

**Plants** ✅ M3 (worldgen), M8 (farming)
6 Sapling (oak, spruce, birch) · 31 Tall Grass / Fern · 32 Dead Bush · 37 Dandelion · 38 Rose · 39 Brown Mushroom · 40 Red Mushroom · 59 Wheat (crop) · 60 Farmland · 81 Cactus · 83 Sugar Cane · 86 Pumpkin · 103 Melon · 104 Pumpkin Stem · 105 Melon Stem · 106 Vines · 111 Lily Pad ⏸ · 99 Huge Brown Mushroom ⏸ · 100 Huge Red Mushroom ⏸

**Building blocks** ✅ M2/M8
5 Wooden Planks · 20 Glass · 35 Wool (16 colors) · 43 Double Slab · 44 Slab (stone, sandstone, wooden, cobblestone, brick, stone brick) · 45 Bricks · 47 Bookshelf · 53 Wooden Stairs · 67 Cobblestone Stairs · 98 Stone Bricks (normal, mossy, cracked) · 101 Iron Bars · 102 Glass Pane · 108 Brick Stairs · 109 Stone Brick Stairs · 19 Sponge ⏸

**Functional / interactive** ✅ M5–M8
26 Bed · 50 Torch · 54 Chest · 58 Crafting Table · 61 Furnace · 62 Furnace (lit) · 63 Sign (standing) · 68 Sign (wall) · 64 Wooden Door · 71 Iron Door · 65 Ladder · 85 Fence · 107 Fence Gate · 96 Trapdoor · 89 Glowstone · 91 Jack o'Lantern · 92 Cake ⏸ · 84 Jukebox ⏸ · 46 TNT ⏸ · 30 Cobweb ⏸ · 51 Fire ⏸ · 52 Monster Spawner ⏸ · 97 Silverfish Stone ⏸

**Redstone and rails** ❌
23 Dispenser · 25 Note Block · 27 Powered Rail · 28 Detector Rail · 29 Sticky Piston · 33 Piston · 34 Piston Head · 36 Moving Piston · 55 Redstone Wire · 66 Rail · 69 Lever · 70 Stone Pressure Plate · 72 Wooden Pressure Plate · 75 Redstone Torch (off) · 76 Redstone Torch (on) · 77 Stone Button · 93 Repeater (off) · 94 Repeater (on)

**Nether** ❌
87 Netherrack · 88 Soul Sand · 90 Nether Portal · 112 Nether Brick · 113 Nether Brick Fence · 114 Nether Brick Stairs · 115 Nether Wart (crop)

**End, enchanting, brewing** ❌
116 Enchantment Table · 117 Brewing Stand · 118 Cauldron · 119 End Portal · 120 End Portal Frame · 121 End Stone · 122 Dragon Egg

**Unused/legacy** ❌
95 Locked Chest (April Fools' leftover)

## Items (IDs 256–382, plus music discs)

**Tools** ✅ M7
Wood: 268 Sword · 269 Shovel · 270 Pickaxe · 271 Axe · 290 Hoe
Stone: 272 Sword · 273 Shovel · 274 Pickaxe · 275 Axe · 291 Hoe
Iron: 267 Sword · 256 Shovel · 257 Pickaxe · 258 Axe · 292 Hoe
Gold: 283 Sword · 284 Shovel · 285 Pickaxe · 286 Axe · 294 Hoe
Diamond: 276 Sword · 277 Shovel · 278 Pickaxe · 279 Axe · 293 Hoe
Other: 259 Flint and Steel ⏸ · 359 Shears · 346 Fishing Rod ⏸ · 261 Bow ✅ M10 · 262 Arrow ✅ M10

**Armor** ⏸ (after M10)
Leather: 298 Cap · 299 Tunic · 300 Pants · 301 Boots
Chain: 302 Helmet · 303 Chestplate · 304 Leggings · 305 Boots
Iron: 306 Helmet · 307 Chestplate · 308 Leggings · 309 Boots
Diamond: 310 Helmet · 311 Chestplate · 312 Leggings · 313 Boots
Gold: 314 Helmet · 315 Chestplate · 316 Leggings · 317 Boots

**Materials** ✅ M7–M8
263 Coal / Charcoal · 264 Diamond · 265 Iron Ingot · 266 Gold Ingot · 280 Stick · 281 Bowl · 287 String · 288 Feather · 289 Gunpowder · 295 Seeds · 296 Wheat · 318 Flint · 334 Leather · 336 Brick · 337 Clay Ball · 338 Sugar Canes · 339 Paper · 340 Book · 341 Slimeball · 351 Dye (16 colors, incl. Bone Meal, Ink Sac, Cocoa Beans, Lapis Lazuli) · 352 Bone · 353 Sugar · 361 Pumpkin Seeds · 362 Melon Seeds · 331 Redstone Dust ❌ · 348 Glowstone Dust ⏸

**Food** ✅ M10
260 Apple · 282 Mushroom Stew · 297 Bread · 319 Raw Porkchop · 320 Cooked Porkchop · 322 Golden Apple · 349 Raw Fish · 350 Cooked Fish · 357 Cookie · 360 Melon Slice · 363 Raw Beef · 364 Steak · 365 Raw Chicken · 366 Cooked Chicken · 367 Rotten Flesh · 375 Spider Eye · 354 Cake ⏸ · 344 Egg ⏸

**Placeable items** (item form of a block) ✅ with their block
321 Painting ⏸ · 323 Sign · 324 Wooden Door · 330 Iron Door · 355 Bed · 356 Redstone Repeater ❌ · 379 Brewing Stand ❌ · 380 Cauldron ❌

**Buckets** ✅ M9
325 Bucket · 326 Water Bucket · 327 Lava Bucket · 335 Milk Bucket ⏸

**Vehicles and utility** ⏸
328 Minecart · 342 Storage Minecart · 343 Powered Minecart · 329 Saddle · 333 Boat · 332 Snowball · 345 Compass · 347 Clock · 358 Map

**Nether / End / brewing items** ❌
368 Ender Pearl · 369 Blaze Rod · 370 Ghast Tear · 371 Gold Nugget · 372 Nether Wart · 373 Potion · 374 Glass Bottle · 376 Fermented Spider Eye · 377 Blaze Powder · 378 Magma Cream · 381 Eye of Ender · 382 Glistering Melon

**Music discs** ⏸ (2256–2266)
13 · cat · blocks · chirp · far · mall · mellohi · stal · strad · ward · 11

**Not in 1.0.0:** spawn eggs (1.1), emeralds and jungle blocks (1.2–1.3), anvils, item frames, flower pots (1.4). Don't add them.
