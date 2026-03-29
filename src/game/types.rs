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

/// Стоимость движения на болоте в очках скорости.
/// Plain = 1 очко, Swamp = SWAMP_COST очков.
pub const SWAMP_COST: u32 = 2;

/// Кулдаун спавна в тиках после создания крипа.
pub const SPAWN_COOLDOWN_TICKS: u32 = 3;

// ═══════════════════════════════════════
//  Action & Position — из lua_api
// ═══════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    Move { target: Position, reason: String },
    MoveTo { target: Position, reason: String },
    Harvest { target_id: String },
    Transfer { target_id: String, resource: String, amount: u32 },
    Spawn { target_id: String, body: Vec<String>, name: String },
    Idle { reason: String },
}

// ═══════════════════════════════════════
//  NearbyEntity, GameContext, UnitContext — из lua_api
// ═══════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NearbyEntity {
    pub id: String,
    pub pos: Position,
    pub resource_amount: u32,
    /// Для спавнов: оставшийся кулдаун в тиках (0 = готов к спавну).
    /// Для источников и крипов: всегда 0.
    #[serde(default)]
    pub cooldown: u32,
}

/// Контекст глобального хука before_tick(game).
/// Передаётся один раз в начале каждого тика, ДО обработки крипов.
/// Позволяет Lua-коду управлять спавном и другой глобальной логикой.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameContext {
    pub tick: u64,
    pub creeps: Vec<NearbyEntity>,
    pub spawns: Vec<NearbyEntity>,
    pub sources: Vec<NearbyEntity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitContext {
    pub id: String,
    pub pos: Position,
    pub hp: u32,
    pub max_hp: u32,
    pub energy: u32,
    pub carry_capacity: u32,
    pub carry: u32,
    pub tick: u64,
    pub nearby_sources: Vec<NearbyEntity>,
    pub nearby_spawns: Vec<NearbyEntity>,
    pub nearby_creeps: Vec<NearbyEntity>,
}

impl UnitContext {
    pub fn empty(id: &str, pos: Position) -> Self {
        UnitContext {
            id: id.to_string(),
            pos,
            hp: 100,
            max_hp: 100,
            energy: 0,
            carry_capacity: 50,
            carry: 0,
            tick: 0,
            nearby_sources: vec![],
            nearby_spawns: vec![],
            nearby_creeps: vec![],
        }
    }
}

// ═══════════════════════════════════════
//  Entity — из world.rs
// ═══════════════════════════════════════

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
    /// Последнее действие крипа (используется для проверки столкновений).
    /// Source и Spawn не используют это поле.
    #[serde(default)]
    pub last_action: Option<Action>,
    /// Кулдаун спавна: оставшиеся тики до следующего создания крипа.
    /// Только для EntityType::Spawn.
    #[serde(default)]
    pub spawn_cooldown: u32,
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
            last_action: None,
            spawn_cooldown: 0,
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
            last_action: None,
            spawn_cooldown: 0,
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
            last_action: None,
            spawn_cooldown: 0,
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

/// Стоимость создания одной body part (в единицах энергии).
pub fn body_part_cost(part: &BodyPart) -> u32 {
    match part {
        BodyPart::Move => 50,
        BodyPart::Work => 100,
        BodyPart::Carry => 50,
        BodyPart::Attack => 80,
        BodyPart::Tough => 10,
    }
}

/// Парсит строку Lua в BodyPart. Возвращает None если имя неизвестно.
pub fn parse_body_part(s: &str) -> Option<BodyPart> {
    match s.to_lowercase().as_str() {
        "move" => Some(BodyPart::Move),
        "work" => Some(BodyPart::Work),
        "carry" => Some(BodyPart::Carry),
        "attack" => Some(BodyPart::Attack),
        "tough" => Some(BodyPart::Tough),
        _ => None,
    }
}

/// Считает общую стоимость набора body parts.
pub fn body_cost(parts: &[BodyPart]) -> u32 {
    parts.iter().map(body_part_cost).sum()
}
