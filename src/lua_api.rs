use mlua::{Lua, Result as LuaResult, Table, Value};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    Move { target: Position, reason: String },
    Harvest { target_id: String },
    Transfer { target_id: String, resource: String, amount: u32 },
    Idle { reason: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Position { pub x: i32, pub y: i32 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NearbyEntity {
    pub id: String, pub pos: Position, pub resource_amount: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitContext {
    pub id: String, pub pos: Position, pub hp: u32, pub max_hp: u32,
    pub energy: u32, pub carry_capacity: u32, pub carry: u32, pub tick: u64,
    pub nearby_sources: Vec<NearbyEntity>,
    pub nearby_spawns: Vec<NearbyEntity>,
    pub nearby_creeps: Vec<NearbyEntity>,
}

impl UnitContext {
    pub fn empty(id: &str, pos: Position) -> Self {
        UnitContext {
            id: id.to_string(), pos, hp: 100, max_hp: 100, energy: 0,
            carry_capacity: 50, carry: 0, tick: 0,
            nearby_sources: vec![], nearby_spawns: vec![], nearby_creeps: vec![],
        }
    }
}

pub struct ScriptEngine { lua: Lua }

impl ScriptEngine {
    pub fn new() -> LuaResult<Self> {
        tracing::debug!("creating Lua VM with sandbox");
        let lua = Lua::new();
        lua.load(r#"
            function distance(a, b) return math.abs(a.x - b.x) + math.abs(a.y - b.y) end
            os, io, debug, require, dofile, loadfile, load, package = nil, nil, nil, nil, nil, nil, nil, nil
            local _mt = getmetatable(_G) or {}
            _mt.__index = function(_, key) return nil end
            setmetatable(_G, _mt)
        "#).exec()?;
        tracing::info!("Lua VM created");
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

    pub fn global_is_nil(&self, name: &str) -> LuaResult<bool> {
        let val: Value = self.lua.globals().get(name)?;
        Ok(matches!(val, Value::Nil))
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
            table.set(i + 1, row)?;
        }
        Ok(table)
    }

    fn parse_action(&self, table: Table) -> LuaResult<Action> {
        let action_type: String = table.get("type")
            .map_err(|_| mlua::Error::external("Action missing 'type'"))?;
        match action_type.as_str() {
            "move" => {
                let target: Table = table.get("target")?;
                Ok(Action::Move { target: Position { x: target.get("x")?, y: target.get("y")? },
                    reason: table.get("reason").unwrap_or_default() })
            }
            "harvest" => Ok(Action::Harvest { target_id: table.get("target_id")? }),
            "transfer" => Ok(Action::Transfer { target_id: table.get("target_id")?,
                resource: table.get("resource")?, amount: table.get("amount")? }),
            "idle" => Ok(Action::Idle { reason: table.get("reason").unwrap_or_default() }),
            other => Err(mlua::Error::external(format!("Unknown action: '{}'", other))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_decide_with_nearby() {
        let engine = ScriptEngine::new().unwrap();
        engine.load_script_from_str(r#"
            function decide(ctx)
                if #ctx.nearby_sources > 0 then return { type = "harvest", target_id = ctx.nearby_sources[1].id } end
                return { type = "idle", reason = "no sources" }
            end
        "#).unwrap();
        let mut ctx = UnitContext::empty("c1", Position { x: 0, y: 0 });
        ctx.nearby_sources.push(NearbyEntity { id: "src1".into(), pos: Position { x: 3, y: 3 }, resource_amount: 100 });
        assert!(matches!(engine.call_decide(&ctx).unwrap(), Action::Harvest { target_id } if target_id == "src1"));
    }
    #[test]
    fn test_distance_builtin() {
        let engine = ScriptEngine::new().unwrap();
        engine.load_script_from_str(r#"
            function decide(ctx)
                local d = distance({x=0,y=0}, {x=3,y=4})
                if d == 7 then return { type = "idle", reason = "ok" } end
                return { type = "idle", reason = "wrong" }
            end
        "#).unwrap();
        assert!(matches!(engine.call_decide(&UnitContext::empty("c1", Position { x: 0, y: 0 })).unwrap(), Action::Idle { .. }));
    }
}
