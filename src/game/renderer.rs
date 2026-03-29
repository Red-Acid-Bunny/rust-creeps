use crate::game::config::GameConfig;
use crate::game::state::GameState;
use crate::script::ScriptEngine;

/// Abstraction over rendering backends.
/// Game logic calls this — implementations can be terminal, TUI, GUI, headless, or network.
pub trait Renderer {
    /// Called once before the game loop starts.
    fn init(&mut self, _config: &GameConfig) {}

    /// Called every tick after game state is updated.
    /// Receives immutable references to game state and script engine.
    fn render_tick(&mut self, state: &GameState, engine: &ScriptEngine);

    /// Called once after the game loop ends.
    fn shutdown(&mut self) {}
}

/// Headless renderer — does nothing. For tests, servers, CI.
#[allow(dead_code)]
pub struct HeadlessRenderer;

impl Renderer for HeadlessRenderer {
    fn render_tick(&mut self, _state: &GameState, _engine: &ScriptEngine) {
        // intentionally empty
    }
}
