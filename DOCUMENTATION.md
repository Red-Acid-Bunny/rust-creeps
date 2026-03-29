# CREEP-SIM — Полная документация к коду

> Screeps-подобная симуляция: Rust-движок + LuaJIT для программирования юнитов.
> Документация написана для тех, кто только начинает знакомство с Rust.

---

## Содержание

1. [Обзор архитектуры](#1-обзор-архитектуры)
2. [Структура проекта](#2-структура-проекта)
3. [lua_api.rs — Модуль интеграции Lua](#3-lua_apirs--модуль-интеграции-lua)
   - 3.1 [Position](#31-position)
   - 3.2 [Action](#32-action)
   - 3.3 [NearbyEntity](#33-nearbyentity)
   - 3.4 [UnitContext](#34-unitcontext)
   - 3.5 [ScriptEngine](#35-scriptengine)
4. [world.rs — Модель игрового мира](#4-worldrs--модель-игрового-мира)
   - 4.1 [TileType](#41-tiletype)
   - 4.2 [EntityType](#42-entitytype)
   - 4.3 [BodyPart](#43-bodypart)
   - 4.4 [Entity](#44-entity)
   - 4.5 [World](#45-world)
5. [main.rs — Точка входа](#6-mainrs--точка-входа)
6. [Lua-скрипты](#7-lua-скрипты)
   - 6.1 [simple.lua](#71-simplelua)
   - 6.2 [harvester.lua](#72-harvesterlua)
7. [Как работает игровой тик (пошагово)](#8-как-работает-игровой-тик-пошагово)
8. [Зависимости (Cargo.toml)](#9-зависимости-cargotoml)
9. [Рекомендации по улучшению архитектуры](#10-рекомендации-по-улучшению-архитектуры)

---

## 1. Обзор архитектуры

```
┌─────────────────────────────────────────────────┐
│                   main.rs                        │
│  Определяет MAP, создаёт World и ScriptEngine,  │
│  запускает игровой цикл: tick() → render()      │
└───────────┬──────────────────────┬──────────────┘
            │                      │
   ┌────────▼────────┐    ┌───────▼──────────┐
   │    world.rs     │    │    lua_api.rs     │
   │                 │    │                   │
   │  World:         │    │  ScriptEngine:    │
   │  - tiles[][]    │    │  - LuaJIT sandbox │
   │  - entities[]   │    │  - load_script()  │
   │  - tick()       │◄──►│  - call_decide()  │
   │  - render()     │    │  - parse_action() │
   │                 │    │                   │
   │  Entity:        │    │  Типы:            │
   │  - Creep        │    │  - Action         │
   │  - Source       │    │  - Position       │
   │  - Spawn        │    │  - UnitContext    │
   └─────────────────┘    └────────┬──────────┘
                                   │
                          ┌────────▼──────────┐
                          │  scripts/*.lua     │
                          │  decide(ctx) →     │
                          │    action          │
                          └───────────────────┘
```

**Принцип работы:** Rust-движок хранит состояние мира (карта, сущности, тик-счётчик). Каждый игровой тик для каждого крипа вызывается Lua-функция `decide()`, которая получает данные о мире и возвращает действие. Rust применяет это действие к миру.

---

## 2. Структура проекта

```
rust-creeps/
├── Cargo.toml              # Конфигурация зависимостей
├── Cargo.lock              # Заблокированные версии (генерируется автоматически)
├── .gitignore              # Игнорирует /target
├── src/
│   ├── main.rs             # Точка входа: карта, цикл симуляции
│   ├── lua_api.rs          # Lua VM, типы данных, парсинг действий
│   └── world.rs            # Модель мира: тайлы, сущности, тики, рендеринг
└── scripts/
    ├── simple.lua           # Простой демо-скрипт (не используется в main.rs)
    └── harvester.lua        # Скрипт харвестера (используется в main.rs)
```

---

## 3. lua_api.rs — Модуль интеграции Lua

Этот файл — мост между Rust и Lua. Он определяет типы данных, которые передаются между двумя языками, и управляет Lua-машиной (VM).

### 3.1 Position

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Position { pub x: i32, pub y: i32 }
```

**Что это:** Координаты на 2D-карте. `i32` — знаковое целое, чтобы можно было работать с отрицательными координатами (хотя текущая карта использует только положительные).

**Почему `Copy`:** Позиция маленькая (два `i32`), поэтому копирование дешевле, чем передача по ссылке. `Copy` в Rust означает, что при присваивании или передаче в функцию создаётся точная копия, а не «перемещение» значения.

**Почему эти derive-макросы:**

- `Debug` — позволяет печатать через `{:?}` при отладке
- `Clone` — можно явно клонировать через `.clone()`
- `Copy` — неявное копирование при передаче
- `Serialize, Deserialize` — конвертация в JSON (полезно для сохранения мира)
- `PartialEq, Eq` — сравнение через `==` (нужно для проверок `pos == target`)

**Пример в Lua:**

```lua
-- Rust передаёт Position как таблицу {x, y}
local pos = ctx.pos
print(pos.x, pos.y)  -- 9, 5
```

---

### 3.2 Action

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    Move { target: Position, reason: String },
    Harvest { target_id: String },
    Transfer { target_id: String, resource: String, amount: u32 },
    Idle { reason: String },
}
```

**Что это:** Перечисление (enum) всех действий, которые юнит может выполнить за один тик. Каждый вариант может содержать дополнительные данные.

**Варианты:**

| Вариант | Поля | Что делает в мире |
|---------|------|-------------------|
| `Move` | `target: Position`, `reason: String` | Делает один шаг к `target`. `reason` — чисто информационное поле для отображения в UI |
| `Harvest` | `target_id: String` | Добывает энергию из источника с ID `target_id`. Крип должен быть рядом (dist <= 1), иметь `Work`-часть и место в `carry` |
| `Transfer` | `target_id`, `resource`, `amount` | Передаёт `amount` ресурса `resource` цели с ID `target_id`. Крип должен быть рядом |
| `Idle` | `reason: String` | Ничего не делает. `reason` для отображения |

**Почему enum, а не struct:** В Screeps юнит выполняет ровно одно действие за тик. Enum гарантирует, что Lua вернёт именно один тип — Rust не может «забыть» обработать вариант, компилятор заставит расписать все ветки в `match`.

**Как это выглядит в Lua:**

```lua
--Lua возвращает таблицу, Rust парсит её в enum
return { type = "move", target = { x = 5, y = 10 }, reason = "иду к источнику" }
return { type = "harvest", target_id = "source1" }
return { type = "transfer", target_id = "spawn1", resource = "energy", amount = 50 }
return { type = "idle", reason = "нет целей" }
```

---

### 3.3 NearbyEntity

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NearbyEntity {
    pub id: String,
    pub pos: Position,
    pub resource_amount: u32,
}
```

**Что это:** Упрощённое описание сущности, которое юнит «видит» рядом. В отличие от полного `Entity`, содержит только ту информацию, которая нужна Lua-скрипту для принятия решения.

**Почему отдельный тип, а не просто `Entity`:** Принцип минимальных привилегий. Lua-скрипт не должен знать HP чужих крипов или их body parts — только позицию и количество ресурса. Если дать полный `Entity`, игрок сможет использовать внутренние данные для нечестной игры.

**Смысл `resource_amount` зависит от типа сущности:**

- Для Source — сколько энергии осталось в источнике
- Для Spawn — сколько энергии хранит спавн (можно использовать как индикатор «хватит ли нам энергии»)
- Для Creep — сколько ресурса несёт другой крип (для координации)

---

### 3.4 UnitContext

```rust
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
```

**Что это:** Полный набор данных о юните и его окружении, который передаётся в Lua-функцию `decide()`. Это «глаза и уши» крипа — всё, что он знает о мире.

**Поля:**

| Поле | Тип | Описание |
|------|-----|----------|
| `id` | `String` | Уникальный идентификатор крипа (например `"worker_1"`) |
| `pos` | `Position` | Текущие координаты `{x, y}` |
| `hp` / `max_hp` | `u32` | Текущее и максимальное здоровье |
| `energy` | `u32` | Внутренняя энергия крипа (резерв, пока не используется в механиках) |
| `carry_capacity` | `u32` | Максимум ресурса, который может нести |
| `carry` | `u32` | Текущий перевозимый ресурс |
| `tick` | `u64` | Номер текущего тика (полезно для таймеров в Lua) |
| `nearby_sources` | `Vec<NearbyEntity>` | Источники энергии в радиусе `view_range` |
| `nearby_spawns` | `Vec<NearbyEntity>` | Спавны в радиусе видимости |
| `nearby_creeps` | `Vec<NearbyEntity>` | Другие крипы в радиусе видимости (без себя) |

**Метод `empty()`:**

```rust
pub fn empty(id: &str, pos: Position) -> Self { ... }
```

Создаёт контекст с нулевыми значениями и пустыми списками nearby. Используется в тестах, когда нужно передать минимальный контекст, не заботясь о всех полях.

---

### 3.5 ScriptEngine

```rust
pub struct ScriptEngine { lua: Lua }
```

**Что это:** Обёртка над LuaJIT-машиной. Каждая `ScriptEngine` содержит один экземпляр Lua VM. Все юниты сейчас используют одну и ту же VM (разделяют Lua-состояние). Это нормально для одиночной игры, но для мультиплеера каждому игроку нужна своя VM.

#### `ScriptEngine::new() -> LuaResult<Self>`

Создаёт Lua-машину и настраивает sandbox:

```rust
pub fn new() -> LuaResult<Self> {
    let lua = Lua::new();
    lua.load(r#"
        -- 1. Добавляем функцию distance() — Манхэттенское расстояние
        function distance(a, b)
            return math.abs(a.x - b.x) + math.abs(a.y - b.y)
        end

        -- 2. Удаляем опасные модули
        os, io, debug, require, dofile, loadfile, load, package = nil, nil, nil, nil, nil, nil, nil, nil

        -- 3. Блокируем доступ к несуществующим глобалам
        local _mt = getmetatable(_G) or {}
        _mt.__index = function(_, key) return nil end
        setmetatable(_G, _mt)
    "#).exec()?;
    Ok(Self { lua })
}
```

**Как работает sandbox (пошагово):**

1. `distance(a, b)` — регистрируем глобальную функцию. Lua-скрипты могут вызывать `distance({x=1,y=1}, {x=3,y=4})` и получить `5`.

2. `os, io, debug, require = nil` — зануляем опасные модули. Даже если LuaJIT загрузил их по умолчанию, после этого `os.execute()` вызовет ошибку `"attempt to index global 'os' (a nil value)"`.

3. `setmetatable(_G, { __index = ... })` — это ключевой трюк. В Lua 5.1 (который использует LuaJIT) нельзя «заменить» глобальную таблицу `_G`. Но можно поставить метатаблицу с `__index`, которая возвращает `nil` для любых неизвестных ключей. Это значит, что обращение к любой несуществующей переменной (включая те, которые занулили на шаге 2) вернёт `nil` вместо ошибки, и — главное — не позволит обратиться к оригинальным значениям через цепочку прототипов.

**Почему `LuaResult`:** Это тип-алиас `mlua::Result<T>`. Все операции с Lua могут завершиться ошибкой (скрипт упал, не та сигнатура функции, нехватка памяти). Rust заставляет обрабатывать эти ошибки через `?` или `match`.

---

#### `load_script(&self, path: &Path) -> LuaResult<()>`

```rust
pub fn load_script(&self, path: &Path) -> LuaResult<()> {
    let code = std::fs::read_to_string(path)
        .map_err(|e| mlua::Error::external(...))?;
    self.lua.load(&code).exec()
}
```

Читает файл с диска и выполняет его в Lua VM. Это загружает функции (например `decide`) в глобальное пространство Lua, чтобы потом вызывать их через `call_decide()`.

**Важно:** `exec()` выполняет код, но не возвращает значение. Это правильно для загрузки скриптов — скрипт только определяет функции, а вызывает их Rust позже.

---

#### `load_script_from_str(&self, code: &str) -> LuaResult<()>`

То же самое, но принимает строку вместо пути к файлу. Удобно для тестов и для загрузки скриптов из сети (в будущем).

---

#### `call_decide(&self, context: &UnitContext) -> LuaResult<Action>`

```rust
pub fn call_decide(&self, context: &UnitContext) -> LuaResult<Action> {
    let ctx_table = self.context_to_lua(context)?;
    let decide_fn: mlua::Function = self.lua.globals().get("decide")?;
    let result: Table = decide_fn.call(ctx_table)?;
    self.parse_action(result)
}
```

**Это главная функция модуля.** Она:

1. **`context_to_lua(context)`** — конвертирует Rust-структуру `UnitContext` в Lua-таблицу (см. ниже)
2. **`self.lua.globals().get("decide")`** — достаёт Lua-функцию `decide` из глобального пространства
3. **`decide_fn.call(ctx_table)`** — вызывает `decide(context)` в Lua и получает результат
4. **`parse_action(result)`** — конвертирует Lua-таблицу-результат обратно в Rust-enum `Action`

**Пример потока данных:**

```
Rust UnitContext           Lua видит:              Lua возвращает:         Rust получает:
─────────────────         ──────────               ──────────────          ────────────
id: "worker_1"      →     ctx.id = "worker_1"
pos: {x:9, y:4}     →     ctx.pos = {x:9, y:4}
carry: 30            →     ctx.carry = 30
nearby_sources: [...] →     ctx.nearby_sources = [...]
                                                         { type = "harvest",    →   Action::Harvest
                                                           target_id = "src1" }      { target_id: "src1" }
```

---

#### `global_is_nil(&self, name: &str) -> LuaResult<bool>`

Проверяет, является ли глобальная переменная в Lua равной `nil`. Используется для проверки sandbox (что `os`, `io`, `debug` действительно заблокированы). В основном нужно для тестов.

---

#### `fn context_to_lua(&self, ctx: &UnitContext) -> LuaResult<Table>` (приватный)

Конвертирует Rust-структуру в Lua-таблицу. Создаёт вложенные таблицы:

```
Lua получает:
{
    id = "worker_1",
    pos = { x = 9, y = 4 },
    hp = 100,
    max_hp = 100,
    carry = 30,
    carry_capacity = 50,
    tick = 42,
    nearby_sources = {
        { id = "source1", pos = { x = 9, y = 5 }, resource_amount = 950 },
    },
    nearby_spawns = {
        { id = "spawn1", pos = { x = 2, y = 2 }, resource_amount = 300 },
    },
    nearby_creeps = {
        { id = "worker_2", pos = { x = 11, y = 7 }, resource_amount = 20 },
    },
}
```

**Почему ручная конвертация, а не `serde`:** `mlua` имеет feature `serialize`, но ручная конвертация даёт полный контроль над тем, какие поля доступны в Lua. Также это проще для отладки — видно каждую строку.

**Примечание:** Также устанавливает `unit_context` как глобальную переменную в Lua для удобства отладки (можно сделать `print(unit_context.id)` прямо в Lua).

---

#### `fn vec_nearby_to_lua(&self, entities: &[NearbyEntity]) -> LuaResult<Table>` (приватный)

Конвертирует вектор Rust-структур в Lua-массив (таблицу с числовыми ключами, начинающимися с 1 — как принято в Lua).

```rust
for (i, entity) in entities.iter().enumerate() {
    // ...
    table.set(i + 1, row)?;  // Lua-индексация с 1!
}
```

**Почему `i + 1`:** В Lua массивы индексируются с 1. Если использовать `i` (начиная с 0), то `#table` (оператор длины в Lua) будет работать некорректно.

---

#### `fn parse_action(&self, table: Table) -> LuaResult<Action>` (приватный)

Парсит Lua-таблицу, возвращённую из `decide()`, в Rust-enum `Action`. Работает через `match` по полю `type`:

```rust
match action_type.as_str() {
    "move" => { ... Action::Move { target, reason } ... }
    "harvest" => { ... Action::Harvest { target_id } ... }
    "transfer" => { ... Action::Transfer { target_id, resource, amount } ... }
    "idle" => { ... Action::Idle { reason } ... }
    other => Err(...)  // неизвестный тип → ошибка
}
```

**Почему `unwrap_or_default()`:** Поля вроде `reason` необязательны в Lua-скрипте. Если скрипт не указал `reason`, подставляется пустая строка. Это удобно — Lua-код можно писать короче.

---

## 4. world.rs — Модель игрового мира

### 4.1 TileType

```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TileType { Plain, Wall, Swamp }
```

**Что это:** Тип тайла (клетки) на карте.

| Вариант | Символ на карте | Проходимость |
|---------|----------------|-------------|
| `Plain` | `.` | Да, нормальная скорость |
| `Wall` | `#` | Нет |
| `Swamp` | `~` | Да, но должна замедлять (пока не реализовано) |

`Copy` — потому что enum без данных внутри, копирование тривиально.

---

### 4.2 EntityType

```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EntityType { Creep, Source, Spawn }
```

**Что это:** Тип сущности (объекта на карте). Определяет, как сущность обрабатывается в игровой логике и как отображается.

| Вариант | Описание | Символ рендера |
|---------|----------|---------------|
| `Creep` | Программируемый юнит | `c` (пустой) / `C` (несёт ресурс) |
| `Source` | Источник энергии | `E` (активный) / `e` (истощён) |
| `Spawn` | Точка спавна (база) | `S` |

---

### 4.3 BodyPart

```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BodyPart { Move, Work, Carry, Attack, Tough }
```

**Что это:** Часть тела крипа (аналог Screeps). Каждая часть добавляет определённую способность.

| Часть | Эффект при создании крипа |
|-------|--------------------------|
| `Move` | Позволяет двигаться (проверяется в `can_move()`) |
| `Work` | Позволяет добывать ресурсы (проверяется в `can_work()`) |
| `Carry` | Увеличивает `carry_capacity` на 50 |
| `Attack` | Зарезервирован для будущей боевой системы |
| `Tough` | Увеличивает `max_hp` на 100 |

**Как считается HP и carry при создании:**

```rust
pub fn new_creep(id: &str, pos: Position, body: Vec<BodyPart>) -> Self {
    let mut hp = 100u32;         // базовое HP
    let mut carry_capacity = 0u32;
    for part in &body {
        match part {
            BodyPart::Tough => hp += 100,
            BodyPart::Carry => carry_capacity += 50,
            _ => {}
        }
    }
    // ...
}
```

**Пример:** Крип с `[Move, Move, Work, Carry]`:

- HP = 100 (базовое, нет Tough)
- carry_capacity = 50 (один Carry)
- Может двигаться и добывать

Крип с `[Move, Tough, Tough, Work, Carry, Carry]`:

- HP = 100 + 100 + 100 = 300
- carry_capacity = 50 + 50 = 100
- Может двигаться и добывать

---

### 4.4 Entity

```rust
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
```

**Что это:** Универсальная сущность на карте. Одна структура для всех типов — крип, источник и спавн. Поля, которые не нужны конкретному типу, остаются нулевыми.

**Почему не разные структуры для каждого типа:** Удобство поиска и хранения — все сущности в одном векторе `Vec<Entity>`, и фильтрация по `entity_type`. Недостаток — часть полей «тратится» впустую для некоторых типов. Для простоты проекта это допустимо.

**Конструкторы:**

#### `Entity::new_source(id, pos, amount)`

```rust
Entity::new_source("source1", Position { x: 9, y: 5 }, 1000)
```

Создаёт источник энергии с `resource_amount = 1000`. Все остальные поля — 0/пустые.

#### `Entity::new_spawn(id, pos, initial_energy)`

```rust
Entity::new_spawn("spawn1", Position { x: 2, y: 2 }, 300)
```

Создаёт спавн с `hp = 5000`, `carry_capacity = 1000`, `energy = 300` (начальный запас).

#### `Entity::new_creep(id, pos, body)`

```rust
Entity::new_creep("worker_1", pos, vec![BodyPart::Move, BodyPart::Move, BodyPart::Work, BodyPart::Carry])
```

Создаёт крипа. HP и carry_capacity рассчитываются из `body` (см. BodyPart выше).

**Методы-помощники:**

```rust
pub fn can_move(&self) -> bool  // true если есть хотя бы одна часть Move
pub fn can_work(&self) -> bool  // true если есть хотя бы одна часть Work
pub fn has_capacity(&self) -> bool  // true если carry < carry_capacity
```

---

### 4.5 World

```rust
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
```

**Что это:** Корневая структура — весь игровой мир. Содержит карту (тайлы), все сущности, счётчик тиков и настройки.

**Поля:**

| Поле | Тип | Описание |
|------|-----|----------|
| `width`, `height` | `usize` | Размер карты в клетках |
| `tiles` | `Vec<Vec<TileType>>` | 2D-массив тайлов. `tiles[y][x]` — сначала строка (y), потом столбец (x) |
| `entities` | `Vec<Entity>` | Все сущности на карте (крипы, источники, спавны) |
| `tick` | `u64` | Текущий номер тика (инкрементируется каждый `tick()`) |
| `view_range` | `i32` | Радиус видимости юнитов (Манхэттенское расстояние). По умолчанию 10, в main.rs установлен в 20 |
| `harvest_rate` | `u32` | Сколько энергии добывается за один тик. По умолчанию 10 |
| `last_action` | `Action` | Последнее действие любого крипа (для отображения в UI) |

---

#### `World::from_map(map_strings: &[&str]) -> Self`

**Самый важный конструктор.** Создаёт мир из массива строк:

```rust
const MAP: &[&str] = &[
    "##############",
    "#............#",
    "#.S.......c..#",
    "#............#",
    "#............#",
    "#........E...#",
    "#............#",
    "#...........c#",
    "##############",
];

let world = World::from_map(MAP);
```

**Как работает:**

1. Определяет `width` = длине самой длинной строки, `height` = количеству строк
2. Заполняет `tiles[][]` по символам: `#` → Wall, `~` → Swamp, всё остальное → Plain
3. Создаёт сущности по символам:
   - `S` → `Entity::new_spawn("spawn1", ...)`
   - `E` → `Entity::new_source("source1", ...)`
   - `c` → `Entity::new_creep("worker_N", ...)` с `N` = порядковый номер
4. Все крипы получают тело `[Move, Move, Work, Carry]`

**Ограничения текущей реализации:**

- Только один источник (`"source1"`) и один спавн (`"spawn1"`) — если на карте два `E`, второй получит тот же ID и затрёт первый
- ID крипов генерируются автоматически: `worker_1`, `worker_2` и т.д.

---

#### `find_by_type(&self, entity_type, pos, range) -> Vec<&Entity>`

```rust
let sources = world.find_by_type(EntityType::Source, creep.pos, world.view_range);
```

Ищет все сущности заданного типа в радиусе `range` от позиции `pos` (Манхэттенское расстояние). Возвращает вектор ссылок на найденные сущности.

**Почему ссылки `&Entity`, а не копии:** Эффективность — нет нужды копировать данные. Rust гарантирует, что ссылки валидны пока существует `&self`.

---

#### `get_entity(&self, id) -> Option<&Entity>` и `get_entity_mut(&mut self, id) -> Option<&mut Entity>`

Поиск сущности по ID. `get_entity` — для чтения, `get_entity_mut` — для изменения.

**Почему `Option`:** Сущность с таким ID может не существовать (например, если Lua-скрипт передал неправильный `target_id`). `Option` заставляет вызывающий код обработать оба случая.

---

#### `is_walkable(&self, pos: Position) -> bool`

Проверяет, может ли юнит встать на клетку:

```rust
pub fn is_walkable(&self, pos: Position) -> bool {
    // 1. Не выходит за границы карты
    if pos.x < 0 || pos.y < 0 || pos.x >= self.width as i32 || pos.y >= self.height as i32 {
        return false;
    }
    // 2. Не стена
    if self.tiles[pos.y as usize][pos.x as usize] == TileType::Wall { return false; }
    // 3. Не занято непроходимой сущностью (Source, Spawn)
    for e in &self.entities {
        if e.pos.x == pos.x && e.pos.y == pos.y {
            if e.entity_type == EntityType::Source || e.entity_type == EntityType::Spawn {
                return false;
            }
        }
    }
    true
}
```

**Почему Source и Spawn непроходимы:** В Screeps источники и структуры занимают клетку. Крип может стоять рядом, но не на них.

---

#### `step_toward(&self, from: Position, to: Position) -> Option<Position>`

Жадный (greedy) алгоритм движения — один шаг к цели:

```rust
pub fn step_toward(&self, from: Position, to: Position) -> Option<Position> {
    if from == to { return None; }  // уже на месте
    let dx = (to.x - from.x).signum();  // -1, 0 или 1
    let dy = (to.y - from.y).signum();
    // Двигаемся сначала по большей оси
    let candidates = if dx.abs() >= dy.abs() {
        vec![Position { x: from.x + dx, y: from.y }, Position { x: from.x, y: from.y + dy }]
    } else {
        vec![Position { x: from.x, y: from.y + dy }, Position { x: from.x + dx, y: from.y }]
    };
    candidates.into_iter().find(|p| self.is_walkable(*p))
}
```

**Как работает:**

1. Если `from == to` — никуда не идём, возвращаем `None`
2. Вычисляем направление по каждой оси: `signum()` даёт -1, 0 или 1
3. Генерируем 2 кандидата: шаг по большей оси + шаг по меньшей
4. Возвращаем первый проходимый кандидат

**`.signum()`** — это метод `i32`, который возвращает знак числа: `-5.signum() = -1`, `0.signum() = 0`, `5.signum() = 1`.

**Пример:** Крип на (3, 3) хочет идти к (7, 5):

- `dx = 4`, `dy = 2` → `dx > dy`, двигаемся сначала по X
- Кандидаты: `(4, 3)` и `(3, 4)`
- Если `(4, 3)` проходима → идём туда

**Это НЕ pathfinding.** Если на пути стена, крип попытается обойти (перейдёт к кандидату по Y), но не найдёт оптимальный путь. Для обхода препятствий нужен A*.

---

#### `distance(a: Position, b: Position) -> i32` (статический)

```rust
World::distance(Position { x: 3, y: 3 }, Position { x: 9, y: 5 })  // = 8
```

Манхэттенское расстояние: `|x1-x2| + |y1-y2|`. Статический метод — вызывается как `World::distance()`, без экземпляра.

---

#### `tick(&mut self, engine: &ScriptEngine)`

**Главный метод — один игровой тик.** Вызывается каждый кадр симуляции:

```rust
pub fn tick(&mut self, engine: &ScriptEngine) {
    // 1. Собираем ID всех крипов (нужны только кривы)
    let creep_ids: Vec<String> = self.entities.iter()
        .filter(|e| e.entity_type == EntityType::Creep)
        .map(|e| e.id.clone()).collect();

    // 2. Для каждого крипа: контекст → Lua → action → применить
    for creep_id in &creep_ids {
        let creep = match self.get_entity(creep_id).cloned() { ... };
        let ctx = self.build_unit_context(&creep);
        let action = engine.call_decide(&ctx).unwrap_or_else(|err| {
            Action::Idle { reason: format!("script error: {}", err) }
        });
        self.last_action = action.clone();
        self.apply_action(creep_id, &action);
    }

    self.tick += 1;
}
```

**Почему клонируем данные крипа (`cloned()`):** Rust запрещает одновременно иметь неизменяемую ссылку на `self` (для поиска крипа) и изменяемую (для `apply_action`). Клонирование — самый простой способ обойти это. В будущем можно оптимизировать через `IndexMap` или разделение на фазы.

**Почему сначала собираем все ID:** Если бы мы итерировали напрямую по `self.entities`, а `apply_action` потенциально добавлял бы новые сущности — это бы вызвало ошибку изменения коллекции во время итерации (borrow checker не пропустил бы).

---

#### `build_unit_context(&self, creep: &Entity) -> UnitContext` (публичный)

Собирает данные о мире, видимые крипом:

```rust
let sources = self.find_by_type(EntityType::Source, creep.pos, self.view_range);
let spawns = self.find_by_type(EntityType::Spawn, creep.pos, self.view_range);
let creeps = self.find_by_type(EntityType::Creep, creep.pos, self.view_range)
    .filter(|e| e.id != creep.id);  // исключаем самого себя
```

Затем конвертирует всё в `UnitContext` и отдаёт Lua.

---

#### `fn apply_action(&mut self, creep_id: &str, action: &Action)` (приватный)

Применяет действие к миру. Разбирём каждую ветку:

**Move:**

```rust
Action::Move { target, reason: _ } => {
    let creep = self.get_entity(creep_id).cloned();
    if let Some(creep) = creep {
        if creep.can_move() {
            if let Some(new_pos) = self.step_toward(creep.pos, *target) {
                if let Some(c) = self.get_entity_mut(creep_id) { c.pos = new_pos; }
            }
        }
    }
}
```

1. Получаем копию крипа (отпускаем borrow на `self`)
2. Проверяем: есть ли часть `Move` в теле?
3. Пытаемся сделать шаг через `step_toward`
4. Если шаг удался — обновляем позицию

**Harvest:**

```rust
Action::Harvest { target_id } => {
    let creep = self.get_entity(creep_id).cloned();
    let source = self.get_entity(target_id).cloned();
    if let (Some(creep), Some(source)) = (creep, source) {
        if source.entity_type != EntityType::Source { return; }
        if Self::distance(creep.pos, source.pos) > 1 { return; }
        if !creep.can_work() || !creep.has_capacity() { return; }
        let amount = self.harvest_rate.min(source.resource_amount).min(creep.carry_capacity - creep.carry);
        if amount > 0 {
            if let Some(s) = self.get_entity_mut(target_id) { s.resource_amount -= amount; }
            if let Some(c) = self.get_entity_mut(creep_id) { c.carry += amount; }
        }
    }
}
```

Валидации:

- Цель должна быть `Source`
- Расстояние <= 1 (рядом)
- У крипа есть `Work`-часть
- У крипа есть место (`has_capacity`)
- Количество = минимум из: `harvest_rate`, остаток источника, свободное место

**Transfer:**

```rust
Action::Transfer { target_id, resource: _, amount } => {
    // analogous to Harvest but transfers carry → target.energy
}
```

Передаёт `amount` из `carry` крипа в `energy` цели. Требует расстояние <= 1.

---

#### `render(&self)`

Отрисовка мира в терминале через ANSI escape-коды:

```rust
print!("\x1B[2J\x1B[H");  // Очистка экрана и курсор в начало
```

`\x1B[2J` — очистить экран, `\x1B[H` — переместить курсор в (0,0).

Для каждой клетки карты определяет символ:

- Если на клетке сущность — рисует символ типа (c/C/E/e/S)
- Иначе — символ тайла (./#/~)

Ниже карты выводит список всех сущностей с их статусом и последнее действие.

---

## 6. main.rs — Точка входа

```rust
const MAP: &[&str] = &[ ... ];
const TOTAL_TICKS: u64 = 4500;
const TICK_DELAY_MS: u64 = 300;
```

`MAP` — определение карты как массив строк. `TOTAL_TICKS` — сколько тиков отыграть. `TICK_DELAY_MS` — пауза между тиками в миллисекундах.

**Игровой цикл:**

```rust
let mut world = World::from_map(MAP);
world.view_range = 20;  // переопределяем дальность видимости

let engine = ScriptEngine::new().expect("...");
engine.load_script(Path::new("scripts/harvester.lua")).expect("...");

for _ in 0..TOTAL_TICKS {
    world.tick(&engine);   // 1. Вычислить тик
    world.render();         // 2. Отрисовать
    thread::sleep(Duration::from_millis(TICK_DELAY_MS));  // 3. Подождать
}
```

`.expect("...")` — метод `Result`, который "разворачивает" результат (достаёт значение) или паникует с сообщением. Удобно для программы, где ошибки критичны — лучше упасть сразу, чем молча продолжить без Lua.

`thread::sleep` — блокирует поток на указанное время. В реальной игре лучше использовать более точные таймеры, но для демо достаточно.

---

## 7. Lua-скрипты

### 7.1 simple.lua

Демо-скрипт, не используется в основном цикле. Показывает базовый синтаксис:

```lua
function decide(unit_context)
    local action = {}
    local x = unit_context.pos.x
    local y = unit_context.pos.y
    local energy = unit_context.energy

    if energy < 50 then
        action.type = "move"
        action.target = { x = x, y = y + 1 }
        action.reason = "going to energy source"
    else
        action.type = "move"
        action.target = { x = 0, y = 0 }
        action.reason = "returning to base"
    end
    return action
end
```

**Ключевые особенности Lua для новичков:**

- Переменные: `local` — локальная, без `local` — глобальная (всегда используй `local`)
- Строки: двойные `"..."` или одинарные `'...'` кавычки
- Таблицы: `{ key = value, ... }` — это и ассоциативные массивы, и массивы, и объекты
- Условие: `if ... then ... elseif ... then ... else ... end`
- Комментарии: `--` однострочный, `--[[ ]]` многострочный

---

### 7.2 harvester.lua

Основной скрипт, используемый в симуляции. Реализует классический Screeps-паттерн «harvester»:

**Логика:**

1. Если крип несёт ресурс (`carry > 0`):
   - Если груз полный **или** источник исчерпан → идти к спавну и передать
   - Иначе (груз не полный, источник ещё есть) → продолжить добычу
2. Если крип пустой:
   - Найти ближайший источник
   - Если рядом (dist <= 1) → добывать
   - Иначе → идти к источнику
3. Запасной вариант → idle

**Интересный паттерн — досыпка до полного:**

```lua
-- Крип с [30/50] рядом с источником НЕ идёт на спавн
-- Он остаётся добывать, пока не заполнит carry полностью
if ctx.carry >= ctx.carry_capacity or not source_ok then
    -- Только тогда идём доставлять
end
```

Это эффективнее, чем «пошёл доставлять 10 единиц, потом вернулся за остальными».

**Встроенная функция `distance()`:**

```lua
local d = distance(ctx.pos, source.pos)  -- Манхэттенское расстояние
if d <= 1 then
    -- Рядом — можно действовать
end
```

---

## 8. Как работает игровой тик (пошагово)

Полный цикл одного тика, для одного крипа:

```
1. Rust: world.tick(&engine)
    │
    ├─ 2. Собираем ID всех крипов: ["worker_1", "worker_2"]
    │
    ├─ 3. Для worker_1:
    │     ├─ 3a. get_entity("worker_1").cloned() → Entity { pos: (9,4), carry: 30, ... }
    │     ├─ 3b. build_unit_context(&creep) → UnitContext {
    │     │       id: "worker_1", pos: {x:9, y:4}, carry: 30,
    │     │       nearby_sources: [{ id: "source1", pos: {x:9,y:5}, resource_amount: 950 }],
    │     │       nearby_spawns: [{ id: "spawn1", pos: {x:2,y:2}, resource_amount: 300 }],
    │     │       ...
    │     │   }
    │     ├─ 3c. context_to_lua(ctx) → Lua-таблица
    │     ├─ 3d. Lua: decide(ctx_table) → { type: "harvest", target_id: "source1" }
    │     ├─ 3e. parse_action(result) → Action::Harvest { target_id: "source1" }
    │     └─ 3f. apply_action("worker_1", Harvest { target_id: "source1" })
    │           ├─ Проверяем: distance(worker_1, source1) = 1 ✓
    │           ├─ Проверяем: can_work() = true ✓
    │           ├─ Проверяем: has_capacity() = true (30 < 50) ✓
    │           ├─ amount = min(10, 950, 20) = 10
    │           ├─ source1.resource_amount: 950 → 940
    │           └─ worker_1.carry: 30 → 40
    │
    ├─ 4. Для worker_2: (тот же процесс)
    │
    └─ 5. tick += 1  (теперь tick = N+1)

6. Rust: world.render()
    └─ Отрисовка карты в терминал

7. thread::sleep(300ms)
    └─ Пауза до следующего тика
```

---

## 9. Зависимости (Cargo.toml)

```toml
mlua = { version = "0.10", features = ["luajit52", "vendored", "serialize"] }
```

- `luajit52` — использует LuaJIT с совместимостью Lua 5.2 (современный синтаксис)
- `vendored` — LuaJIT компилируется из исходников при сборке. Не нужно устанавливать LuaJIT в системе
- `serialize` — интеграция с `serde` для конвертации Rust ↔ Lua

```toml
ratatui = "0.29"
crossterm = "0.28"
```

Библиотеки для терминального UI. **В текущем коде не используются** — рендеринг реализован через raw ANSI-escape-коды. Зарезервированы для будущего TUI.

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Сериализация. `derive` позволяет генерировать `Serialize/Deserialize` через `#[derive(Serialize, Deserialize)]`. Используется в типах данных.

```toml
tracing = "0.1"
tracing-subscriber = "0.3"
```

Фреймворк логирования. **В текущем коде не используется.** Зарезервирован для отладки Lua-скриптов и профилирования.

---

## 10. Рекомендации по улучшению архитектуры

### 10.1 Разделение логики и рендеринга

**Проблема сейчас:** `World::render()` прямо внутри `world.rs` рисует ANSI-коды. Логика и графика смешаны.

**Решение — паттерн «отделение ответственности» (Separation of Concerns):**

```
world.rs          ← чистая игровая логика, ничего про графику
renderer.rs       ← интерфейс рендеринга (trait)
  ├─ terminal.rs  ← реализация: ANSI-терминал
  ├─ ratatui.rs   ← реализация: ratatui TUI
  ├─ text.rs      ← реализация: просто текстовый лог
  └─ web.rs       ← реализация: WebSocket + JSON для браузера
```

**Trait Renderer:**

```rust
pub trait Renderer {
    /// Вызывается после каждого тика для отображения состояния
    fn render(&self, world: &World);
}
```

Каждый рендерер реализует этот trait. В `main.rs` — выбирается нужный:

```rust
let renderer: Box<dyn Renderer> = match args.renderer {
    "terminal" => Box::new(TerminalRenderer::new()),
    "text" => Box::new(TextRenderer::new()),
    "web" => Box::new(WebRenderer::new("0.0.0.0:8080")),
};
```

**Entity как сущность, а не символ:** При таком разделении `Entity` содержит только данные (позиция, HP, тип). Рендерер решает, как отобразить:

- Terminal: `c` / `C` / `S` / `E`
- TUI: цветные блоки в ratatui
- Web: JSON-объект для фронтенда
- Text: `"Worker #1 at (5,3) carrying 30/50 energy"`

---

### 10.2 Мультиплеер

**Архитектура для мультиплеера:**

```
┌──────────────────┐
│  Game Server     │
│  (Rust, Axum)    │
│                  │
│  World state     │◄──── WebSocket от клиентов
│  Tick loop       │──── JSON: world snapshot
│                  │
│  Per-player:     │
│  - ScriptEngine  │
│  - Lua sandbox   │
└──────────────────┘
        │
   WebSocket/HTTP
        │
┌───────▼──────────┐
│  Браузерный UI   │
│  (JS/Canvas)     │
│  или TUI клиент  │
└──────────────────┘
```

**Ключевые решения:**

1. **Детерминизм:** Все клиенты получают одно и то же состояние мира. Lua-скрипты выполняются на сервере, клиент — только показывает результат. Это исключает читерство.

2. **Изоляция игроков:** У каждого игрока своя `ScriptEngine`. Проверки по ID: юнит может видеть только свои сущности и общие объекты (источники, стены).

3. **Асинхронные тики:** `tokio` + `axum` для WebSocket:

   ```rust
   use axum::{Router, extract::ws::WebSocket};
   use tokio::time::interval;

   async fn game_loop(state: SharedState) {
       let mut ticker = interval(Duration::from_secs(1));
       loop {
           ticker.tick().await;
           state.lock().tick();
           state.broadcast_snapshot().await;
       }
   }
   ```

4. **API для клиентов:**
   - WebSocket: получение обновлений мира в реальном времени
   - HTTP REST: загрузка/перезагрузка Lua-скриптов, просмотр логов

---

### 10.3 Перезагрузка скриптов в реальном времени

**Проблема сейчас:** Скрипт загружается один раз при старте. Чтобы изменить логику — нужно перезапустить программу.

**Решение — file watcher:**

```rust
use notify::{Watcher, RecursiveMode, watcher};

fn setup_script_reloader(engine: Arc<Mutex<ScriptEngine>>, path: PathBuf) {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = watcher(tx, Duration::from_secs(1)).unwrap();
    watcher.watch(&path, RecursiveMode::NonRecursive).unwrap();

    thread::spawn(move || {
        for event in rx {
            if let Ok(event) = event {
                if event.kind.is_modify() {
                    let mut engine = engine.lock().unwrap();
                    match engine.reload_script(&path) {
                        Ok(()) => println!("Script reloaded!"),
                        Err(e) => eprintln!("Reload error: {}", e),
                    }
                }
            }
        }
    });
}
```

Зависимость: `notify = "6"` в Cargo.toml.

**Метод в `ScriptEngine`:**

```rust
impl ScriptEngine {
    pub fn reload_script(&self, path: &Path) -> LuaResult<()> {
        // Создаём новую VM с sandbox (чистое состояние)
        let lua = Lua::new();
        // ... повторяем setup sandbox ...
        let code = std::fs::read_to_string(path)?;
        lua.load(&code).exec()?;
        self.lua = lua;  // заменяем VM
        Ok(())
    }
}
```

**Что ещё нужно учесть:**

- В Screeps есть `Memory` — глобальный объект, который переживает перезагрузку. Нужно сериализовать `Memory` перед перезагрузкой и восстановить после
- Проверять скрипты на ошибки ДО перезагрузки: загрузить в отдельную тестовую VM, и только если `decide()` отработал без ошибок — применять
- Добавить HTTP-эндпоинт для загрузки скрипта из браузера (для веб-мультиплеера)

---

### 10.4 Генерация карт

**Проблема сейчас:** Карта — статический массив строк в `main.rs`. Каждое изменение — перекомпиляция.

**Этапы решения:**

1. **Загрузка из файла:**

   ```rust
   let map = std::fs::read_to_string("maps/arena01.txt")?;
   let lines: Vec<&str> = map.lines().collect();
   let world = World::from_map(&lines);
   ```

2. **JSON-конфигурация:**

   ```json
   {
     "width": 30,
     "height": 20,
     "rooms": [
       { "x": 0, "y": 0, "spawn": [2, 2], "sources": [[8, 5], [5, 10]] }
     ]
   }
   ```

   World::from_json() генерирует карту из конфига, размещая стены по краям и сущности в указанных точках.

3. **Процедурная генерация:**
   - Простая: случайное размещение N источников и M спавнов в пустых клетках
   - Продвинутая: клеточные автоматы (Cave Generation), BSP-деревья (Room Generation)
   - Биомы: случайные кластеры болот, горных стен

4. **Редактор карт (будущее):**
   - TUI-редитор в ratatui: стрелками перемещаешься, клавишами ставишь стены/сущности
   - Или веб-редитор с drag-and-drop

---

### 10.5 Дополнительные улучшения

**A* Pathfinding:**
Заменить жадный `step_toward` на A* с библиотекой `pathfinding = "4"`:

```rust
use pathfinding::directed::astar::astar;

pub fn find_path(&self, from: Position, to: Position) -> Option<Vec<Position>> {
    astar(
        &from,
        |p| self.successors(*p),      // все проходимые соседи
        |p| Self::distance(*p, to),    // эвристика — Манхэттен
        |p| *p == to,                  // условие остановки
    ).map(|(path, _cost)| path)
}
```

Крип будет хранить путь (`Vec<Position>`) и выполнять по одному шагу за тик из пути.

**Система Memory (персистентное хранилище):**

В Screeps `Memory` — это JSON-объект, который сохраняется между тиками. Крипы могут записывать туда свои цели:

```lua
-- В Lua:
Memory["worker_1"] = Memory["worker_1"] or {}
Memory["worker_1"].target = "source1"
Memory["worker_1"].role = "harvester"
```

Реализация в Rust:

```rust
pub struct Memory {
    data: serde_json::Value,  // произвольный JSON
}

impl Memory {
    pub fn before_tick(&self, engine: &ScriptEngine) {
        // Сериализовать JSON → Lua-таблица Memory
        let table = engine.value_to_lua(&self.data)?;
        engine.lua.globals().set("Memory", table)?;
    }

    pub fn after_tick(&mut self, engine: &ScriptEngine) {
        // Сериализовать Lua-таблицу Memory → JSON
        let table: Table = engine.lua.globals().get("Memory")?;
        self.data = engine.lua_to_value(&table)?;
    }
}
```

**Несколько Lua-скриптов (роли):**

Вместо одного `harvester.lua` — отдельные скрипты для разных ролей:

```
scripts/
├── roles/
│   ├── harvester.lua     # добытчик
│   ├── upgrader.lua      # улучшатель контроллера
│   └── defender.lua      # защитник
└── main.lua              # распределяет роли по криповым ID
```

`main.lua` загружает `roles/*.lua` и назначает:

```lua
-- main.lua
function decide(ctx)
    local role = Memory.roles[ctx.id] or "harvester"
    if role == "harvester" then return harvester_decide(ctx) end
    if role == "defender" then return defender_decide(ctx) end
end
```

---

### 10.6 Резюме приоритетов улучшений

| Приоритет | Улучшение | Сложность | Ценность |
|-----------|-----------|-----------|----------|
| 1 | Разделение логики и рендеринга (trait Renderer) | Низкая | Высокая — основа для всего остального |
| 2 | Перезагрузка скриптов (file watcher) | Низкая | Высокая — критично для комфортной разработки AI |
| 3 | Загрузка карт из файлов | Низкая | Средняя — больше не нужно перекомпилировать |
| 4 | Система Memory | Средняя | Высокая — координация между крипами |
| 5 | A* Pathfinding | Средняя | Высокая — крипы перестанут застревать у стен |
| 6 | Процедурная генерация карт | Средняя | Средняя — разнообразие |
| 7 | Несколько скриптов / ролей | Средняя | Высокая — richer gameplay |
| 8 | Мультиплеер (WebSocket) | Высокая | Очень высокая — но требует п.1 |
