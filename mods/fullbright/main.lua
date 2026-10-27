-- Fullbright: a key that lights every block as if it were in full light.
-- Client side only: the key and the brightness override exist in the
-- player's own game, so it works in single player and on LAN alike.

local on = false

rc.register_key("toggle", {
    name = "Toggle Fullbright",
    default = "G",
    on_press = function()
        on = not on
        -- 15 is the brightest light level; nil puts the real light back.
        rc.client.set_brightness_override(on and 15 or nil)
        if on then
            rc.client.hud_text("fullbright", "Fullbright: ON")
        else
            -- "OFF" shows for two seconds; "ON" stays while it is on.
            rc.client.hud_text("fullbright", "Fullbright: OFF", 2)
        end
    end,
})
