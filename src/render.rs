use crate::game::config::GameConfig;
use crate::game::renderer::Renderer;
use crate::game::state::GameState;
use crate::game::types::*;
use crate::script::ScriptEngine;

/// Terminal-based renderer using ANSI escape codes and print!()
pub struct CliRenderer;

impl Renderer for CliRenderer {
    fn init(&mut self, _config: &GameConfig) {
        println!();
        println!("╔══════════════════════════════════════╗");
        println!("║      CREEP-SIM: World Demo           ║");
        println!("╚══════════════════════════════════════╝");
        println!();
    }

    fn render_tick(&mut self, world: &GameState, engine: &ScriptEngine) {
        print!("\x1B[2J\x1B[H");
        let sep = "─".repeat(world.width + 4);
        println!("┌{}┐", sep);
        println!("│ CREEP-SIM  Tick: {:>4} {:>20} │", world.tick, "");
        println!("├{}┤", sep);
        for y in 0..world.height {
            print!("│ ");
            for x in 0..world.width {
                let pos = Position {
                    x: x as i32,
                    y: y as i32,
                };
                let ch = world
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
                    .unwrap_or_else(|| match world.tiles[y][x] {
                        TileType::Plain => '.',
                        TileType::Wall => '#',
                        TileType::Swamp => '~',
                    });
                print!("{}", ch);
            }
            println!(" │");
        }
        println!("└{}┘", sep);
        // ── Entity panel ──
        // Each creep — exactly one line.
        // SPAWN is not shown as "creep action" — it's a global event.
        let mut creep_lines: Vec<String> = Vec::new();
        let mut source_lines: Vec<String> = Vec::new();
        let mut spawn_entity_lines: Vec<String> = Vec::new();

        for e in &world.entities {
            match e.entity_type {
                EntityType::Creep => {
                    let action_str = match &e.last_action {
                        Some(Action::Move { target, reason }) => {
                            format!("MOVE ({},{}) [{}]", target.x, target.y, reason)
                        }
                        Some(Action::MoveTo { target, reason }) => {
                            let pos = e.pos;
                            let tgt = *target;
                            if pos == tgt {
                                format!("ARRIVED ({},{})", tgt.x, tgt.y)
                            } else {
                                format!("MOVETO ({},{}) [{}]", tgt.x, tgt.y, reason)
                            }
                        }
                        Some(Action::Harvest { target_id }) => {
                            format!("HARVEST {}", target_id)
                        }
                        Some(Action::Transfer { target_id, amount, .. }) => {
                            format!("TRANSFER {} -> {}", amount, target_id)
                        }
                        // Spawn as "creep action" not shown —
                        // it's a before_tick event, not a creep command.
                        // Instead the creep will show as "NEW" in a separate block.
                        Some(Action::Spawn { name, .. }) => {
                            format!("NEW (spawned as {})", name)
                        }
                        Some(Action::Idle { reason }) => {
                            format!("IDLE [{}]", reason)
                        }
                        None => "—".to_string(),
                    };
                    let carry_mark = if e.carry > 0 && e.carry_capacity > 0 { 'C' } else { 'c' };
                    creep_lines.push(format!(
                        "  {} {} ({},{})  hp:{}/{}  carry:{}/{}  {}",
                        carry_mark, e.id, e.pos.x, e.pos.y,
                        e.hp, e.max_hp, e.carry, e.carry_capacity, action_str
                    ));
                }
                EntityType::Source => {
                    source_lines.push(format!(
                        "  [SOURCE] {}  ({},{})  energy: {}",
                        e.id, e.pos.x, e.pos.y, e.resource_amount
                    ));
                }
                EntityType::Spawn => {
                    let cd_str = if e.spawn_cooldown > 0 {
                        format!("  cd:{}", e.spawn_cooldown)
                    } else {
                        "  ready".to_string()
                    };
                    spawn_entity_lines.push(format!(
                        "  [SPAWN]  {}  ({},{})  stored: {}{}",
                        e.id, e.pos.x, e.pos.y, e.energy, cd_str
                    ));
                }
            }
        }

        for line in &spawn_entity_lines { println!("{}", line); }
        for line in &source_lines { println!("{}", line); }
        for line in &creep_lines { println!("{}", line); }
        println!();
        println!("  Legend: # wall | . plain | S spawn | E source | c/C creep");
        println!();
        println!("  MEMORY:");
        match engine.format_memory() {
            Ok(s) => println!("{}", s),
            Err(e) => println!("  (error: {})", e),
        }
        println!();
    }

    fn shutdown(&mut self) {
        print!("\x1B[2J\x1B[H");
        println!();
        println!("══════════════════════════════════════");
        println!("  Simulation complete.");
        println!("══════════════════════════════════════");
        println!();
    }
}
