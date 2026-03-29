-- harvester.lua — Screeps-like AI для сбора энергии
--
-- Архитектура:
--   before_tick(game) — глобальный хук, вызывается ОДИН раз в начале тика.
--     Отвечает за спавн новых крипов. Работает даже когда крипов нет на карте.
--   decide(ctx) — вызывается для КАЖДОГО крипа индивидуально.
--     Отвечает за движение, добычу и доставку ресурсов.

-- ═══════════════════════════════════════════════════
--  Глобальный хук: управление спавном
-- ═══════════════════════════════════════════════════
function before_tick(game)
    local BODY_COST = { move = 50, work = 100, carry = 50, attack = 80, tough = 10 }
    local MAX_CREEPS = 3

    -- Инициализация Memory
    if not Memory.creeps then Memory.creeps = {} end

    -- Считаем живых крипов через game.creeps от Rust,
    -- а НЕ через Memory.creeps — Memory может устареть
    -- (крип умер, скрипт перезагружен, и т.д.)
    if #game.creeps >= MAX_CREEPS then
        return nil
    end

    -- Ищем доступный спавн
    for _, sp in ipairs(game.spawns) do
        if sp.cooldown == 0 then
            local body = { "move", "move", "work", "carry" }
            local cost = 0
            for _, part in ipairs(body) do
                cost = cost + (BODY_COST[part] or 0)
            end

            if sp.resource_amount >= cost then
                -- Имя оставляем пустым — Rust сгенерирует уникальное
                -- автоматически (worker_1, worker_2, ...).
                -- Если имя занято, Rust добавит суффикс (worker_1_2).
                return {
                    type = "spawn",
                    target_id = sp.id,
                    body = body,
                    name = "",
                }
            end
        end
    end

    return nil
end

-- ═══════════════════════════════════════════════════
--  Per-creep: decide(ctx)
-- ═══════════════════════════════════════════════════
function decide(ctx)
    -- Обновляем Memory: помечаем себя живым
    Memory.creeps[ctx.id] = {
        tick = ctx.tick,
        pos = { x = ctx.pos.x, y = ctx.pos.y },
        carry = ctx.carry,
    }

    -- ── Хелперы ──────────────────────────────────

    -- Ищем ближайший доступный источник.
    -- Сортируем по дистанции, A* только для ближайших.
    local function find_nearest_reachable_source()
        local candidates = {}
        for _, src in ipairs(ctx.nearby_sources) do
            if src.resource_amount > 0 then
                local d = distance(ctx.pos, src.pos)
                candidates[#candidates + 1] = { src = src, dist = d }
            end
        end
        table.sort(candidates, function(a, b) return a.dist < b.dist end)

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

    -- ── Несём ресурс ────────────────────────────
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

        -- Не полный и есть источник — продолжаем добычу
        if src_dist <= 1 then
            return { type = "harvest", target_id = source.id }
        end
        return { type = "moveto", target = { x = source.pos.x, y = source.pos.y },
                 reason = "back to source (" .. ctx.carry .. "/" .. ctx.carry_capacity .. ")" }
    end

    -- ── Пустой — ищем источник ───────────────────
    local source, src_dist = find_nearest_reachable_source()
    if source then
        if src_dist <= 1 then
            return { type = "harvest", target_id = source.id }
        end
        return { type = "moveto", target = { x = source.pos.x, y = source.pos.y },
                 reason = "going to source (dist " .. src_dist .. ")" }
    end

    return { type = "idle", reason = "no reachable sources in range" }
end
