//! The map of one act.
//!
//! A grid of `width` columns and `floors` rows. A handful of paths climb from the bottom row to the
//! top, each step going straight up or one column to the side, and no two steps cross. Every node on
//! the top row leads to the act's lord. This is the shape Slay the Spire made familiar; the code is
//! this mod's own.

use crate::rng::Rng;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// A pack of ordinary enemies, or a duel with a boss at the act's normal level.
    Fight,
    /// A boss above the act's level. Better rewards.
    Elite,
    /// A shrine: heal or scavenge.
    Rest,
    Shop,
    Event,
    /// A free blessing.
    Treasure,
    /// The act's last fight.
    Lord,
}

impl NodeKind {
    pub fn is_fight(self) -> bool {
        matches!(self, NodeKind::Fight | NodeKind::Elite | NodeKind::Lord)
    }

    pub fn label(self) -> &'static str {
        match self {
            NodeKind::Fight => "Fight",
            NodeKind::Elite => "Elite",
            NodeKind::Rest => "Shrine",
            NodeKind::Shop => "Merchant",
            NodeKind::Event => "Unknown",
            NodeKind::Treasure => "Treasure",
            NodeKind::Lord => "Lord",
        }
    }

    /// One character for the console map.
    pub fn glyph(self) -> char {
        match self {
            NodeKind::Fight => 'F',
            NodeKind::Elite => 'E',
            NodeKind::Rest => 'R',
            NodeKind::Shop => '$',
            NodeKind::Event => '?',
            NodeKind::Treasure => 'T',
            NodeKind::Lord => 'L',
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: usize,
    pub floor: usize,
    pub col: usize,
    pub kind: NodeKind,
    /// Ids of the nodes this one leads to, left to right.
    pub next: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActMap {
    pub width: usize,
    /// Rows below the lord.
    pub floors: usize,
    /// Every node; `nodes[i].id == i`. The lord is the last one.
    pub nodes: Vec<Node>,
    /// Ids of the bottom-row nodes, left to right.
    pub starts: Vec<usize>,
    pub lord: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapParams {
    pub width: usize,
    pub floors: usize,
    pub paths: usize,
    /// Elites and shrines appear from this floor up (0 is the bottom row).
    pub first_special_floor: usize,
    pub weight_fight: u32,
    pub weight_elite: u32,
    pub weight_rest: u32,
    pub weight_shop: u32,
    pub weight_event: u32,
}

impl MapParams {
    pub const MIN_FLOORS: usize = 5;
    pub const MAX_FLOORS: usize = 30;
    pub const MIN_WIDTH: usize = 3;
    pub const MAX_WIDTH: usize = 9;

    pub fn standard(floors: usize) -> Self {
        MapParams {
            width: 7,
            floors,
            paths: 6,
            first_special_floor: 3,
            weight_fight: 45,
            weight_elite: 16,
            weight_rest: 12,
            weight_shop: 6,
            weight_event: 21,
        }
    }

    pub fn check(&self) -> Result<(), String> {
        if !(Self::MIN_FLOORS..=Self::MAX_FLOORS).contains(&self.floors) {
            return Err(format!(
                "a map needs {} to {} floors, not {}",
                Self::MIN_FLOORS,
                Self::MAX_FLOORS,
                self.floors
            ));
        }
        if !(Self::MIN_WIDTH..=Self::MAX_WIDTH).contains(&self.width) {
            return Err(format!(
                "a map needs {} to {} columns, not {}",
                Self::MIN_WIDTH,
                Self::MAX_WIDTH,
                self.width
            ));
        }
        if self.paths < 2 || self.paths > 12 {
            return Err(format!("a map needs 2 to 12 paths, not {}", self.paths));
        }
        // Elites go on a floor from first_special_floor up, below the top row (all shrines) and off
        // the treasure floor.
        let top = self.floors - 1;
        let room = (self.first_special_floor..top).any(|f| f != self.treasure_floor());
        if self.first_special_floor == 0 || !room {
            return Err("first_special_floor leaves no room for elites".to_string());
        }
        if self.weight_fight == 0 {
            return Err("weight_fight must be above zero".to_string());
        }
        Ok(())
    }

    fn treasure_floor(&self) -> usize {
        self.floors / 2
    }
}

/// Kinds that may not follow themselves along a path.
fn no_repeat(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Elite | NodeKind::Rest | NodeKind::Shop)
}

impl ActMap {
    pub fn generate(rng: &mut Rng, params: &MapParams) -> Result<ActMap, String> {
        params.check()?;
        let (width, floors) = (params.width, params.floors);

        // edges[f][c] = the columns on floor f+1 that (f, c) leads to.
        let mut edges: Vec<Vec<Vec<usize>>> = vec![vec![Vec::new(); width]; floors - 1];
        let mut used = vec![vec![false; width]; floors];
        let mut first_start = None;

        for path in 0..params.paths {
            let mut col = rng.below(width as u32) as usize;
            if path == 1 {
                // The first two paths begin apart, so the first choice is always a real one.
                while Some(col) == first_start {
                    col = rng.below(width as u32) as usize;
                }
            }
            if path == 0 {
                first_start = Some(col);
            }
            used[0][col] = true;
            for floor in 0..floors - 1 {
                let step = rng.below(3) as i64 - 1;
                let mut next = (col as i64 + step).clamp(0, width as i64 - 1) as usize;
                if crosses(&edges[floor], col, next) {
                    // Going straight up never crosses anything.
                    next = col;
                }
                if !edges[floor][col].contains(&next) {
                    edges[floor][col].push(next);
                }
                used[floor + 1][next] = true;
                col = next;
            }
        }

        // Number the nodes row by row, left to right.
        let mut id_at = vec![vec![usize::MAX; width]; floors];
        let mut nodes = Vec::new();
        for floor in 0..floors {
            for col in 0..width {
                if used[floor][col] {
                    id_at[floor][col] = nodes.len();
                    nodes.push(Node {
                        id: nodes.len(),
                        floor,
                        col,
                        kind: NodeKind::Fight,
                        next: Vec::new(),
                    });
                }
            }
        }
        let lord = nodes.len();
        for floor in 0..floors {
            for col in 0..width {
                if !used[floor][col] {
                    continue;
                }
                let id = id_at[floor][col];
                if floor + 1 == floors {
                    nodes[id].next.push(lord);
                } else {
                    let mut targets = edges[floor][col].clone();
                    targets.sort_unstable();
                    nodes[id].next = targets.into_iter().map(|c| id_at[floor + 1][c]).collect();
                }
            }
        }
        nodes.push(Node {
            id: lord,
            floor: floors,
            col: width / 2,
            kind: NodeKind::Lord,
            next: Vec::new(),
        });
        let starts: Vec<usize> = (0..width)
            .filter(|c| used[0][*c])
            .map(|c| id_at[0][c])
            .collect();

        let mut map = ActMap {
            width,
            floors,
            nodes,
            starts,
            lord,
        };
        map.assign_kinds(rng, params);
        map.validate(params)?;
        Ok(map)
    }

    fn assign_kinds(&mut self, rng: &mut Rng, params: &MapParams) {
        let parents = self.parent_lists();
        let weights = [
            (NodeKind::Fight, params.weight_fight),
            (NodeKind::Elite, params.weight_elite),
            (NodeKind::Rest, params.weight_rest),
            (NodeKind::Shop, params.weight_shop),
            (NodeKind::Event, params.weight_event),
        ];
        let raw: Vec<u32> = weights.iter().map(|(_, w)| *w).collect();

        // Nodes are stored bottom row first, so a node's parents already have their kind.
        for id in 0..self.lord {
            let floor = self.nodes[id].floor;
            let kind = if floor == 0 {
                NodeKind::Fight
            } else if floor == params.treasure_floor() {
                NodeKind::Treasure
            } else if floor + 1 == self.floors {
                NodeKind::Rest
            } else {
                let mut chosen = NodeKind::Fight;
                for _ in 0..24 {
                    let Some(i) = rng.weighted(&raw) else { break };
                    let candidate = weights[i].0;
                    if self.kind_allowed(candidate, id, &parents, params) {
                        chosen = candidate;
                        break;
                    }
                }
                chosen
            };
            self.nodes[id].kind = kind;
        }

        // Every act has at least one elite and one merchant.
        for wanted in [NodeKind::Elite, NodeKind::Shop] {
            if self.nodes.iter().any(|n| n.kind == wanted) {
                continue;
            }
            // Turn a plain fight into the missing kind; failing that an event, then a shrine.
            for replace in [NodeKind::Fight, NodeKind::Event, NodeKind::Rest] {
                let mut candidates: Vec<usize> = (0..self.lord)
                    .filter(|id| {
                        let node = &self.nodes[*id];
                        node.kind == replace
                            && node.floor != 0
                            && node.floor + 1 != self.floors
                            && self.kind_allowed(wanted, *id, &parents, params)
                            && self.children_allow(wanted, *id)
                    })
                    .collect();
                rng.shuffle(&mut candidates);
                if let Some(id) = candidates.first() {
                    self.nodes[*id].kind = wanted;
                    break;
                }
            }
        }
    }

    fn kind_allowed(
        &self,
        kind: NodeKind,
        id: usize,
        parents: &[Vec<usize>],
        params: &MapParams,
    ) -> bool {
        let floor = self.nodes[id].floor;
        if matches!(kind, NodeKind::Elite | NodeKind::Rest) && floor < params.first_special_floor {
            return false;
        }
        // The top row is all shrines; a shrine right below it would be wasted.
        if kind == NodeKind::Rest && floor + 2 == self.floors {
            return false;
        }
        if no_repeat(kind) && parents[id].iter().any(|p| self.nodes[*p].kind == kind) {
            return false;
        }
        true
    }

    fn children_allow(&self, kind: NodeKind, id: usize) -> bool {
        !no_repeat(kind)
            || self.nodes[id]
                .next
                .iter()
                .all(|c| self.nodes[*c].kind != kind)
    }

    /// For each node, the ids of the nodes that lead to it.
    pub fn parent_lists(&self) -> Vec<Vec<usize>> {
        let mut parents = vec![Vec::new(); self.nodes.len()];
        for node in &self.nodes {
            for next in &node.next {
                parents[*next].push(node.id);
            }
        }
        parents
    }

    pub fn node(&self, id: usize) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// Where the player may go from `at` (`None` is before the first step of the act).
    pub fn options_from(&self, at: Option<usize>) -> Vec<usize> {
        match at {
            None => self.starts.clone(),
            Some(id) => self.node(id).map(|n| n.next.clone()).unwrap_or_default(),
        }
    }

    /// Checks every rule a generated map must keep. Also run on maps loaded from a save.
    pub fn validate(&self, params: &MapParams) -> Result<(), String> {
        if self.nodes.is_empty() || self.lord + 1 != self.nodes.len() {
            return Err("the lord must be the last node".to_string());
        }
        if self.floors != params.floors || self.width != params.width {
            return Err("map size does not match its parameters".to_string());
        }
        for (i, node) in self.nodes.iter().enumerate() {
            if node.id != i {
                return Err(format!("node {i} carries id {}", node.id));
            }
        }
        if self.starts.len() < 2 {
            return Err("a map needs at least two starting nodes".to_string());
        }
        for start in &self.starts {
            match self.node(*start) {
                Some(n) if n.floor == 0 => {}
                _ => return Err(format!("start {start} is not on the bottom row")),
            }
        }
        let lord = &self.nodes[self.lord];
        if lord.kind != NodeKind::Lord || !lord.next.is_empty() || lord.floor != self.floors {
            return Err("the lord node is malformed".to_string());
        }
        let parents = self.parent_lists();
        let mut seen_cells = std::collections::HashSet::new();
        for node in &self.nodes[..self.lord] {
            if node.floor >= self.floors || node.col >= self.width {
                return Err(format!("node {} is off the grid", node.id));
            }
            if !seen_cells.insert((node.floor, node.col)) {
                return Err(format!(
                    "two nodes share floor {} column {}",
                    node.floor, node.col
                ));
            }
            if node.kind == NodeKind::Lord {
                return Err(format!("node {} is a second lord", node.id));
            }
            if node.next.is_empty() {
                return Err(format!("node {} is a dead end", node.id));
            }
            if node.floor == 0 {
                if !self.starts.contains(&node.id) {
                    return Err(format!("bottom-row node {} is not a start", node.id));
                }
            } else if parents[node.id].is_empty() {
                return Err(format!("node {} cannot be reached", node.id));
            }
            for next in &node.next {
                let Some(target) = self.node(*next) else {
                    return Err(format!("node {} leads to a missing node", node.id));
                };
                if node.floor + 1 == self.floors {
                    if *next != self.lord || node.next.len() != 1 {
                        return Err(format!(
                            "top-row node {} must lead only to the lord",
                            node.id
                        ));
                    }
                } else {
                    if target.floor != node.floor + 1 {
                        return Err(format!("node {} skips a floor", node.id));
                    }
                    if target.col.abs_diff(node.col) > 1 {
                        return Err(format!("node {} jumps more than one column", node.id));
                    }
                }
            }
            // Kind rules.
            let expected_fixed = if node.floor == 0 {
                Some(NodeKind::Fight)
            } else if node.floor == params.treasure_floor() {
                Some(NodeKind::Treasure)
            } else if node.floor + 1 == self.floors {
                Some(NodeKind::Rest)
            } else {
                None
            };
            match expected_fixed {
                Some(kind) if node.kind != kind => {
                    return Err(format!(
                        "node {} on floor {} should be {:?}, is {:?}",
                        node.id, node.floor, kind, node.kind
                    ));
                }
                Some(_) => {}
                None => {
                    if node.kind == NodeKind::Treasure {
                        return Err(format!(
                            "node {} is a treasure off the treasure floor",
                            node.id
                        ));
                    }
                    if !self.kind_allowed(node.kind, node.id, &parents, params) {
                        return Err(format!(
                            "node {} breaks a placement rule as {:?}",
                            node.id, node.kind
                        ));
                    }
                }
            }
        }
        // No two steps between the same pair of floors may cross.
        for floor in 0..self.floors.saturating_sub(1) {
            let steps: Vec<(usize, usize)> = self.nodes[..self.lord]
                .iter()
                .filter(|n| n.floor == floor)
                .flat_map(|n| n.next.iter().map(move |t| (n.col, self.nodes[*t].col)))
                .collect();
            for (i, a) in steps.iter().enumerate() {
                for b in &steps[i + 1..] {
                    if (a.0 < b.0 && a.1 > b.1) || (a.0 > b.0 && a.1 < b.1) {
                        return Err(format!("steps cross above floor {floor}"));
                    }
                }
            }
        }
        Ok(())
    }

    /// The map as text for a fixed-width console, top row first. `at` is marked with brackets and
    /// `visited` nodes are shown in lower case.
    pub fn render(&self, at: Option<usize>, visited: &[usize]) -> Vec<String> {
        let cell = 4;
        let line_width = self.width * cell;
        let mut out = Vec::new();
        let lord_mark = if at == Some(self.lord) { "[L]" } else { " L " };
        out.push(center(lord_mark, line_width));
        for floor in (0..self.floors).rev() {
            let mut row = vec![' '; line_width];
            let mut links = vec![' '; line_width];
            for node in self.nodes[..self.lord].iter().filter(|n| n.floor == floor) {
                let x = node.col * cell + 1;
                let mut glyph = node.kind.glyph();
                if visited.contains(&node.id) {
                    glyph = glyph.to_ascii_lowercase();
                }
                if at == Some(node.id) {
                    row[x - 1] = '[';
                    row[x + 1] = ']';
                }
                row[x] = glyph;
                if floor + 1 < self.floors {
                    for next in &node.next {
                        let target = self.nodes[*next].col;
                        if target < node.col {
                            links[x - 2] = '\\';
                        } else if target > node.col {
                            links[x + 2] = '/';
                        } else {
                            links[x] = '|';
                        }
                    }
                }
            }
            if floor + 1 < self.floors {
                out.push(links.iter().collect::<String>().trim_end().to_string());
            }
            out.push(row.iter().collect::<String>().trim_end().to_string());
        }
        out
    }
}

fn center(text: &str, width: usize) -> String {
    let len = text.chars().count();
    let pad = width.saturating_sub(len) / 2;
    format!("{}{}", " ".repeat(pad), text)
}

/// Would a step from `col` to `next` cross a step already drawn on this floor?
fn crosses(floor_edges: &[Vec<usize>], col: usize, next: usize) -> bool {
    floor_edges.iter().enumerate().any(|(from, targets)| {
        targets
            .iter()
            .any(|to| (from < col && *to > next) || (from > col && *to < next))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_of_maps_keep_every_rule() {
        for floors in [5, 8, 10, 15] {
            let params = MapParams::standard(floors);
            for seed in 0..1500u64 {
                let mut rng = Rng::stream(seed, "map", floors as u64);
                let map = ActMap::generate(&mut rng, &params)
                    .unwrap_or_else(|e| panic!("seed {seed}, {floors} floors: {e}"));
                // generate() validates; check again from the outside for good measure.
                map.validate(&params).unwrap();
                assert!(
                    map.nodes.iter().any(|n| n.kind == NodeKind::Elite),
                    "seed {seed}: no elite"
                );
                assert!(
                    map.nodes.iter().any(|n| n.kind == NodeKind::Shop),
                    "seed {seed}: no merchant"
                );
            }
        }
    }

    #[test]
    fn same_seed_same_map() {
        let params = MapParams::standard(10);
        let a = ActMap::generate(&mut Rng::stream(5, "map", 0), &params).unwrap();
        let b = ActMap::generate(&mut Rng::stream(5, "map", 0), &params).unwrap();
        let c = ActMap::generate(&mut Rng::stream(6, "map", 0), &params).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn every_path_reaches_the_lord_in_exactly_floors_steps() {
        let params = MapParams::standard(10);
        let map = ActMap::generate(&mut Rng::stream(123, "map", 0), &params).unwrap();
        fn walk(map: &ActMap, id: usize, depth: usize) {
            if id == map.lord {
                assert_eq!(depth, map.floors);
                return;
            }
            for next in &map.nodes[id].next {
                walk(map, *next, depth + 1);
            }
        }
        for start in &map.starts {
            walk(&map, *start, 0);
        }
    }

    #[test]
    fn options_follow_the_edges() {
        let params = MapParams::standard(8);
        let map = ActMap::generate(&mut Rng::stream(9, "map", 0), &params).unwrap();
        assert_eq!(map.options_from(None), map.starts);
        let first = map.starts[0];
        assert_eq!(map.options_from(Some(first)), map.nodes[first].next);
        assert!(map.options_from(Some(map.lord)).is_empty());
        assert!(map.options_from(Some(9999)).is_empty());
    }

    #[test]
    fn bad_parameters_are_refused() {
        let mut p = MapParams::standard(10);
        p.floors = 2;
        assert!(ActMap::generate(&mut Rng::new(1), &p).is_err());
        let mut p = MapParams::standard(10);
        p.width = 1;
        assert!(ActMap::generate(&mut Rng::new(1), &p).is_err());
        let mut p = MapParams::standard(10);
        p.paths = 1;
        assert!(ActMap::generate(&mut Rng::new(1), &p).is_err());
        let mut p = MapParams::standard(10);
        p.weight_fight = 0;
        assert!(ActMap::generate(&mut Rng::new(1), &p).is_err());
    }

    #[test]
    fn validate_catches_tampering() {
        let params = MapParams::standard(10);
        let good = ActMap::generate(&mut Rng::stream(77, "map", 0), &params).unwrap();

        let mut dead_end = good.clone();
        dead_end.nodes[0].next.clear();
        assert!(dead_end.validate(&params).is_err());

        let mut wrong_kind = good.clone();
        wrong_kind.nodes[0].kind = NodeKind::Elite;
        assert!(wrong_kind.validate(&params).is_err());

        let mut bad_link = good.clone();
        bad_link.nodes[0].next = vec![good.lord];
        assert!(bad_link.validate(&params).is_err());

        let mut renumbered = good.clone();
        renumbered.nodes[1].id = 0;
        assert!(renumbered.validate(&params).is_err());
    }

    #[test]
    fn render_marks_position_and_fits_the_grid() {
        let params = MapParams::standard(10);
        let map = ActMap::generate(&mut Rng::stream(4, "map", 0), &params).unwrap();
        let start = map.starts[0];
        let lines = map.render(Some(start), &[start]);
        assert_eq!(lines.len(), 1 + params.floors * 2 - 1);
        assert!(lines.last().unwrap().contains("[f]"));
        assert!(lines.iter().all(|l| l.chars().count() <= params.width * 4));
    }

    #[test]
    fn map_survives_a_save() {
        let params = MapParams::standard(10);
        let map = ActMap::generate(&mut Rng::stream(31, "map", 2), &params).unwrap();
        let text = serde_json::to_string(&map).unwrap();
        let back: ActMap = serde_json::from_str(&text).unwrap();
        assert_eq!(map, back);
    }
}
