use crate::lua_api::{Action, NearbyEntity, Position, ScriptEngine, UnitContext};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

// ═══════════════════════════════════════
//  Типы тайлов и сущностей
// ═══════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TileType {
    Plain,
    Wall,
    Swamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EntityType {
    Creep,
    Source,
    Spawn,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BodyPart {
    Move,
    Work,
    Carry,
    Attack,
    Tough,
}

/// Стоимость движения на болоте в очках скорости.
/// Plain = 1 очко, Swamp = SWAMP_COST очков.
pub const SWAMP_COST: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub entity_type: EntityType,
    pub pos: Position,
    pub hp: u32,
    pub max_hp: u32,
    pub energy: u32,
    pub carry: u32,
    pub carry_capacity: u32,
    pub body: Vec<BodyPart>,
    pub resource_amount: u32,
    /// Запланированный маршрут (используется MoveTo).
    /// Путь включает все позиции от старта до цели.
    #[serde(default)]
    pub planned_path: Vec<Position>,
}

impl Entity {
    pub fn new_source(id: &str, pos: Position, amount: u32) -> Self {
        Entity {
            id: id.to_string(),
            entity_type: EntityType::Source,
            pos,
            hp: 0,
            max_hp: 0,
            energy: 0,
            carry: 0,
            carry_capacity: 0,
            body: vec![],
            resource_amount: amount,
            planned_path: vec![],
        }
    }

    pub fn new_spawn(id: &str, pos: Position, initial_energy: u32) -> Self {
        Entity {
            id: id.to_string(),
            entity_type: EntityType::Spawn,
            pos,
            hp: 5000,
            max_hp: 5000,
            energy: initial_energy,
            carry: 0,
            carry_capacity: 1000,
            body: vec![],
            resource_amount: 0,
            planned_path: vec![],
        }
    }

    pub fn new_creep(id: &str, pos: Position, body: Vec<BodyPart>) -> Self {
        let mut hp = 100u32;
        let mut carry_capacity = 0u32;
        for part in &body {
            match part {
                BodyPart::Tough => hp += 100,
                BodyPart::Carry => carry_capacity += 50,
                _ => {}
            }
        }
        Entity {
            id: id.to_string(),
            entity_type: EntityType::Creep,
            pos,
            hp,
            max_hp: hp,
            energy: 0,
            carry: 0,
            carry_capacity,
            body,
            resource_amount: 0,
            planned_path: vec![],
        }
    }

    /// Скорость движения: сколько клеток за тик.
    /// Сейчас: количество частей Move.
    /// Будущее: система веса, buff'ы/дебаффы — менять только здесь.
    pub fn move_speed(&self) -> u32 {
        self.body.iter().filter(|p| **p == BodyPart::Move).count() as u32
    }

    pub fn can_move(&self) -> bool {
        self.move_speed() > 0
    }

    pub fn can_work(&self) -> bool {
        self.body.contains(&BodyPart::Work)
    }

    pub fn has_capacity(&self) -> bool {
        self.carry < self.carry_capacity
    }
}

// ═══════════════════════════════════════
//  Pathfinding — A*
// ═══════════════════════════════════════

fn in_bounds(pos: Position, width: i32, height: i32) -> bool {
    pos.x >= 0 && pos.y >= 0 && pos.x < width && pos.y < height
}

fn tile_move_cost(tile: TileType) -> u32 {
    match tile {
        TileType::Plain => 1,
        TileType::Swamp => SWAMP_COST,
        TileType::Wall => u32::MAX, // непроходимо
    }
}

