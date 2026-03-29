use std::collections::{HashMap, HashSet};

use crate::game::config::GameConfig;
use crate::game::pathfinding;
use crate::game::types::*;
use crate::script::ScriptEngine;

// ═══════════════════════════════════════
//  GameState — игровой мир
// ═══════════════════════════════════════

pub struct GameState {
    pub width: usize,
    pub height: usize,
    pub tiles: Vec<Vec<TileType>>,
    pub entities: Vec<Entity>,
    entity_index: HashMap<String, usize>,
    pub tick: u64,
    pub view_range: i32,
    pub harvest_rate: u32,
    pub source_regen_rate: u32,
    pub max_source_amount: u32,
}

impl GameState {
    /// Creates the world from a string-based map description (backward compat for tests).
    /// '#' wall  '~' swamp  '.' plain  'S' spawn  'E' source  'c' creep
    pub fn from_map(map_strings: &[&str]) -> Self {
        let config = GameConfig::with_defaults(
            map_strings.iter().map(|s| s.to_string()).collect(),
        );
        Self::from_config(&config)
    }

    /// Creates GameState from a GameConfig (loaded from JSON).
    /// Uses config fields for spawn energy, source amounts, rates, etc.
    pub fn from_config(config: &GameConfig) -> Self {
        let height = config.map.len();
        let width = config.map.iter().map(|s| s.len()).max().unwrap_or(0);
        let mut tiles = vec![vec![TileType::Plain; width]; height];
        let mut entities = Vec::new();
        let mut creep_count = 0u32;
        let mut source_count = 0u32;

        for (y, row) in config.map.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                if x >= width {
                    break;
                }
                tiles[y][x] = match ch {
                    '#' => TileType::Wall,
                    '~' => TileType::Swamp,
                    _ => TileType::Plain,
                };
                match ch {
                    'S' => entities.push(Entity::new_spawn(
                        "spawn1",
                        Position {
                            x: x as i32,
                            y: y as i32,
                        },
                        config.spawn_initial_energy,
                    )),
                    'E' => {
                        source_count += 1;
                        entities.push(Entity::new_source(
                            &format!("source_{}", source_count),
                            Position {
                                x: x as i32,
                                y: y as i32,
                            },
                            config.source_initial_amount,
                        ))
                    }
                    'c' => {
                        creep_count += 1;
                        entities.push(Entity::new_creep(
                            &format!("worker_{}", creep_count),
                            Position {
                                x: x as i32,
                                y: y as i32,
                            },
                            vec![
                                BodyPart::Move,
                                BodyPart::Move,
                                BodyPart::Work,
                                BodyPart::Carry,
                            ],
                        ));
                    }
                    _ => {}
                }
            }
        }

        let entity_index: HashMap<String, usize> = entities
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id.clone(), i))
            .collect();

        let world = GameState {
            width,
            height,
            tiles,
            entities,
            entity_index,
            tick: 0,
            view_range: config.view_range,
            harvest_rate: config.harvest_rate,
            source_regen_rate: config.source_regen_rate,
            max_source_amount: config.max_source_amount,
        };

        tracing::info!(
            width = world.width,
            height = world.height,
            creeps = world
                .entities
                .iter()
                .filter(|e| e.entity_type == EntityType::Creep)
                .count(),
            sources = world
                .entities
                .iter()
                .filter(|e| e.entity_type == EntityType::Source)
                .count(),
            spawns = world
                .entities
                .iter()
                .filter(|e| e.entity_type == EntityType::Spawn)
                .count(),
            "world created"
        );

        world
    }

    /// Собирает позиции непроходимых сущностей (sources, spawns).
    pub fn block_positions(&self) -> Vec<Position> {
        self.entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Source || e.entity_type == EntityType::Spawn)
            .map(|e| e.pos)
            .collect()
    }

    /// BFS от целевой позиции — находит ближайшую проходимую клетку.
    /// Используется когда цель сама непроходима (стена, источник, спавн).
    /// Возвращает None если проходимых клеток нет в радиусе max_dist.
    fn find_nearest_walkable(&self, target: Position, max_dist: u32) -> Option<Position> {
        if self.is_walkable(target) {
            return Some(target);
        }
        let directions: [(i32, i32); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];
        let mut visited: HashSet<Position> = HashSet::new();
        visited.insert(target);
        let w = self.width as i32;
        let h = self.height as i32;
        for _ in 1..=max_dist {
            let mut frontier: Vec<Position> = Vec::new();
            for &pos in &visited {
                for &(dx, dy) in &directions {
                    let next = Position { x: pos.x + dx, y: pos.y + dy };
                    if next.x >= 0 && next.y >= 0 && next.x < w && next.y < h {
                        if !visited.contains(&next) && !frontier.contains(&next) {
                            if self.is_walkable(next) {
                                return Some(next);
                            }
                            frontier.push(next);
                        }
                    }
                }
            }
            for pos in frontier {
                visited.insert(pos);
            }
            if visited.len() > (max_dist as usize * max_dist as usize * 4) {
                break;
            }
        }
        None
    }

    /// Регистрирует глобальные Lua-функции, зависящие от состояния мира.
    /// Вызывать один раз после создания GameState и ScriptEngine.
    pub fn register_lua_functions(&self, engine: &ScriptEngine) -> mlua::Result<()> {
        let tiles = self.tiles.clone();
        let width = self.width as i32;
        let height = self.height as i32;
        let blockers = self.block_positions();

        engine.with_lua(|lua| {
            // ── find_path(from, to [, opts]) ──────────────
            let tiles_fp = tiles.clone();
            let w = width;
            let h = height;
            let bl = blockers.clone();

            let find_path_fn = lua.create_function(
                move |lua: &mlua::Lua,
                      (from, to, opts): (mlua::Table, mlua::Table, Option<mlua::Table>)|
                      -> mlua::Result<mlua::Value> {
                    let from_pos = Position {
                        x: from.get("x")?,
                        y: from.get("y")?,
                    };
                    let to_pos = Position {
                        x: to.get("x")?,
                        y: to.get("y")?,
                    };
                    let avoid_swamp = if let Some(ref o) = opts {
                        match o.get::<String>("avoid") {
                            Ok(s) => s == "swamp",
                            Err(_) => false,
                        }
                    } else {
                        false
                    };

                    // find_path: если цель непроходима, автоматически перенаправляет
                    // на ближайшую проходимую клетку. Если цели нельзя достичь
                    // для взаимодействия (нет walkable клетки на dist 1), возвращает nil.
                    let effective_to = if pathfinding::is_pos_walkable(&tiles_fp, to_pos, w, h, &bl) {
                        to_pos
                    } else {
                        match pathfinding::find_adjacent_walkable(&tiles_fp, to_pos, w, h, &bl) {
                            Some(p) => p,
                            None => {
                                // Цель полностью окружена — недоступна
                                return Ok(mlua::Value::Nil);
                            }
                        }
                    };

                    match pathfinding::astar(&tiles_fp, w, h, &bl, from_pos, effective_to, avoid_swamp) {
                        Some(positions) => {
                            let table = lua.create_table()?;
                            for (i, pos) in positions.iter().enumerate() {
                                let p = lua.create_table()?;
                                p.set("x", pos.x)?;
                                p.set("y", pos.y)?;
                                table.set(i + 1, p)?;
                            }
                            Ok(mlua::Value::Table(table))
                        }
                        None => Ok(mlua::Value::Nil),
                    }
                },
            )?;
            lua.globals().set("find_path", find_path_fn)?;

            // ── get_tile(x, y) ────────────────────────────
            let tiles_gt = tiles;
            let w = width;
            let h = height;

            let get_tile_fn = lua.create_function(
                move |lua: &mlua::Lua, (x, y): (i32, i32)| -> mlua::Result<mlua::Value> {
                    if x < 0 || y < 0 || x >= w || y >= h {
                        return Ok(mlua::Value::Nil);
                    }
                    let s = match tiles_gt[y as usize][x as usize] {
                        TileType::Plain => "plain",
                        TileType::Wall => "wall",
                        TileType::Swamp => "swamp",
                    };
                    Ok(mlua::Value::String(lua.create_string(s)?))
                },
            )?;
            lua.globals().set("get_tile", get_tile_fn)?;

            tracing::info!("registered Lua functions: find_path, get_tile");
            Ok(())
        })
    }

    pub fn find_by_type(
        &self,
        entity_type: EntityType,
        pos: Position,
        range: i32,
    ) -> Vec<&Entity> {
        self.entities
            .iter()
            .filter(|e| {
                e.entity_type == entity_type
                    && (e.pos.x - pos.x).abs() + (e.pos.y - pos.y).abs() <= range
            })
            .collect()
    }

    pub fn get_entity(&self, id: &str) -> Option<&Entity> {
        self.entity_index.get(id).map(|&idx| &self.entities[idx])
    }

    pub fn get_entity_mut(&mut self, id: &str) -> Option<&mut Entity> {
        if let Some(&idx) = self.entity_index.get(id) {
            self.entities.get_mut(idx)
        } else {
            None
        }
    }

    pub fn is_walkable(&self, pos: Position) -> bool {
        if pos.x < 0
            || pos.y < 0
            || pos.x >= self.width as i32
            || pos.y >= self.height as i32
        {
            return false;
        }
        if self.tiles[pos.y as usize][pos.x as usize] == TileType::Wall {
            return false;
        }
        for e in &self.entities {
            if e.pos.x == pos.x && e.pos.y == pos.y {
                if e.entity_type == EntityType::Source || e.entity_type == EntityType::Spawn {
                    return false;
                }
            }
        }
        true
    }

    pub fn step_toward(&self, from: Position, to: Position) -> Option<Position> {
        if from == to {
            return None;
        }
        let dx = (to.x - from.x).signum();
        let dy = (to.y - from.y).signum();
        let candidates = if dx.abs() >= dy.abs() {
            vec![
                Position {
                    x: from.x + dx,
                    y: from.y,
                },
                Position {
                    x: from.x,
                    y: from.y + dy,
                },
            ]
        } else {
            vec![
                Position {
                    x: from.x,
                    y: from.y + dy,
                },
                Position {
                    x: from.x + dx,
                    y: from.y,
                },
            ]
        };
        candidates.into_iter().find(|p| self.is_walkable(*p))
    }

    pub fn distance(a: Position, b: Position) -> i32 {
        (a.x - b.x).abs() + (a.y - b.y).abs()
    }

    /// Собирает GameContext для передачи в before_tick().
    pub fn build_game_context(&self) -> GameContext {
        let creeps: Vec<NearbyEntity> = self
            .entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Creep)
            .map(|e| NearbyEntity {
                id: e.id.clone(),
                pos: e.pos,
                resource_amount: e.carry,
                cooldown: 0,
            })
            .collect();
        let spawns: Vec<NearbyEntity> = self
            .entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Spawn)
            .map(|e| NearbyEntity {
                id: e.id.clone(),
                pos: e.pos,
                resource_amount: e.energy,
                cooldown: e.spawn_cooldown,
            })
            .collect();
        let sources: Vec<NearbyEntity> = self
            .entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Source)
            .map(|e| NearbyEntity {
                id: e.id.clone(),
                pos: e.pos,
                resource_amount: e.resource_amount,
                cooldown: 0,
            })
            .collect();
        GameContext { tick: self.tick, creeps, spawns, sources }
    }

    /// Обрабатывает экшен Spawn. Вынесен отдельно, чтобы вызывать
    /// как из apply_action() (от крипа), так и из tick() (от before_tick).
    /// Возвращает true если крип успешно создан.
    fn process_spawn(&mut self, target_id: &str, body: &[String], name: &str) -> bool {
        let spawn = match self.get_entity(target_id).cloned() {
            Some(s) if s.entity_type == EntityType::Spawn => s,
            Some(_) => {
                tracing::warn!(target_id = %target_id, "spawn failed: target is not a Spawn");
                return false;
            }
            None => {
                tracing::warn!(target_id = %target_id, "spawn failed: spawn not found");
                return false;
            }
        };

        if spawn.spawn_cooldown > 0 {
            tracing::debug!(
                target_id = %target_id,
                cooldown = spawn.spawn_cooldown,
                "spawn failed: cooldown"
            );
            return false;
        }

        let mut parts = Vec::new();
        for part_str in body {
            match parse_body_part(part_str) {
                Some(p) => parts.push(p),
                None => {
                    tracing::warn!(unknown_part = %part_str, "spawn failed: unknown body part");
                    return false;
                }
            }
        }
        if parts.is_empty() {
            tracing::warn!("spawn failed: empty body");
            return false;
        }

        let cost = body_cost(&parts);
        if cost > spawn.energy {
            tracing::debug!(
                target_id = %target_id,
                cost,
                available = spawn.energy,
                "spawn failed: not enough energy"
            );
            return false;
        }

        // Ищем свободную клетку рядом со спавном
        let directions: [(i32, i32); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];
        let spawn_pos = loop {
            let mut found = None;
            for &(dx, dy) in &directions {
                let next = Position { x: spawn.pos.x + dx, y: spawn.pos.y + dy };
                if self.is_walkable(next) {
                    found = Some(next);
                    break;
                }
            }
            match found {
                Some(p) => break p,
                None => {
                    tracing::warn!(target_id = %target_id, "spawn failed: no adjacent walkable cell");
                    return false;
                }
            }
        };

        // Генерируем уникальный ID
        let base_name = if name.is_empty() {
            format!("worker_{}", self.entities.iter().filter(|e| e.entity_type == EntityType::Creep).count() + 1)
        } else {
            name.to_string()
        };
        let creep_name = if self.get_entity(&base_name).is_none() {
            base_name
        } else {
            let mut suffix: u32 = 2;
            loop {
                let candidate = format!("{}_{}", base_name, suffix);
                if self.get_entity(&candidate).is_none() {
                    break candidate;
                }
                suffix += 1;
            }
        };

        let new_creep = Entity::new_creep(&creep_name, spawn_pos, parts);
        tracing::info!(
            target_id = %target_id,
            new_id = %creep_name,
            cost,
            pos.x = spawn_pos.x,
            pos.y = spawn_pos.y,
            body = ?body,
            "spawned creep"
        );

        // Тратим энергию и ставим кулдаун
        if let Some(s) = self.get_entity_mut(target_id) {
            s.energy -= cost;
            s.spawn_cooldown = SPAWN_COOLDOWN_TICKS;
        }
        self.entities.push(new_creep);
        self.entity_index.insert(creep_name, self.entities.len() - 1);
        true
    }

    pub fn tick(&mut self, engine: &ScriptEngine) {
        // Уменьшаем кулдауны спавнов на 1
        for entity in &mut self.entities {
            if entity.entity_type == EntityType::Spawn && entity.spawn_cooldown > 0 {
                entity.spawn_cooldown -= 1;
            }
        }

        // Регенерация источников
        for entity in &mut self.entities {
            if entity.entity_type == EntityType::Source {
                entity.resource_amount = (entity.resource_amount as u32 + self.source_regen_rate)
                    .min(self.max_source_amount);
            }
        }

        // ── before_tick(game) — глобальный хук, аналог Screeps loop() ──
        // Вызывается ОДИН раз в начале тика, ДО обработки крипов.
        // Позволяет Lua-коду спавнить крипов даже когда их нет на карте.
        let game_ctx = self.build_game_context();
        if let Ok(Some(action)) = engine.call_before_tick(&game_ctx) {
            match &action {
                Action::Spawn { target_id, body, name } => {
                    self.process_spawn(target_id, body, name);
                }
                other => {
                    tracing::warn!(action = ?other, "before_tick returned non-spawn action, ignored");
                }
            }
        }

        // ── Per-creep decide() ──
        let creep_ids: Vec<String> = self
            .entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Creep)
            .map(|e| e.id.clone())
            .collect();

        // Phase 1: Collect all creep snapshots and build contexts BEFORE any mutations
        let creep_snapshots: Vec<(String, Entity)> = creep_ids
            .iter()
            .filter_map(|id| self.get_entity(id).cloned().map(|c| (id.clone(), c)))
            .collect();

        let mut actions: Vec<(String, Action)> = Vec::new();
        for (creep_id, creep) in &creep_snapshots {
            let span = tracing::info_span!("creep", id = %creep_id, tick = self.tick);
            let _enter = span.enter();

            let ctx = self.build_unit_context(creep);
            let action = engine.call_decide(&ctx).unwrap_or_else(|err| {
                tracing::error!(error = %err, "Lua error during decide()");
                Action::Idle {
                    reason: format!("script error: {}", err),
                }
            });
            actions.push((creep_id.clone(), action));
        }

        // Phase 2: Apply all actions
        for (creep_id, action) in &actions {
            self.apply_action(creep_id, action);
        }

        self.tick += 1;
    }

    pub fn build_unit_context(&self, creep: &Entity) -> UnitContext {
        let sources: Vec<NearbyEntity> = self
            .find_by_type(EntityType::Source, creep.pos, self.view_range)
            .into_iter()
            .map(|e| NearbyEntity {
                id: e.id.clone(),
                pos: e.pos,
                resource_amount: e.resource_amount,
                cooldown: 0,
            })
            .collect();
        let spawns: Vec<NearbyEntity> = self
            .find_by_type(EntityType::Spawn, creep.pos, self.view_range)
            .into_iter()
            .map(|e| NearbyEntity {
                id: e.id.clone(),
                pos: e.pos,
                resource_amount: e.energy,
                cooldown: e.spawn_cooldown,
            })
            .collect();
        let creeps: Vec<NearbyEntity> = self
            .find_by_type(EntityType::Creep, creep.pos, self.view_range)
            .into_iter()
            .filter(|e| e.id != creep.id)
            .map(|e| NearbyEntity {
                id: e.id.clone(),
                pos: e.pos,
                resource_amount: e.carry,
                cooldown: 0,
            })
            .collect();

        UnitContext {
            id: creep.id.clone(),
            pos: creep.pos,
            hp: creep.hp,
            max_hp: creep.max_hp,
            energy: creep.energy,
            carry_capacity: creep.carry_capacity,
            carry: creep.carry,
            tick: self.tick,
            nearby_sources: sources,
            nearby_spawns: spawns,
            nearby_creeps: creeps,
        }
    }

    pub fn apply_action(&mut self, creep_id: &str, action: &Action) {
        // Записываем last_action на сущность (для проверки столкновений)
        if let Some(c) = self.get_entity_mut(creep_id) {
            c.last_action = Some(action.clone());
        }

        // Очищаем запланированный путь для всех действий кроме MoveTo
        if !matches!(action, Action::MoveTo { .. }) {
            if let Some(c) = self.get_entity_mut(creep_id) {
                c.planned_path.clear();
            }
        }

        match action {
            Action::Move { target, reason } => {
                let creep = self.get_entity(creep_id).cloned();
                if let Some(creep) = creep {
                    if creep.move_speed() == 0 {
                        tracing::warn!("cannot move: speed is 0");
                        return;
                    }

                    // Жадное движение: до move_speed() шагов к цели
                    let mut move_points = creep.move_speed();
                    let mut pos = creep.pos;
                    let mut steps = 0;

                    while move_points > 0 {
                        if pos == *target {
                            break;
                        }
                        if let Some(next_pos) = self.step_toward(pos, *target) {
                            let next_tile = self.tiles[next_pos.y as usize][next_pos.x as usize];
                            let step_cost = pathfinding::tile_move_cost(next_tile);

                            if move_points < step_cost {
                                break;
                            }

                            pos = next_pos;
                            move_points -= step_cost;
                            steps += 1;
                        } else {
                            break;
                        }
                    }

                    if pos != creep.pos {
                        tracing::info!(
                            from.x = creep.pos.x, from.y = creep.pos.y,
                            to.x = pos.x, to.y = pos.y,
                            steps,
                            %reason, "moved"
                        );
                        if let Some(c) = self.get_entity_mut(creep_id) {
                            c.pos = pos;
                        }
                    } else {
                        tracing::warn!(
                            target.x = target.x, target.y = target.y,
                            "path blocked or already at target"
                        );
                    }
                }
            }

            Action::MoveTo { target, reason } => {
                let creep = self.get_entity(creep_id).cloned();
                if let Some(creep) = creep {
                    if creep.move_speed() == 0 {
                        tracing::warn!("cannot move: speed is 0");
                        return;
                    }

                    // Если цель непроходима (стена, источник, спавн), BFS-ом ищем
                    // ближайшую проходимую клетку. Радиус 10 покрывает любую разумную карту.
                    let effective_target = self
                        .find_nearest_walkable(*target, 10)
                        .unwrap_or(*target);

                    if !self.is_walkable(effective_target) {
                        // Даже BFS ничего не нашёл — цель полностью окружена
                        if creep.planned_path.is_empty() {
                            tracing::warn!(
                                target.x = target.x, target.y = target.y,
                                "target completely surrounded, no walkable cell nearby"
                            );
                        }
                        return;
                    }

                    // Пересчитываем путь, если он пустой или ведёт к другой цели
                    let needs_recompute = creep.planned_path.is_empty()
                        || creep.planned_path.last().copied() != Some(effective_target);

                    let path = if needs_recompute {
                        let blockers = self.block_positions();
                        match pathfinding::astar(
                            &self.tiles,
                            self.width as i32,
                            self.height as i32,
                            &blockers,
                            creep.pos,
                            effective_target,
                            false,
                        ) {
                            Some(p) => {
                                if effective_target != *target {
                                    tracing::info!(
                                        target.x = target.x, target.y = target.y,
                                        effective.x = effective_target.x, effective.y = effective_target.y,
                                        "target non-walkable, rerouted to adjacent cell"
                                    );
                                }
                                tracing::debug!(
                                    from.x = creep.pos.x, from.y = creep.pos.y,
                                    target.x = target.x, target.y = target.y,
                                    effective_target.x = effective_target.x, effective_target.y = effective_target.y,
                                    path_len = p.len(), "path computed"
                                );
                                p
                            }
                            None => {
                                // Логируем WARN только если путь был пуст (первая попытка),
                                // чтобы не спамить каждый тик при постоянной неудаче.
                                if creep.planned_path.is_empty() {
                                    tracing::warn!(
                                        target.x = target.x, target.y = target.y,
                                        effective_target.x = effective_target.x, effective_target.y = effective_target.y,
                                        "no path found to target"
                                    );
                                }
                                return;
                            }
                        }
                    } else {
                        creep.planned_path.clone()
                    };

                    // path[0] — текущая позиция, пропускаем
                    // Идём до move_speed() шагов, учитывая стоимость болота
                    let mut move_points = creep.move_speed();
                    let mut final_pos = creep.pos;
                    let mut steps_taken = 0;

                    for i in 1..path.len() {
                        if move_points == 0 {
                            break;
                        }

                        let next_tile = self.tiles[path[i].y as usize][path[i].x as usize];
                        let step_cost = pathfinding::tile_move_cost(next_tile);

                        if move_points < step_cost {
                            break;
                        }

                        if self.is_walkable(path[i]) {
                            final_pos = path[i];
                            steps_taken = i;
                            move_points -= step_cost;
                        } else {
                            tracing::debug!(
                                x = path[i].x, y = path[i].y,
                                "path blocked, will recompute next tick"
                            );
                            break;
                        }
                    }

                    // Сохраняем оставшийся путь с текущей позицией в индексе 0.
                    // Это критично: цикл `for i in 1..` всегда ожидает,
                    // что path[0] = текущая позиция крипа.
                    let mut cached_path = vec![final_pos];
                    cached_path.extend(path.into_iter().skip(steps_taken + 1));

                    if final_pos != creep.pos {
                        tracing::info!(
                            from.x = creep.pos.x, from.y = creep.pos.y,
                            to.x = final_pos.x, to.y = final_pos.y,
                            steps = steps_taken,
                            path_remaining = cached_path.len() - 1,
                            %reason, "path move"
                        );
                    }

                    if let Some(c) = self.get_entity_mut(creep_id) {
                        c.pos = final_pos;
                        c.planned_path = cached_path;
                    }

                    if final_pos == creep.pos && steps_taken == 0 {
                        // Не смогли сдвинуться — очищаем путь для пересчёта
                        if let Some(c) = self.get_entity_mut(creep_id) {
                            c.planned_path.clear();
                        }
                    }
                }
            }

            Action::Harvest { target_id } => {
                let creep = self.get_entity(creep_id).cloned();
                let source = self.get_entity(target_id).cloned();
                if let (Some(creep), Some(source)) = (creep, source) {
                    if source.entity_type != EntityType::Source {
                        tracing::warn!(target_id = %target_id, "harvest failed: target is not a Source");
                        return;
                    }
                    if Self::distance(creep.pos, source.pos) > 1 {
                        tracing::warn!(target_id = %target_id, "harvest failed: not adjacent to source");
                        return;
                    }
                    if !creep.can_work() {
                        tracing::warn!("harvest failed: no Work body part");
                        return;
                    }
                    if !creep.has_capacity() {
                        tracing::warn!(
                            carry = creep.carry,
                            capacity = creep.carry_capacity,
                            "harvest failed: carry full"
                        );
                        return;
                    }
                    let amount = self
                        .harvest_rate
                        .min(source.resource_amount)
                        .min(creep.carry_capacity - creep.carry);
                    if amount > 0 {
                        tracing::info!(
                            target_id = %target_id,
                            amount,
                            source_remaining = source.resource_amount - amount,
                            "harvested"
                        );
                        if let Some(s) = self.get_entity_mut(target_id) {
                            s.resource_amount -= amount;
                        }
                        if let Some(c) = self.get_entity_mut(creep_id) {
                            c.carry += amount;
                        }
                    } else {
                        tracing::warn!(target_id = %target_id, "harvest skipped: source depleted");
                    }
                }
            }

            Action::Transfer {
                target_id,
                resource,
                amount,
            } => {
                let creep = self.get_entity(creep_id).cloned();
                if let Some(creep) = creep {
                    let transfer = (*amount).min(creep.carry);
                    if transfer == 0 {
                        tracing::info!("transfer skipped: nothing to carry");
                        return;
                    }
                    let target_pos = self.get_entity(target_id).map(|t| t.pos);
                    if let Some(tp) = target_pos {
                        if Self::distance(creep.pos, tp) > 1 {
                            tracing::warn!(
                                target_id = %target_id,
                                distance = Self::distance(creep.pos, tp),
                                "transfer failed: too far from target"
                            );
                            return;
                        }
                    } else {
                        tracing::warn!(target_id = %target_id, "transfer failed: target not found");
                        return;
                    }
                    tracing::info!(
                        target_id = %target_id,
                        resource = %resource,
                        amount = transfer,
                        "transferred"
                    );
                    if let Some(c) = self.get_entity_mut(creep_id) {
                        c.carry -= transfer;
                    }
                    if let Some(t) = self.get_entity_mut(target_id) {
                        t.energy += transfer;
                    }
                }
            }

            Action::Spawn {
                target_id,
                body,
                name,
            } => {
                self.process_spawn(target_id, body, name);
            }

            Action::Idle { reason } => {
                tracing::info!(reason = %reason, "idle");
            }
        }
    }
}

