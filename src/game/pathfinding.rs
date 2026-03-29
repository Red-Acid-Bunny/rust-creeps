use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::game::types::{Position, SWAMP_COST, TileType};

// ═══════════════════════════════════════
//  Pathfinding — A*
// ═══════════════════════════════════════

pub fn in_bounds(pos: Position, width: i32, height: i32) -> bool {
    pos.x >= 0 && pos.y >= 0 && pos.x < width && pos.y < height
}

pub fn tile_move_cost(tile: TileType) -> u32 {
    match tile {
        TileType::Plain => 1,
        TileType::Swamp => SWAMP_COST,
        TileType::Wall => u32::MAX, // непроходимо
    }
}

/// Проверяет, проходима ли позиция (учитывает тайлы и блокеры).
pub fn is_pos_walkable(
    tiles: &[Vec<TileType>],
    pos: Position,
    width: i32,
    height: i32,
    blockers: &[Position],
) -> bool {
    if !in_bounds(pos, width, height) {
        return false;
    }
    if tiles[pos.y as usize][pos.x as usize] == TileType::Wall {
        return false;
    }
    if blockers.contains(&pos) {
        return false;
    }
    true
}

/// Ищет ближайшую проходимую клетку, смежную с pos (distance = 1).
/// Возвращает None если все 4 соседа непроходимы.
pub fn find_adjacent_walkable(
    tiles: &[Vec<TileType>],
    pos: Position,
    width: i32,
    height: i32,
    blockers: &[Position],
) -> Option<Position> {
    let directions: [(i32, i32); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];
    for &(dx, dy) in &directions {
        let next = Position {
            x: pos.x + dx,
            y: pos.y + dy,
        };
        if is_pos_walkable(tiles, next, width, height, blockers) {
            return Some(next);
        }
    }
    None
}

/// A* поиск пути. Возвращает полный путь (включая from и to) или None.
///
/// - `avoid_swamp = true` → swamp treated as walls (no swamp in path)
/// - `avoid_swamp = false` → swamp allowed, costs SWAMP_MOVE_COST movement points
///
/// `blockers` — позиции непроходимых сущностей (sources, spawns).
pub fn astar(
    tiles: &[Vec<TileType>],
    width: i32,
    height: i32,
    blockers: &[Position],
    from: Position,
    to: Position,
    avoid_swamp: bool,
) -> Option<Vec<Position>> {
    if from == to {
        return Some(vec![from]);
    }

    if !in_bounds(from, width, height) || !in_bounds(to, width, height) {
        return None;
    }

    let is_blocked = |pos: Position| -> bool {
        if tiles[pos.y as usize][pos.x as usize] == TileType::Wall {
            return true;
        }
        if avoid_swamp && tiles[pos.y as usize][pos.x as usize] == TileType::Swamp {
            return true;
        }
        if blockers.contains(&pos) && pos != to {
            return true;
        }
        false
    };

    if is_blocked(from) || is_blocked(to) {
        return None;
    }

    #[derive(Clone, Copy, Eq, PartialEq)]
    struct Node {
        pos: Position,
        g: u32,
        f: u32,
    }

    impl Ord for Node {
        fn cmp(&self, other: &Self) -> Ordering {
            other.f.cmp(&self.f) // min-heap
        }
    }
    impl PartialOrd for Node {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    let heuristic = |a: Position, b: Position| -> u32 {
        (a.x - b.x).unsigned_abs() + (a.y - b.y).unsigned_abs()
    };

    let mut open = BinaryHeap::new();
    let mut g_score: HashMap<Position, u32> = HashMap::new();
    let mut came_from: HashMap<Position, Position> = HashMap::new();

    g_score.insert(from, 0);
    open.push(Node {
        pos: from,
        g: 0,
        f: heuristic(from, to),
    });

    let directions: [(i32, i32); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];

    while let Some(current) = open.pop() {
        if current.pos == to {
            // Восстановление пути
            let mut path = Vec::new();
            let mut p = to;
            while p != from {
                path.push(p);
                p = came_from[&p];
            }
            path.push(from);
            path.reverse();
            return Some(path);
        }

        // Пропускаем устаревшие записи
        if current.g > *g_score.get(&current.pos).unwrap_or(&u32::MAX) {
            continue;
        }

        for (dx, dy) in directions {
            let next = Position {
                x: current.pos.x + dx,
                y: current.pos.y + dy,
            };

            if !in_bounds(next, width, height) {
                continue;
            }
            if is_blocked(next) {
                continue;
            }

            let cost = tile_move_cost(tiles[next.y as usize][next.x as usize]);
            let tentative_g = current.g.saturating_add(cost);

            if tentative_g < *g_score.get(&next).unwrap_or(&u32::MAX) {
                g_score.insert(next, tentative_g);
                came_from.insert(next, current.pos);
                open.push(Node {
                    pos: next,
                    g: tentative_g,
                    f: tentative_g + heuristic(next, to),
                });
            }
        }
    }

    None
}