/// A* поиск пути. Возвращает полный путь (включая from и to) или None.
///
/// - `avoid_swamp = true` → swamp treated as walls (no swamp in path)
/// - `avoid_swamp = false` → swamp allowed, costs SWAMP_MOVE_COST movement points
///
/// `blockers` — позиции непроходимых сущностей (sources, spawns).
pub fn astar(
    tiles: &[Vec<TileType>],
    width: i32,
    height: i32,
    blockers: &[Position],
    from: Position,
    to: Position,
    avoid_swamp: bool,
) -> Option<Vec<Position>> {
    if from == to {
        return Some(vec![from]);
    }

    if !in_bounds(from, width, height) || !in_bounds(to, width, height) {
        return None;
    }

    let is_blocked = |pos: Position| -> bool {
        if tiles[pos.y as usize][pos.x as usize] == TileType::Wall {
            return true;
        }
        if avoid_swamp && tiles[pos.y as usize][pos.x as usize] == TileType::Swamp {
            return true;
        }
        if blockers.contains(&pos) && pos != to {
            return true;
        }
        false
    };

    if is_blocked(from) || is_blocked(to) {
        return None;
    }

    #[derive(Clone, Copy, Eq, PartialEq)]
    struct Node {
        pos: Position,
        g: u32,
        f: u32,
    }

    impl Ord for Node {
        fn cmp(&self, other: &Self) -> Ordering {
            other.f.cmp(&self.f) // min-heap
        }
    }
    impl PartialOrd for Node {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    let heuristic = |a: Position, b: Position| -> u32 {
        (a.x - b.x).unsigned_abs() + (a.y - b.y).unsigned_abs()
    };

    let mut open = BinaryHeap::new();
    let mut g_score: HashMap<Position, u32> = HashMap::new();
    let mut came_from: HashMap<Position, Position> = HashMap::new();

    g_score.insert(from, 0);
    open.push(Node {
        pos: from,
        g: 0,
        f: heuristic(from, to),
    });

    let directions: [(i32, i32); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];

    while let Some(current) = open.pop() {
        if current.pos == to {
            // Восстановление пути
            let mut path = Vec::new();
            let mut p = to;
            while p != from {
                path.push(p);
                p = came_from[&p];
            }
            path.push(from);
            path.reverse();
            return Some(path);
        }

        // Пропускаем устаревшие записи
        if current.g > *g_score.get(&current.pos).unwrap_or(&u32::MAX) {
            continue;
        }

        for (dx, dy) in directions {
            let next = Position {
                x: current.pos.x + dx,
                y: current.pos.y + dy,
            };

            if !in_bounds(next, width, height) {
                continue;
            }
            if is_blocked(next) {
                continue;
            }

            let cost = tile_move_cost(tiles[next.y as usize][next.x as usize]);
            let tentative_g = current.g.saturating_add(cost);

            if tentative_g < *g_score.get(&next).unwrap_or(&u32::MAX) {
                g_score.insert(next, tentative_g);
                came_from.insert(next, current.pos);
                open.push(Node {
                    pos: next,
                    g: tentative_g,
                    f: tentative_g + heuristic(next, to),
                });
            }
        }
    }

    None
}

// ═══════════════════════════════════════
//  World — игровой мир
// ═══════════════════════════════════════

pub struct World {
    pub width: usize,
    pub height: usize,
    pub tiles: Vec<Vec<TileType>>,
    pub entities: Vec<Entity>,
    pub tick: u64,
    pub view_range: i32,
    pub harvest_rate: u32,
    pub last_action: Action,
}

impl World {
    /// Создаёт мир из строкового описания карты
    /// '#' стена  '~' болото  '.' пусто  'S' спавн  'E' источник  'c' крип
    pub fn from_map(map_strings: &[&str]) -> Self {
        let height = map_strings.len();
        let width = map_strings.iter().map(|s| s.len()).max().unwrap_or(0);
        let mut tiles = vec![vec![TileType::Plain; width]; height];
        let mut entities = Vec::new();
        let mut creep_count = 0u32;
        let mut source_count = 0u32;

        for (y, row) in map_strings.iter().enumerate() {
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
                        300,
                    )),
                    'E' => {
                        source_count += 1;
                        entities.push(Entity::new_source(
                            &format!("source_{}", source_count),
                            Position {
                                x: x as i32,
                                y: y as i32,
                            },
                            1000,
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

        let world = World {
            width,
            height,
            tiles,
            entities,
            tick: 0,
            view_range: 10,
            harvest_rate: 10,
            last_action: Action::Idle {
                reason: "world created".to_string(),
            },
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
    fn block_positions(&self) -> Vec<Position> {
        self.entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Source || e.entity_type == EntityType::Spawn)
            .map(|e| e.pos)
            .collect()
    }

    /// Регистрирует глобальные Lua-функции, зависящие от состояния мира.
    /// Вызывать один раз после создания World и ScriptEngine.
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

                    match astar(&tiles_fp, w, h, &bl, from_pos, to_pos, avoid_swamp) {
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
        self.entities.iter().find(|e| e.id == id)
    }

    pub fn get_entity_mut(&mut self, id: &str) -> Option<&mut Entity> {
        self.entities.iter_mut().find(|e| e.id == id)
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

    pub fn tick(&mut self, engine: &ScriptEngine) {
        let creep_ids: Vec<String> = self
            .entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Creep)
            .map(|e| e.id.clone())
            .collect();

        for creep_id in &creep_ids {
            let creep = match self.get_entity(creep_id).cloned() {
                Some(c) => c,
                None => continue,
            };

            let span = tracing::info_span!("creep", id = %creep_id, tick = self.tick);
            let _enter = span.enter();

            let ctx = self.build_unit_context(&creep);
            let action = engine.call_decide(&ctx).unwrap_or_else(|err| {
                tracing::error!(error = %err, "Lua error during decide()");
                Action::Idle {
                    reason: format!("script error: {}", err),
                }
            });
            self.last_action = action.clone();
            self.apply_action(creep_id, &action);
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
            })
            .collect();
        let spawns: Vec<NearbyEntity> = self
            .find_by_type(EntityType::Spawn, creep.pos, self.view_range)
            .into_iter()
            .map(|e| NearbyEntity {
                id: e.id.clone(),
                pos: e.pos,
                resource_amount: e.energy,
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

    fn apply_action(&mut self, creep_id: &str, action: &Action) {
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
                            let step_cost = tile_move_cost(next_tile);

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

                    // Если цель непроходима (стена, источник, спавн), ищем ближайшую
                    // смежную проходимую клетку — крип подходит к ней и может
                    // взаимодействовать с целью (harvest, transfer) с distance <= 1.
                    let effective_target = if self.is_walkable(*target) {
                        *target
                    } else {
                        let directions: [(i32, i32); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];
                        directions
                            .iter()
                            .map(|(dx, dy)| Position {
                                x: target.x + dx,
                                y: target.y + dy,
                            })
                            .find(|p| self.is_walkable(*p))
                            .unwrap_or(*target)
                    };

                    // Пересчитываем путь, если он пустой или ведёт к другой цели
                    let needs_recompute = creep.planned_path.is_empty()
                        || creep.planned_path.last().copied() != Some(effective_target);

                    let path = if needs_recompute {
                        let blockers = self.block_positions();
                        match astar(
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
                        let step_cost = tile_move_cost(next_tile);

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

                    let remaining: Vec<Position> = path.into_iter().skip(steps_taken + 1).collect();

                    if final_pos != creep.pos {
                        tracing::info!(
                            from.x = creep.pos.x, from.y = creep.pos.y,
                            to.x = final_pos.x, to.y = final_pos.y,
                            steps = steps_taken,
                            path_remaining = remaining.len(),
                            %reason, "path move"
                        );
                    }

                    if let Some(c) = self.get_entity_mut(creep_id) {
                        c.pos = final_pos;
                        c.planned_path = remaining;
                    }

                    if final_pos == creep.pos && steps_taken == 0 && needs_recompute {
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

            Action::Idle { reason } => {
                tracing::info!(reason = %reason, "idle");
            }
        }
    }

    pub fn render(&self) {
        print!("\x1B[2J\x1B[H");
        let sep = "─".repeat(self.width + 4);
        println!("┌{}┐", sep);
        println!("│ CREEP-SIM  Tick: {:>4} {:>20} │", self.tick, "");
        println!("├{}┤", sep);
        for y in 0..self.height {
            print!("│ ");
            for x in 0..self.width {
                let pos = Position {
                    x: x as i32,
                    y: y as i32,
                };
                let ch = self
                    .entities
                    .iter()
                    .find_map(|e| {
                        if e.pos == pos {
                            Some(match e.entity_type {
                                EntityType::Creep => {
                                    if e.carry > 0 {
                                        'C'
                                    } else {
                                        'c'
                                    }
                                }
                                EntityType::Source => {
                                    if e.resource_amount > 0 {
                                        'E'
                                    } else {
                                        'e'
                                    }
                                }
                                EntityType::Spawn => 'S',
                            })
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| match self.tiles[y][x] {
                        TileType::Plain => '.',
                        TileType::Wall => '#',
                        TileType::Swamp => '~',
                    });
                print!("{}", ch);
            }
            println!(" │");
        }
        println!("└{}┘", sep);
        println!();
        for e in &self.entities {
            match e.entity_type {
                EntityType::Creep => println!(
                    "  [CREEP]  {}  pos:({},{})  hp:{}/{}  carry:[{}/{}]",
                    e.id, e.pos.x, e.pos.y, e.hp, e.max_hp, e.carry, e.carry_capacity
                ),
                EntityType::Source => println!(
                    "  [SOURCE] {}  pos:({},{})  energy: {}",
                    e.id, e.pos.x, e.pos.y, e.resource_amount
                ),
                EntityType::Spawn => println!(
                    "  [SPAWN]  {}  pos:({},{})  stored: {}",
                    e.id, e.pos.x, e.pos.y, e.energy
                ),
            }
        }
        println!();
        print!("  ACTION: ");
        match &self.last_action {
            Action::Move { target, reason } => {
                println!("MOVE -> ({},{})  [{}]", target.x, target.y, reason)
            }
            Action::MoveTo { target, reason } => {
                println!("MOVETO -> ({},{})  [{}]", target.x, target.y, reason)
            }
            Action::Harvest { target_id } => println!("HARVEST from {}", target_id),
            Action::Transfer {
                target_id,
                resource,
                amount,
            } => println!("TRANSFER {} {} -> {}", amount, resource, target_id),
            Action::Idle { reason } => println!("IDLE [{}]", reason),
        }
        println!();
        println!("  Legend: # wall | . plain | S spawn | E source | c/C creep");
        println!();
    }
}

// ═══════════════════════════════════════
//  Tests
// ═══════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

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
        let path = astar(
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
        let path = astar(
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
        let path = astar(
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
        let path = astar(
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
        let path = astar(
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
        let path = astar(
            &tiles, 3, 2, &[blocker],
            Position { x: 0, y: 0 },
            Position { x: 1, y: 0 },
            false,
        );
        assert!(path.is_some());

        // Пройти через блокер нельзя
        let path = astar(
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
}
