mod lua_api;
mod world;

use lua_api::ScriptEngine;
use std::thread;
use std::time::Duration;
use world::World;

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

const TOTAL_TICKS: u64 = 4500;
const TICK_DELAY_MS: u64 = 300;

fn main() {
    println!();
    println!("╔══════════════════════════════════════╗");
    println!("║      CREEP-SIM: World Demo           ║");
    println!("╚══════════════════════════════════════╝");
    println!();
    println!("  Loading harvester.lua ...");
    println!("  Running {} ticks ({}ms each)", TOTAL_TICKS, TICK_DELAY_MS);
    println!("  Press Ctrl+C to stop");
    println!();
    thread::sleep(Duration::from_secs(2));

    let mut world = World::from_map(MAP);
    world.view_range = 20;

    let engine = ScriptEngine::new().expect("Failed to create Lua VM");
    engine
        .load_script(std::path::Path::new("scripts/harvester.lua"))
        .expect("Failed to load harvester.lua");

    for _ in 0..TOTAL_TICKS {
        world.tick(&engine);
        world.render();
        thread::sleep(Duration::from_millis(TICK_DELAY_MS));
    }

    print!("\x1B[2J\x1B[H");
    println!();
    println!("══════════════════════════════════════");
    println!("  Simulation complete. {} ticks played.", world.tick);
    println!("  Run again: cargo run");
    println!("══════════════════════════════════════");
    println!();
}
