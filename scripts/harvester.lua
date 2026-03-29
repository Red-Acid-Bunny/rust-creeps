function decide(ctx)
    if ctx.carry > 0 then
        local source = ctx.nearby_sources[1]
        local source_ok = source and source.resource_amount > 0

        if ctx.carry >= ctx.carry_capacity or not source_ok then
            if #ctx.nearby_spawns > 0 then
                local spawn = ctx.nearby_spawns[1]
                local d = distance(ctx.pos, spawn.pos)
                if d <= 1 then
                    return { type = "transfer", target_id = spawn.id, resource = "energy", amount = ctx.carry }
                else
                    return { type = "move", target = { x = spawn.pos.x, y = spawn.pos.y },
                             reason = "delivering energy (" .. ctx.carry .. "/" .. ctx.carry_capacity .. ")" }
                end
            end
            return { type = "idle", reason = "carrying but no spawn" }
        end

        local d = distance(ctx.pos, source.pos)
        if d <= 1 then
            return { type = "harvest", target_id = source.id }
        else
            return { type = "move", target = { x = source.pos.x, y = source.pos.y },
                     reason = "back to source (" .. ctx.carry .. "/" .. ctx.carry_capacity .. ")" }
        end
    end

    if #ctx.nearby_sources > 0 then
        local source = ctx.nearby_sources[1]
        if source.resource_amount <= 0 then return { type = "idle", reason = "source depleted" } end
        local d = distance(ctx.pos, source.pos)
        if d <= 1 then return { type = "harvest", target_id = source.id } end
        return { type = "move", target = { x = source.pos.x, y = source.pos.y },
                 reason = "going to source (dist " .. d .. ")" }
    end

    return { type = "idle", reason = "no sources in range" }
end
