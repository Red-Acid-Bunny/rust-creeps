use crate::lua_api::{Action, NearbyEntity, Position, ScriptEngine, UnitContext};
use serde::{Deserialize, Serialize};

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
        }
    }

    pub fn can_move(&self) -> bool {
        self.body.contains(&BodyPart::Move)
    }
    pub fn can_work(&self) -> bool {
        self.body.contains(&BodyPart::Work)
    }
    pub fn has_capacity(&self) -> bool {
        self.carry < self.carry_capacity
    }
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
                    'E' => entities.push(Entity::new_source(
                        "source1",
                        Position {
                            x: x as i32,
                            y: y as i32,
                        },
                        1000,
                    )),
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

        World {
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
        }
    }

    pub fn find_by_type(&self, entity_type: EntityType, pos: Position, range: i32) -> Vec<&Entity> {
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
        if pos.x < 0 || pos.y < 0 || pos.x >= self.width as i32 || pos.y >= self.height as i32 {
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
            let ctx = self.build_unit_context(&creep);
            let action = engine.call_decide(&ctx).unwrap_or_else(|err| {
                eprintln!("[{}] Lua error: {}", creep_id, err);
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
        match action {
            Action::Move { target, .. } => {
                let creep = self.get_entity(creep_id).cloned();
                if let Some(creep) = creep {
                    if creep.can_move() {
                        if let Some(new_pos) = self.step_toward(creep.pos, *target) {
                            if let Some(c) = self.get_entity_mut(creep_id) {
                                c.pos = new_pos;
                            }
                        }
                    }
                }
            }
            Action::Harvest { target_id } => {
                let creep = self.get_entity(creep_id).cloned();
                let source = self.get_entity(target_id).cloned();
                if let (Some(creep), Some(source)) = (creep, source) {
                    if source.entity_type != EntityType::Source {
                        return;
                    }
                    if Self::distance(creep.pos, source.pos) > 1 {
                        return;
                    }
                    if !creep.can_work() || !creep.has_capacity() {
                        return;
                    }
                    let amount = self
                        .harvest_rate
                        .min(source.resource_amount)
                        .min(creep.carry_capacity - creep.carry);
                    if amount > 0 {
                        if let Some(s) = self.get_entity_mut(target_id) {
                            s.resource_amount -= amount;
                        }
                        if let Some(c) = self.get_entity_mut(creep_id) {
                            c.carry += amount;
                        }
                    }
                }
            }
            Action::Transfer {
                target_id,
                resource: _,
                amount,
            } => {
                let creep = self.get_entity(creep_id).cloned();
                if let Some(creep) = creep {
                    let transfer = (*amount).min(creep.carry);
                    if transfer == 0 {
                        return;
                    }
                    let target_pos = self.get_entity(target_id).map(|t| t.pos);
                    if let Some(tp) = target_pos {
                        if Self::distance(creep.pos, tp) > 1 {
                            return;
                        }
                    }
                    if let Some(c) = self.get_entity_mut(creep_id) {
                        c.carry -= transfer;
                    }
                    if let Some(t) = self.get_entity_mut(target_id) {
                        t.energy += transfer;
                    }
                }
            }
            Action::Idle { .. } => {}
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
