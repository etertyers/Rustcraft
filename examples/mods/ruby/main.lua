-- The Ruby example mod. The data/ folder adds the block, the item and the
-- recipes; this script adds a command and reacts to breaking a block.

-- /rubies [count]: gives rubies to whoever typed it.
rc.register_command("rubies", {
    usage = "/rubies [count]",
    cheat = true, -- only where commands are allowed
    run = function(ctx, args)
        local count = tonumber(args[1]) or 1
        if count < 1 or count > 64 then
            return nil, "The count must be from 1 to 64"
        end
        rc.give(ctx.player, "ruby:ruby", count)
        return "Gave you " .. count .. " ruby"
    end,
})

-- Breaking a block of ruby: say so, and drop a little gold in thanks.
rc.on("block_break", function(event)
    if event.block == "ruby:ruby_block" then
        rc.say("A block of ruby was broken at " .. event.x .. ", " .. event.y .. ", " .. event.z, event.player)
        rc.give(event.player, "minecraft:gold_nugget", 3)
    end
end)

-- A ruby in the hand: right-click to roll the ruby:treasure loot table.
rc.register_item("treasure_map", {
    texture = "map_empty",
    max_stack = 1,
    on_use = function(ctx)
        for _, found in ipairs(rc.loot("ruby:treasure")) do
            rc.give(ctx.player, found.item, found.count)
        end
        rc.say("The map crumbles and leaves treasure behind.", ctx.player)
    end,
})
