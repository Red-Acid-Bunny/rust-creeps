mod game;
mod render;
mod script;

use game::config::GameConfig;
use game::renderer::Renderer;
use game::state::GameState;
use render::CliRenderer;
use script::ScriptEngine;
use std::thread;
use std::time::Duration;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() {
    // ── Config ─────────────────────────────────────────────
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "maps/default.json".to_string());

    let config = GameConfig::from_file(std::path::Path::new(&config_path)).unwrap_or_else(|e| {
        eprintln!("Failed to load config from {}: {}", config_path, e);
        std::process::exit(1);
    });

    // ── Logging ───────────────────────────────────────────
    // Architecture: game logic (game/, script/) uses
    // only tracing macros — no backend dependency.
    // Backend (file / console / future UI-layer) configured here.
    //
    // To add UI output, create a tracing Layer
    // and connect via .with(my_ui_layer) below.
    let log_dir = std::path::Path::new("logs");
    std::fs::create_dir_all(log_dir).expect("Failed to create logs directory");

    let file_appender = tracing_appender::rolling::never(log_dir, "rust-creeps.log");
    let (non_blocking, writer_guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_target(false),
        )
        // .with(my_custom_ui_layer)  // <- future UI layer
        .init();

    tracing::info!(
        map = %config_path,
        ticks = config.total_ticks,
        delay_ms = config.tick_delay_ms,
        script = %config.script_path,
        "CREEP-SIM started"
    );

    // ── Terminal banner ────────────────────────────────────
    // ── Renderer init ─────────────────────────────────────
    let mut renderer = CliRenderer;
    renderer.init(&config);

    println!("  Loading {} ...", config.script_path);
    println!("  Running {} ticks ({}ms each)", config.total_ticks, config.tick_delay_ms);
    println!("  Press Ctrl+C to stop");
    println!("  Log file: logs/rust-creeps.log");
    println!();
    thread::sleep(Duration::from_secs(2));

    // ── World setup ────────────────────────────────────────
    let mut game = GameState::from_config(&config);

    let engine = ScriptEngine::new().expect("Failed to create Lua VM");

    // Register Lua functions that depend on world state (find_path, get_tile)
    game.register_lua_functions(&engine)
        .expect("Failed to register world Lua functions");

    engine
        .load_script(std::path::Path::new(&config.script_path))
        .expect("Failed to load Lua script");

    // ── Game loop ──────────────────────────────────────────
    for tick_num in 0..config.total_ticks {
        game.tick(&engine);
        renderer.render_tick(&game, &engine);
        thread::sleep(Duration::from_millis(config.tick_delay_ms));

        if tick_num > 0 && tick_num % 500 == 0 {
            tracing::info!(tick = tick_num, "simulation progress");
        }
    }

    tracing::info!(total_ticks = game.tick, "simulation complete");

    // writer_guard dropped here -> flushes remaining log entries
    drop(writer_guard);

    renderer.shutdown();
}
