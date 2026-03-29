use mlua::{Lua, Result as LuaResult, Table, Value};
use serde::{Deserialize, Serialize};
use std::path::Path;

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
pub enum Action {
    Move { target: Position, reason: String },
    MoveTo { target: Position, reason: String },
    Harvest { target_id: String },
    Transfer { target_id: String, resource: String, amount: u32 },
    Spawn { target_id: String, body: Vec<String>, name: String },
    Idle { reason: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

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

/// Рекурсивно форматирует Lua-значение в компактную строку.
/// depth — глубина рекурсии (лимит 2 уровня для компактности).
fn format_lua_value(value: &mlua::Value, depth: usize) -> String {
    if depth > 2 {
        return "{...}".to_string();
    }
    match value {
        mlua::Value::Nil => "nil".to_string(),
        mlua::Value::Boolean(b) => b.to_string(),
        mlua::Value::Integer(n) => n.to_string(),
        mlua::Value::Number(n) => format!("{:.0}", n),
        mlua::Value::String(s) => s.to_string_lossy().to_string(),
        mlua::Value::Table(t) => {
            let mut parts: Vec<String> = Vec::new();
            for pair in t.pairs::<mlua::Value, mlua::Value>() {
                if let Ok((k, v)) = pair {
                    let key_str = match &k {
                        mlua::Value::String(s) => s.to_string_lossy().to_string(),
                        mlua::Value::Integer(i) => i.to_string(),
                        mlua::Value::Number(n) => format!("{:.0}", n),
                        _ => "?".to_string(),
                    };
                    let val_str = format_lua_value(&v, depth + 1);
                    parts.push(format!("{}={}", key_str, val_str));
                }
            }
            if parts.is_empty() {
                "{}".to_string()
            } else {
                format!("{{{}}}", parts.join(", "))
            }
        }
        mlua::Value::Function(_) => "[function]".to_string(),
        mlua::Value::UserData(_) | mlua::Value::LightUserData(_) => "[userdata]".to_string(),
        mlua::Value::Thread(_) => "[thread]".to_string(),
        mlua::Value::Error(e) => format!("[error: {}]", e),
        _ => "[?]".to_string(),
    }
}

pub struct ScriptEngine {
    lua: Lua,
}

impl ScriptEngine {
    pub fn new() -> LuaResult<Self> {
        tracing::debug!("creating Lua VM with sandbox");
        let lua = Lua::new();
        lua.load(r#"
            -- Block dangerous globals
            os, io, debug, require, dofile, loadfile, load, package = nil, nil, nil, nil, nil, nil, nil, nil

            -- Block dangerous string functions
            local _string = string
            local _safe_string = {
                byte = _string.byte,
                char = _string.char,
                find = _string.find,
                format = _string.format,
                gmatch = _string.gmatch,
                gsub = _string.gsub,
                len = _string.len,
                lower = _string.lower,
                match = _string.match,
                rep = _string.rep,
                reverse = _string.reverse,
                sub = _string.sub,
                upper = _string.upper,
            }
            string = setmetatable(_safe_string, {
                __index = function(_, key) return nil end,
                __newindex = function(_, key, value)
                    if _safe_string[key] ~= nil or key == "dump" then
                        -- silently block reassignment of dangerous functions
                        return
                    end
                    rawset(_safe_string, key, value)
                end,
            })

            -- Lock down metatables to prevent sandbox escape
            local _mt = getmetatable(_G) or {}
            _mt.__index = function(_, key) return nil end
            _mt.__newindex = function(_, key, value)
                if key == "string" or key == "table" or key == "math" or key == "coroutine" then
                    return -- prevent replacing safe libraries
                end
                rawset(_G, key, value)
            end
            setmetatable(_G, _mt)

            -- Prevent debug library access through metatable manipulation
            local _table = table
            table = setmetatable({}, {
                __index = function(_, key)
                    if key == "getmetatable" or key == "setmetatable" then
                        return nil -- block metatable access through table library
                    end
                    return _table[key]
                end,
            })

            -- Limit coroutine (can be used to escape sandbox)
            local _coroutine = coroutine
            coroutine = setmetatable({
                create = _coroutine.create,
                resume = _coroutine.resume,
                running = _coroutine.running,
                status = _coroutine.status,
                wrap = _coroutine.wrap,
                yield = _coroutine.yield,
            }, {
                __index = function(_, key) return nil end,
            })

            function distance(a, b) return math.abs(a.x - b.x) + math.abs(a.y - b.y) end
        "#)
        .exec()?;

        // Memory — глобальная персистентная таблица (как в Screeps).
        // Доступна всем крипам во всех тиках. Выживает при перезагрузке скрипта
        // (т.к. load_script не очищает глобалы).
        let memory = lua.create_table()?;
        lua.globals().set("Memory", memory)?;
        tracing::info!("Lua VM created, Memory initialized");
        Ok(Self { lua })
    }

    pub fn load_script(&self, path: &Path) -> LuaResult<()> {
        tracing::info!(path = %path.display(), "loading script");
        let code = std::fs::read_to_string(path)
            .map_err(|e| mlua::Error::external(format!("Cannot read {}: {}", path.display(), e)))?;
        self.lua.load(&code).exec().map_err(|e| {
            tracing::error!(path = %path.display(), error = %e, "script execution failed");
            e
        })?;
        tracing::info!(path = %path.display(), "script loaded successfully");
        Ok(())
    }

    pub fn load_script_from_str(&self, code: &str) -> LuaResult<()> {
        tracing::debug!("loading script from string");
        self.lua.load(code).exec()
    }

    pub fn call_decide(&self, context: &UnitContext) -> LuaResult<Action> {
        let ctx_table = self.context_to_lua(context)?;
        let decide_fn: mlua::Function = self.lua.globals().get("decide")?;
        let result: Table = decide_fn.call(ctx_table)?;
        self.parse_action(result)
    }

    /// Вызывает Lua-функцию before_tick(game), если она определена.
    /// Возвращает Some(action) если функция вернула экшен, иначе None.
    /// Если функция не определена — возвращает None (не ошибка).
    pub fn call_before_tick(&self, game: &GameContext) -> LuaResult<Option<Action>> {
        self.with_lua(|lua| {
            let val: mlua::Value = lua.globals().get("before_tick")?;
            match val {
                mlua::Value::Function(func) => {
                    let game_table = lua.create_table()?;
                    game_table.set("tick", game.tick)?;
                    game_table.set("creeps", Self::vec_nearby_to_lua_static(&lua, &game.creeps)?)?;
                    game_table.set("spawns", Self::vec_nearby_to_lua_static(&lua, &game.spawns)?)?;
                    game_table.set("sources", Self::vec_nearby_to_lua_static(&lua, &game.sources)?)?;
                    let result: Table = func.call(game_table)?;
                    Ok(Some(Self::parse_action_static(&lua, result)?))
                }
                _ => Ok(None),
            }
        })
    }

    /// Статическая версия vec_nearby_to_lua (для call_before_tick где нет &self).
    fn vec_nearby_to_lua_static(lua: &Lua, entities: &[NearbyEntity]) -> LuaResult<Table> {
        let table = lua.create_table()?;
        for (i, entity) in entities.iter().enumerate() {
            let pos = lua.create_table()?;
            pos.set("x", entity.pos.x)?;
            pos.set("y", entity.pos.y)?;
            let row = lua.create_table()?;
            row.set("id", entity.id.clone())?;
            row.set("pos", pos)?;
            row.set("resource_amount", entity.resource_amount)?;
            row.set("cooldown", entity.cooldown)?;
            table.set(i + 1, row)?;
        }
        Ok(table)
    }

    /// Статическая версия parse_action (для call_before_tick где нет &self).
    fn parse_action_static(_lua: &Lua, table: Table) -> LuaResult<Action> {
        let action_type: String = table
            .get("type")
            .map_err(|_| mlua::Error::external("Action missing 'type'"))?;
        match action_type.as_str() {
            "move" => {
                let target: Table = table.get("target")?;
                Ok(Action::Move {
                    target: Position { x: target.get("x")?, y: target.get("y")? },
                    reason: table.get("reason").unwrap_or_default(),
                })
            }
            "moveto" => {
                let target: Table = table.get("target")?;
                Ok(Action::MoveTo {
                    target: Position { x: target.get("x")?, y: target.get("y")? },
                    reason: table.get("reason").unwrap_or_default(),
                })
            }
            "harvest" => Ok(Action::Harvest { target_id: table.get("target_id")? }),
            "transfer" => Ok(Action::Transfer {
                target_id: table.get("target_id")?,
                resource: table.get("resource")?,
                amount: table.get("amount")?,
            }),
            "spawn" => {
                let body: Vec<String> = table.get("body")?;
                Ok(Action::Spawn {
                    target_id: table.get("target_id")?,
                    body,
                    name: table.get("name").unwrap_or_default(),
                })
            }
            "idle" => Ok(Action::Idle { reason: table.get("reason").unwrap_or_default() }),
            other => Err(mlua::Error::external(format!("Unknown action: '{}'", other))),
        }
    }

    pub fn global_is_nil(&self, name: &str) -> LuaResult<bool> {
        let val: Value = self.lua.globals().get(name)?;
        Ok(matches!(val, Value::Nil))
    }

    /// Предоставляет доступ к Lua-инстансу для регистрации глобальных функций.
    /// Замыкание может возвращать ошибку — она пробросится наверх.
    pub fn with_lua<F, R>(&self, f: F) -> LuaResult<R>
    where
        F: FnOnce(&Lua) -> LuaResult<R>,
    {
        f(&self.lua)
    }

    /// Читает число из Memory[key]. Для тестов.
    /// Возвращает None если Memory[key] не существует или не число.
    pub fn get_memory_number(&self, key: &str) -> LuaResult<Option<f64>> {
        self.with_lua(|lua| {
            let memory: mlua::Table = lua.globals().get("Memory")?;
            let val: mlua::Value = memory.get(key)?;
            match val {
                mlua::Value::Integer(n) => Ok(Some(n as f64)),
                mlua::Value::Number(n) => Ok(Some(n)),
                _ => Ok(None),
            }
        })
    }

    /// Читает строку из Memory[key]. Для тестов.
    pub fn get_memory_string(&self, key: &str) -> LuaResult<Option<String>> {
        self.with_lua(|lua| {
            let memory: mlua::Table = lua.globals().get("Memory")?;
            let val: Value = memory.get(key)?;
            match val {
                Value::String(s) => Ok(Some(s.to_string_lossy().to_string())),
                _ => Ok(None),
            }
        })
    }

    /// Возвращает форматированную строку с содержимым Memory для отображения в UI.
    /// Работает с любой структурой Memory — не привязан к конкретной схеме.
    pub fn format_memory(&self) -> LuaResult<String> {
        self.with_lua(|lua| {
            let memory: mlua::Table = lua.globals().get("Memory")?;
            let mut lines = Vec::new();
            for pair in memory.pairs::<mlua::Value, mlua::Value>() {
                if let Ok((k, v)) = pair {
                    let key_str = match &k {
                        mlua::Value::String(s) => s.to_string_lossy().to_string(),
                        mlua::Value::Integer(i) => i.to_string(),
                        mlua::Value::Number(n) => format!("{:.0}", n),
                        _ => continue,
                    };
                    let val_str = format_lua_value(&v, 1);
                    lines.push(format!("  {} = {}", key_str, val_str));
                }
            }
            if lines.is_empty() {
                Ok("  (empty)".to_string())
            } else {
                Ok(lines.join("\n"))
            }
        })
    }

    fn context_to_lua(&self, ctx: &UnitContext) -> LuaResult<Table> {
        let pos_table = self.lua.create_table()?;
        pos_table.set("x", ctx.pos.x)?;
        pos_table.set("y", ctx.pos.y)?;
        let ctx_table = self.lua.create_table()?;
        ctx_table.set("id", ctx.id.clone())?;
        ctx_table.set("pos", pos_table)?;
        ctx_table.set("hp", ctx.hp)?;
        ctx_table.set("max_hp", ctx.max_hp)?;
        ctx_table.set("energy", ctx.energy)?;
        ctx_table.set("carry_capacity", ctx.carry_capacity)?;
        ctx_table.set("carry", ctx.carry)?;
        ctx_table.set("tick", ctx.tick)?;
        ctx_table.set("nearby_sources", self.vec_nearby_to_lua(&ctx.nearby_sources)?)?;
        ctx_table.set("nearby_spawns", self.vec_nearby_to_lua(&ctx.nearby_spawns)?)?;
        ctx_table.set("nearby_creeps", self.vec_nearby_to_lua(&ctx.nearby_creeps)?)?;
        self.lua.globals().set("unit_context", ctx_table.clone())?;
        Ok(ctx_table)
    }

    fn vec_nearby_to_lua(&self, entities: &[NearbyEntity]) -> LuaResult<Table> {
        let table = self.lua.create_table()?;
        for (i, entity) in entities.iter().enumerate() {
            let pos = self.lua.create_table()?;
            pos.set("x", entity.pos.x)?;
            pos.set("y", entity.pos.y)?;
            let row = self.lua.create_table()?;
            row.set("id", entity.id.clone())?;
            row.set("pos", pos)?;
            row.set("resource_amount", entity.resource_amount)?;
            row.set("cooldown", entity.cooldown)?;
            table.set(i + 1, row)?;
        }
        Ok(table)
    }

    fn parse_action(&self, table: Table) -> LuaResult<Action> {
        let action_type: String = table
            .get("type")
            .map_err(|_| mlua::Error::external("Action missing 'type'"))?;
        match action_type.as_str() {
            "move" => {
                let target: Table = table.get("target")?;
                Ok(Action::Move {
                    target: Position {
                        x: target.get("x")?,
                        y: target.get("y")?,
                    },
                    reason: table.get("reason").unwrap_or_default(),
                })
            }
            "moveto" => {
                let target: Table = table.get("target")?;
                Ok(Action::MoveTo {
                    target: Position {
                        x: target.get("x")?,
                        y: target.get("y")?,
                    },
                    reason: table.get("reason").unwrap_or_default(),
                })
            }
            "harvest" => Ok(Action::Harvest {
                target_id: table.get("target_id")?,
            }),
            "transfer" => Ok(Action::Transfer {
                target_id: table.get("target_id")?,
                resource: table.get("resource")?,
                amount: table.get("amount")?,
            }),
            "spawn" => {
                let body: Vec<String> = table.get("body")?;
                Ok(Action::Spawn {
                    target_id: table.get("target_id")?,
                    body,
                    name: table.get("name").unwrap_or_default(),
                })
            }
            "idle" => Ok(Action::Idle {
                reason: table.get("reason").unwrap_or_default(),
            }),
            other => Err(mlua::Error::external(format!(
                "Unknown action: '{}'",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decide_with_nearby() {
        let engine = ScriptEngine::new().unwrap();
        engine
            .load_script_from_str(
                r#"
            function decide(ctx)
                if #ctx.nearby_sources > 0 then return { type = "harvest", target_id = ctx.nearby_sources[1].id } end
                return { type = "idle", reason = "no sources" }
            end
        "#,
            )
            .unwrap();
        let mut ctx = UnitContext::empty("c1", Position { x: 0, y: 0 });
        ctx.nearby_sources.push(NearbyEntity {
            id: "src1".into(),
            pos: Position { x: 3, y: 3 },
            resource_amount: 100,
            cooldown: 0,
        });
        assert!(matches!(
            engine.call_decide(&ctx).unwrap(),
            Action::Harvest { target_id } if target_id == "src1"
        ));
    }

    #[test]
    fn test_distance_builtin() {
        let engine = ScriptEngine::new().unwrap();
        engine
            .load_script_from_str(
                r#"
            function decide(ctx)
                local d = distance({x=0,y=0}, {x=3,y=4})
                if d == 7 then return { type = "idle", reason = "ok" } end
                return { type = "idle", reason = "wrong" }
            end
        "#,
            )
            .unwrap();
        assert!(matches!(
            engine
                .call_decide(&UnitContext::empty("c1", Position { x: 0, y: 0 }))
                .unwrap(),
            Action::Idle { .. }
        ));
    }

    #[test]
    fn test_moveto_action() {
        let engine = ScriptEngine::new().unwrap();
        engine
            .load_script_from_str(
                r#"
            function decide(ctx)
                return { type = "moveto", target = { x = 5, y = 5 }, reason = "going there" }
            end
        "#,
            )
            .unwrap();
        let action = engine
            .call_decide(&UnitContext::empty("c1", Position { x: 0, y: 0 }))
            .unwrap();
        assert!(matches!(
            action,
            Action::MoveTo { target, reason } if target.x == 5 && target.y == 5 && reason == "going there"
        ));
    }

    #[test]
    fn test_memory_persists_between_calls() {
        let engine = ScriptEngine::new().unwrap();
        engine
            .load_script_from_str(
                r#"
            function decide(ctx)
                Memory.counter = (Memory.counter or 0) + 1
                return { type = "idle", reason = "count=" .. Memory.counter }
            end
        "#,
            )
            .unwrap();
        let ctx = UnitContext::empty("c1", Position { x: 0, y: 0 });

        // Первый вызов
        let a1 = engine.call_decide(&ctx).unwrap();
        assert!(matches!(a1, Action::Idle { ref reason } if reason == "count=1"));

        // Второй вызов — Memory.counter должен быть 2
        let a2 = engine.call_decide(&ctx).unwrap();
        assert!(matches!(a2, Action::Idle { ref reason } if reason == "count=2"));

        // Третий вызов
        let a3 = engine.call_decide(&ctx).unwrap();
        assert!(matches!(a3, Action::Idle { ref reason } if reason == "count=3"));
    }

    #[test]
    fn test_memory_shared_across_creeps() {
        let engine = ScriptEngine::new().unwrap();
        engine
            .load_script_from_str(
                r#"
            function decide(ctx)
                Memory.creeps = Memory.creeps or {}
                Memory.creeps[ctx.id] = ctx.tick
                local n = 0
                for _ in pairs(Memory.creeps) do n = n + 1 end
                return { type = "idle", reason = "known=" .. n }
            end
        "#,
            )
            .unwrap();

        // Имитация двух крипов
        let ctx1 = UnitContext::empty("worker_1", Position { x: 0, y: 0 });
        let ctx2 = UnitContext::empty("worker_2", Position { x: 1, y: 0 });

        let a1 = engine.call_decide(&ctx1).unwrap();
        assert!(matches!(a1, Action::Idle { ref reason } if reason == "known=1"));

        // worker_2 видит Memory.creeps с worker_1
        let a2 = engine.call_decide(&ctx2).unwrap();
        assert!(matches!(a2, Action::Idle { ref reason } if reason == "known=2"));
    }

    #[test]
    fn test_memory_survives_script_reload() {
        let engine = ScriptEngine::new().unwrap();
        engine
            .load_script_from_str(
                r#"
            function decide(ctx)
                Memory.data = (Memory.data or 0) + 1
                return { type = "idle", reason = "data=" .. Memory.data }
            end
        "#,
            )
            .unwrap();

        let ctx = UnitContext::empty("c1", Position { x: 0, y: 0 });
        engine.call_decide(&ctx).unwrap(); // data=1
        engine.call_decide(&ctx).unwrap(); // data=2

        // Перезагрузка скрипта
        engine
            .load_script_from_str(
                r#"
            function decide(ctx)
                Memory.data = (Memory.data or 0) + 10
                return { type = "idle", reason = "data=" .. Memory.data }
            end
        "#,
            )
            .unwrap();

        // Memory.data должно быть 12 (2 + 10), а не 10
        let a = engine.call_decide(&ctx).unwrap();
        assert!(matches!(a, Action::Idle { ref reason } if reason == "data=12"));
    }
}
