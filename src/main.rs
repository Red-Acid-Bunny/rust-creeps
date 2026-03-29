mod game;
mod render;
mod script;

use game::state::GameState;
use script::ScriptEngine;
use std::thread;
use std::time::Duration;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const MAP: &[&str] = &[
    "#############################",
    "#S#...#...#......~~~~~~..#EE#",
    "#.#.#.#.#.#.~~~~~~~~~~~..#EE#",
    "#.#.#.#.#.#.~~.~~~~~~~~..#..#",
    "#.#.#.#.#.#.~.........~c.#..#",
    "#.#.#.#.#.#.~~~~~~~~.~~..#..#",
    "#.#.#.#.#.#.~~~~~~~~.~~..#..#",
    "#...#...#...~~~~~~~~..~.....#",
    "#############################",
];

const TOTAL_TICKS: u64 = 4500;
const TICK_DELAY_MS: u64 = 30;

fn main() {
    // ── Logging ───────────────────────────────────────────
    // Архитектура: game logic (game/, script/) использует
    // только tracing-макросы — без привязки к конкретному бэкенду.
    // Бэкенд (file / console / будущий UI-layer) настраивается здесь.
    //
    // Чтобы добавить вывод в UI, достаточно создать свой tracing Layer
    // и подключить через .with(my_ui_layer) ниже.
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
        // .with(my_custom_ui_layer)  // ← будущий UI-слой
        .init();

    tracing::info!(
        ticks = TOTAL_TICKS,
        delay_ms = TICK_DELAY_MS,
        script = "scripts/harvester.lua",
        "CREEP-SIM started"
    );

    // ── Terminal banner ────────────────────────────────────
    println!();
    println!("╔══════════════════════════════════════╗");
    println!("║      CREEP-SIM: World Demo           ║");
    println!("╚══════════════════════════════════════╝");
    println!();
    println!("  Loading harvester.lua ...");
    println!("  Running {} ticks ({}ms each)", TOTAL_TICKS, TICK_DELAY_MS);
    println!("  Press Ctrl+C to stop");
    println!("  Log file: logs/rust-creeps.log");
    println!();
    thread::sleep(Duration::from_secs(2));

    // ── World setup ────────────────────────────────────────
    let mut game = GameState::from_map(MAP);
    game.view_range = 50;

    let engine = ScriptEngine::new().expect("Failed to create Lua VM");

    // Регистрируем Lua-функции, зависящие от состояния мира (find_path, get_tile)
    game
        .register_lua_functions(&engine)
        .expect("Failed to register world Lua functions");

    engine
        .load_script(std::path::Path::new("scripts/harvester.lua"))
        .expect("Failed to load harvester.lua");

    // ── Game loop ──────────────────────────────────────────
    for tick_num in 0..TOTAL_TICKS {
        game.tick(&engine);
        render::render(&game, &engine);
        thread::sleep(Duration::from_millis(TICK_DELAY_MS));

        if tick_num > 0 && tick_num % 500 == 0 {
            tracing::info!(tick = tick_num, "simulation progress");
        }
    }

    tracing::info!(total_ticks = game.tick, "simulation complete");

    // writer_guard dropped here → flushes remaining log entries
    drop(writer_guard);

    print!("\x1B[2J\x1B[H");
    println!();
    println!("══════════════════════════════════════");
    println!("  Simulation complete. {} ticks played.", game.tick);
    println!("  Log file: logs/rust-creeps.log");
    println!("  Run again: cargo run");
    println!("══════════════════════════════════════");
    println!();
}
