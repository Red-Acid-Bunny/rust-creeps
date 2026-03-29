function decide(ctx)
    -- Стоимость body parts (должна совпадать с Rust)
    local BODY_COST = { move = 50, work = 100, carry = 50, attack = 80, tough = 10 }

    -- ── Memory ────────────────────────────────────
    -- Инициализация при первом вызове
    if not Memory.creeps then Memory.creeps = {} end
    if not Memory.spawn_count then Memory.spawn_count = 0 end

    -- Обновляем: помечаем себя живым
    Memory.creeps[ctx.id] = {
        tick = ctx.tick,
        pos = { x = ctx.pos.x, y = ctx.pos.y },
        carry = ctx.carry,
    }

    -- Считаем количество живых крипов
    local function count_creeps()
        local n = 0
        for _ in pairs(Memory.creeps) do n = n + 1 end
        return n
    end

    -- ── Хелперы ──────────────────────────────────
    local function find_nearest_reachable_source()
        -- Собираем кандидатов с ресурсами, сортируем по дистанции
        local candidates = {}
        for _, src in ipairs(ctx.nearby_sources) do
            if src.resource_amount > 0 then
                local d = distance(ctx.pos, src.pos)
                candidates[#candidates + 1] = { src = src, dist = d }
            end
        end
        table.sort(candidates, function(a, b) return a.dist < b.dist end)

        -- Пробуем find_path начиная с ближайшего
        for _, entry in ipairs(candidates) do
            if entry.dist <= 1 then
                return entry.src, 1
            end
            local path = find_path(ctx.pos, entry.src.pos)
            if path then
                return entry.src, #path
            end
        end
        return nil, nil
    end

    local function find_nearest_spawn()
        local best = nil
        local best_dist = math.huge
        for _, sp in ipairs(ctx.nearby_spawns) do
            local d = distance(ctx.pos, sp.pos)
            if d < best_dist then
                best_dist = d
                best = sp
            end
        end
        return best, best_dist
    end

    -- ── Спавн ────────────────────────────────────
    -- Спавним нового крипа, если:
    --   1. Есть доступный спавн
    --   2. Спавн не на кулдауне
    --   3. Хватает энергии
    --   4. Крипов меньше лимита
    local function try_spawn(max_creeps)
        if count_creeps() >= max_creeps then
            return nil
        end

        local spawn = find_nearest_spawn()
        if not spawn then
            return nil
        end

        if spawn.cooldown > 0 then
            return nil
        end

        local body = { "move", "move", "work", "carry" }
        local cost = 0
        for _, part in ipairs(body) do
            cost = cost + (BODY_COST[part] or 0)
        end

        if spawn.resource_amount < cost then
            return nil
        end

        Memory.spawn_count = Memory.spawn_count + 1

        return {
            type = "spawn",
            target_id = spawn.id,
            body = body,
            name = "worker_" .. Memory.spawn_count
        }
    end

    -- ── Основная логика ──────────────────────────
    -- Несём ресурс
    if ctx.carry > 0 then
        local source, src_dist = find_nearest_reachable_source()
        local full = ctx.carry >= ctx.carry_capacity
        local source_gone = (source == nil)

        -- Идём доставлять, если полный или нет доступных источников
        if full or source_gone then
            local spawn, sp_dist = find_nearest_spawn()
            if not spawn then
                return { type = "idle", reason = "carrying but no spawn in range" }
            end
            if sp_dist <= 1 then
                return { type = "transfer", target_id = spawn.id, resource = "energy", amount = ctx.carry }
            end
            return { type = "moveto", target = { x = spawn.pos.x, y = spawn.pos.y },
                     reason = "delivering energy (" .. ctx.carry .. "/" .. ctx.carry_capacity .. ")" }
        end

        -- Не полный и есть доступный источник — продолжаем добычу
        if src_dist <= 1 then
            return { type = "harvest", target_id = source.id }
        end
        return { type = "moveto", target = { x = source.pos.x, y = source.pos.y },
                 reason = "back to source (" .. ctx.carry .. "/" .. ctx.carry_capacity .. ")" }
    end

    -- Пустой — пробуем спавн
    local spawn_action = try_spawn(3)
    if spawn_action then
        return spawn_action
    end

    -- Пустой — ищем ближайший доступный источник
    local source, src_dist = find_nearest_reachable_source()
    if source then
        if src_dist <= 1 then
            return { type = "harvest", target_id = source.id }
        end
        return { type = "moveto", target = { x = source.pos.x, y = source.pos.y },
                 reason = "going to source (dist " .. src_dist .. ")" }
    end

    -- Все источники недоступны
    return { type = "idle", reason = "no reachable sources in range" }
end