// ═══════════════════════════════════════
//  Tests
// ═══════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::pathfinding;

    #[test]
    fn test_move_speed() {
        let fast = Entity::new_creep(
            "scout",
            Position { x: 0, y: 0 },
            vec![BodyPart::Move, BodyPart::Move, BodyPart::Move, BodyPart::Move],
        );
        assert_eq!(fast.move_speed(), 4);

        let worker = Entity::new_creep(
            "worker",
            Position { x: 0, y: 0 },
            vec![BodyPart::Move, BodyPart::Move, BodyPart::Work, BodyPart::Carry],
        );
        assert_eq!(worker.move_speed(), 2);

        let turret = Entity::new_creep(
            "turret",
            Position { x: 0, y: 0 },
            vec![BodyPart::Work, BodyPart::Attack],
        );
        assert_eq!(turret.move_speed(), 0);
        assert!(!turret.can_move());
    }

    #[test]
    fn test_astar_basic() {
        let tiles = vec![
            vec![TileType::Plain, TileType::Plain, TileType::Plain],
            vec![TileType::Plain, TileType::Wall, TileType::Plain],
            vec![TileType::Plain, TileType::Plain, TileType::Plain],
        ];
        let path = pathfinding::astar(
            &tiles, 3, 3, &[],
            Position { x: 0, y: 0 },
            Position { x: 2, y: 2 },
            false,
        );
        assert!(path.is_some());
        let p = path.unwrap();
        assert_eq!(p[0], Position { x: 0, y: 0 });
        assert_eq!(*p.last().unwrap(), Position { x: 2, y: 2 });
        // Должен идти вокруг стены, а не через неё
        assert!(p.len() > 3);
        // Ни одна промежуточная клетка не должна быть стеной
        for pos in &p[1..p.len() - 1] {
            assert_ne!(tiles[pos.y as usize][pos.x as usize], TileType::Wall);
        }
    }

    #[test]
    fn test_astar_swamp_allowed() {
        let tiles = vec![
            vec![TileType::Plain, TileType::Swamp, TileType::Plain],
            vec![TileType::Plain, TileType::Plain, TileType::Plain],
        ];
        // Swamp разрешён: кратчайший путь через него
        let path = pathfinding::astar(
            &tiles, 3, 2, &[],
            Position { x: 0, y: 0 },
            Position { x: 2, y: 0 },
            false,
        );
        assert!(path.is_some());
        let p = path.unwrap();
        assert_eq!(p.len(), 3); // прямой путь через swamp
    }

    #[test]
    fn test_astar_avoid_swamp() {
        let tiles = vec![
            vec![TileType::Plain, TileType::Swamp, TileType::Plain],
            vec![TileType::Plain, TileType::Plain, TileType::Plain],
        ];
        // Swamp запрещён: путь в обход
        let path = pathfinding::astar(
            &tiles, 3, 2, &[],
            Position { x: 0, y: 0 },
            Position { x: 2, y: 0 },
            true,
        );
        assert!(path.is_some());
        let p = path.unwrap();
        assert!(p.len() > 3); // более длинный путь
        assert_eq!(*p.last().unwrap(), Position { x: 2, y: 0 });
        // Ни одна клетка не должна быть swamp
        for pos in &p {
            assert_ne!(tiles[pos.y as usize][pos.x as usize], TileType::Swamp);
        }
    }

    #[test]
    fn test_astar_no_path() {
        let tiles = vec![
            vec![TileType::Plain, TileType::Wall, TileType::Plain],
            vec![TileType::Wall, TileType::Wall, TileType::Wall],
            vec![TileType::Plain, TileType::Wall, TileType::Plain],
        ];
        let path = pathfinding::astar(
            &tiles, 3, 3, &[],
            Position { x: 0, y: 0 },
            Position { x: 2, y: 2 },
            false,
        );
        assert!(path.is_none());
    }

    #[test]
    fn test_astar_same_position() {
        let tiles = vec![vec![TileType::Plain]];
        let path = pathfinding::astar(
            &tiles, 1, 1, &[],
            Position { x: 0, y: 0 },
            Position { x: 0, y: 0 },
            false,
        );
        assert_eq!(path, Some(vec![Position { x: 0, y: 0 }]));
    }

    #[test]
    fn test_astar_blockers() {
        let tiles = vec![
            vec![TileType::Plain, TileType::Plain, TileType::Plain],
            vec![TileType::Plain, TileType::Plain, TileType::Plain],
        ];
        let blocker = Position { x: 1, y: 0 };
        // Цель — на блокере, путь должен существовать (целевая клетка разрешена)
        let path = pathfinding::astar(
            &tiles, 3, 2, &[blocker],
            Position { x: 0, y: 0 },
            Position { x: 1, y: 0 },
            false,
        );
        assert!(path.is_some());

        // Пройти через блокер нельзя
        let path = pathfinding::astar(
            &tiles, 3, 2, &[blocker],
            Position { x: 0, y: 0 },
            Position { x: 2, y: 0 },
            false,
        );
        assert!(path.is_some());
        let p = path.unwrap();
        // Должен идти в обход (row 1)
        for pos in &p[1..p.len() - 1] {
            assert_ne!(*pos, blocker);
        }
    }

    #[test]
    fn test_body_part_cost() {
        assert_eq!(body_part_cost(&BodyPart::Move), 50);
        assert_eq!(body_part_cost(&BodyPart::Work), 100);
        assert_eq!(body_part_cost(&BodyPart::Carry), 50);
        assert_eq!(body_part_cost(&BodyPart::Attack), 80);
        assert_eq!(body_part_cost(&BodyPart::Tough), 10);
    }

    #[test]
    fn test_body_cost_sum() {
        let parts = vec![BodyPart::Move, BodyPart::Move, BodyPart::Work, BodyPart::Carry];
        assert_eq!(body_cost(&parts), 250); // 50+50+100+50
    }

    #[test]
    fn test_parse_body_part() {
        assert_eq!(parse_body_part("move"), Some(BodyPart::Move));
        assert_eq!(parse_body_part("Move"), Some(BodyPart::Move));
        assert_eq!(parse_body_part("MOVE"), Some(BodyPart::Move));
        assert_eq!(parse_body_part("work"), Some(BodyPart::Work));
        assert_eq!(parse_body_part("carry"), Some(BodyPart::Carry));
        assert_eq!(parse_body_part("attack"), Some(BodyPart::Attack));
        assert_eq!(parse_body_part("tough"), Some(BodyPart::Tough));
        assert_eq!(parse_body_part("fly"), None);
        assert_eq!(parse_body_part(""), None);
    }

    #[test]
    fn test_spawn_creep_basic() {
        let mut world = GameState::from_map(&[
            "#####",
            "#S.c#",
            "#####",
        ]);
        let spawn_id = "spawn1".to_string();
        let creep_id = "worker_1".to_string();

        // Спавн: 300 энергии, кулдаун = 0
        let cost = body_cost(&[BodyPart::Move, BodyPart::Move, BodyPart::Work, BodyPart::Carry]); // 250
        assert!(world.get_entity(&spawn_id).unwrap().energy >= cost);

        world.apply_action(
            &creep_id,
            &Action::Spawn {
                target_id: spawn_id.clone(),
                body: vec!["move".into(), "move".into(), "work".into(), "carry".into()],
                name: "worker_2".into(),
            },
        );

        // Новый крип создан
        assert!(world.get_entity("worker_2").is_some());
        assert_eq!(world.entities.iter().filter(|e| e.entity_type == EntityType::Creep).count(), 2);

        // Энергия потрачена
        let spawn = world.get_entity(&spawn_id).unwrap();
        assert_eq!(spawn.energy, 300 - cost);

        // Кулдаун установлен
        assert_eq!(spawn.spawn_cooldown, SPAWN_COOLDOWN_TICKS);
    }

    #[test]
    fn test_spawn_not_enough_energy() {
        let mut world = GameState::from_map(&[
            "#####",
            "#S.c#",
            "#####",
        ]);
        // Спавн пытается спавнить дорогого крипа
        world.apply_action(
            "worker_1",
            &Action::Spawn {
                target_id: "spawn1".into(),
                body: vec!["move".into(), "move".into(), "work".into(), "carry".into(),
                         "carry".into(), "carry".into()], // 400 энергии — больше чем есть
                name: "expensive".into(),
            },
        );
        // Крип НЕ создан — не хватает энергии
        assert_eq!(world.entities.iter().filter(|e| e.entity_type == EntityType::Creep).count(), 1);
    }

    #[test]
    fn test_spawn_cooldown() {
        let mut world = GameState::from_map(&[
            "#####",
            "#S.c#",
            "#####",
        ]);

        // Первый спавн — успешно
        world.apply_action(
            "worker_1",
            &Action::Spawn {
                target_id: "spawn1".into(),
                body: vec!["move".into(), "work".into(), "carry".into()],
                name: "w2".into(),
            },
        );
        assert_eq!(world.entities.len(), 3); // spawn + 2 creep

        // Второй спавн сразу — кулдаун
        world.apply_action(
            "worker_1",
            &Action::Spawn {
                target_id: "spawn1".into(),
                body: vec!["move".into(), "work".into(), "carry".into()],
                name: "w3".into(),
            },
        );
        assert_eq!(world.entities.len(), 3); // не создался
    }

    /// Интеграционный тест: Memory персистентна через GameState.tick()
    /// Проверяем полный цикл: GameState → ScriptEngine → Lua decide() → Memory → Rust
    #[test]
    fn test_memory_persists_through_world_tick() {
        let engine = ScriptEngine::new().unwrap();

        // Скрипт записывает в Memory.creeps[id] позицию при каждом вызове
        engine
            .load_script_from_str(
                r#"
            function decide(ctx)
                if not Memory.total_ticks then Memory.total_ticks = 0 end
                Memory.total_ticks = Memory.total_ticks + 1
                Memory.last_creep_id = ctx.id
                return { type = "idle", reason = "tick" }
            end
        "#,
            )
            .unwrap();

        let mut world = GameState::from_map(&[
            ".....",
            ".Sc..",
            "..E..",
            ".....",
            ".....",
        ]);
        world.view_range = 50;
        world
            .register_lua_functions(&engine)
            .unwrap();

        // 3 тика — 1 крип, Memory.total_ticks должен быть 3
        for _ in 0..3 {
            world.tick(&engine);
        }
        assert_eq!(world.tick, 3);
        assert_eq!(
            engine.get_memory_number("total_ticks").unwrap(),
            Some(3.0)
        );
        assert_eq!(
            engine.get_memory_string("last_creep_id").unwrap(),
            Some("worker_1".to_string())
        );

        // Добавляем второго крипа
        world.entities.push(Entity::new_creep(
            "worker_2",
            Position { x: 1, y: 2 },
            vec![BodyPart::Move, BodyPart::Work, BodyPart::Carry],
        ));
        world.entity_index.insert("worker_2".to_string(), world.entities.len() - 1);

        // Ещё 2 тика — 2 крипа, каждый вызывает decide(), total_ticks = 3 + 2*2 = 7
        for _ in 0..2 {
            world.tick(&engine);
        }
        assert_eq!(world.tick, 5);
        assert_eq!(
            engine.get_memory_number("total_ticks").unwrap(),
            Some(7.0)
        );
        // Последним был worker_2 (крипы обрабатываются по порядку)
        assert_eq!(
            engine.get_memory_string("last_creep_id").unwrap(),
            Some("worker_2".to_string())
        );
    }

    /// Интеграционный тест: harvester.lua использует Memory через GameState.tick()
    /// Проверяем, что реальный скрипт пишет в Memory.creeps
    #[test]
    fn test_harvester_lua_uses_memory() {
        let engine = ScriptEngine::new().unwrap();

        // Загружаем реальный harvester.lua
        let map = [
            "..........",
            ".S....E..",
            "....c....",
            "..........",
        ];
        let mut world = GameState::from_map(&map);
        world.view_range = 50;
        world
            .register_lua_functions(&engine)
            .unwrap();
        engine
            .load_script(std::path::Path::new("scripts/harvester.lua"))
            .unwrap();

        // Запускаем 2 тика — harvester.lua должен инициализировать Memory
        world.tick(&engine);
        world.tick(&engine);

        // Проверяем, что Memory.creeps существует
        let creeps = engine
            .with_lua(|lua| -> mlua::Result<bool> {
                let memory: mlua::Table = lua.globals().get("Memory")?;
                let creeps: mlua::Value = memory.get("creeps")?;
                Ok(matches!(creeps, mlua::Value::Table(_)))
            })
            .unwrap();
        assert!(creeps, "Memory.creeps should be initialized by harvester.lua");

        // Проверяем, что Memory.spawn_count существует
        let count = engine.get_memory_number("spawn_count");
        assert!(count.is_ok(), "Memory.spawn_count should be set");
    }

    /// Тест: process_spawn генерирует уникальное имя при дубликате
    #[test]
    fn test_spawn_unique_name_on_duplicate() {
        let mut world = GameState::from_map(&[
            "#####",
            "#S.c#",
            "#####",
        ]);

        // Пытаемся заспавнить крипа с именем worker_1 (уже существует)
        world.apply_action(
            "worker_1",
            &Action::Spawn {
                target_id: "spawn1".into(),
                body: vec!["move".into()], // дешёвый крип (50 энергии)
                name: "worker_1".into(),
            },
        );

        // Крип создан, но с уникальным именем worker_1_2
        assert_eq!(world.entities.len(), 3); // spawn + worker_1 + worker_1_2
        assert!(world.get_entity("worker_1").is_some());
        assert!(world.get_entity("worker_1_2").is_some());

        // Ещё один дубликат → worker_1_3
        world.entities.iter_mut()
            .find(|e| e.entity_type == EntityType::Spawn)
            .unwrap().spawn_cooldown = 0;
        world.apply_action(
            "worker_1",
            &Action::Spawn {
                target_id: "spawn1".into(),
                body: vec!["move".into()],
                name: "worker_1".into(),
            },
        );
        assert!(world.get_entity("worker_1_3").is_some());
    }

    /// Тест: before_tick() вызывается и может спавнить крипов
    #[test]
    fn test_before_tick_spawns_creep() {
        let engine = ScriptEngine::new().unwrap();

        engine
            .load_script_from_str(
                r#"
            function before_tick(game)
                if #game.creeps < 2 then
                    for _, sp in ipairs(game.spawns) do
                        if sp.cooldown == 0 then
                            return {
                                type = "spawn",
                                target_id = sp.id,
                                body = {"move", "work", "carry"},
                                name = "auto_" .. (#game.creeps + 1)
                            }
                        end
                    end
                end
                return nil
            end

            function decide(ctx)
                return { type = "idle", reason = "tick" }
            end
        "#,
            )
            .unwrap();

        let mut world = GameState::from_map(&[
            ".....",
            ".Sc..",
            "..E..",
            ".....",
            ".....",
        ]);
        world.view_range = 50;
        world
            .register_lua_functions(&engine)
            .unwrap();

        // 1 крип на карте. before_tick должен заспавнить второго.
        assert_eq!(
            world.entities.iter().filter(|e| e.entity_type == EntityType::Creep).count(),
            1
        );

        world.tick(&engine);

        // Теперь 2 крипа
        assert_eq!(
            world.entities.iter().filter(|e| e.entity_type == EntityType::Creep).count(),
            2
        );
        assert!(world.get_entity("auto_2").is_some());

        // Ещё один тик — уже 2 крипа, спавн не должен сработать (лимит 2)
        world.tick(&engine);
        assert_eq!(
            world.entities.iter().filter(|e| e.entity_type == EntityType::Creep).count(),
            2
        );
    }

    /// Тест: мир без крипов — before_tick() спавнит первого
    #[test]
    fn test_before_tick_bootstrap_no_creeps() {
        let engine = ScriptEngine::new().unwrap();

        engine
            .load_script_from_str(
                r#"
            function before_tick(game)
                if #game.creeps == 0 then
                    for _, sp in ipairs(game.spawns) do
                        if sp.cooldown == 0 then
                            return {
                                type = "spawn",
                                target_id = sp.id,
                                body = {"move", "move", "work", "carry"},
                                name = "first_creep"
                            }
                        end
                    end
                end
                return nil
            end

            function decide(ctx)
                return { type = "idle", reason = "alive" }
            end
        "#,
            )
            .unwrap();

        // Карта БЕЗ крипов — только спавн и источник
        let mut world = GameState::from_map(&[
            ".....",
            ".SE..",
            ".....",
            ".....",
            ".....",
        ]);
        world.view_range = 50;
        world
            .register_lua_functions(&engine)
            .unwrap();

        assert_eq!(
            world.entities.iter().filter(|e| e.entity_type == EntityType::Creep).count(),
            0
        );

        // before_tick спавнит первого крипа
        world.tick(&engine);
        assert_eq!(
            world.entities.iter().filter(|e| e.entity_type == EntityType::Creep).count(),
            1
        );
        assert!(world.get_entity("first_creep").is_some());

        // Второй тик — decide() вызвался для первого крипа
        world.tick(&engine);
        assert_eq!(
            world.entities.iter().filter(|e| e.entity_type == EntityType::Creep).count(),
            1
        );
        let creep = world.get_entity("first_creep").unwrap();
        assert!(creep.last_action.is_some());
    }
}
