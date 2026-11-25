//! Display waves and lifted edges for a parent's subtasks: the run-order map, the Waves list
//! and the plan cards.
//!
//! `core`'s `run_order` leaves done subtasks out (rule 18); the map shows them in their wave,
//! so the display waves are worked out here from the view model's snapshot: Kahn layering over
//! the parent's direct subtasks that aren't dropped or proposed, done ones included, with a
//! descendant's dependency lifted to its top-level subtask under the parent.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use airboss_core::{RunOrder, TaskId};

use crate::dto::{OutsideDto, RunEdgeDto, RunMap, RunNodeDto, StatusDto, WavesDto};
use crate::viewmodel::{ViewModel, is_open};

/// Above this many subtasks the page opens on Waves instead of the map.
pub const MAP_MAX_NODES: usize = 40;
/// Above this many waves the page opens on Waves instead of the map.
pub const MAP_MAX_WAVES: usize = 8;

/// A parent's subtasks in display waves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DisplayWaves {
    /// The stations, in sibling order.
    pub nodes: Vec<TaskId>,
    pub waves: Vec<Vec<TaskId>>,
    /// Lifted edges, blocker → waiting task.
    pub edges: Vec<RunEdgeDto>,
    /// Stations held by open tasks outside the parent, directly or through their subtree.
    pub outside: Vec<OutsideDto>,
    /// Stations Kahn layering can't place: they wait on each other in a loop.
    pub tangled: Vec<TaskId>,
}

impl DisplayWaves {
    /// For the panel's Waves list.
    pub fn to_dto(&self) -> WavesDto {
        WavesDto {
            waves: self.waves.clone(),
            outside: self.outside.clone(),
            tangled: self.tangled.clone(),
        }
    }
}

/// Works out the display waves of `parent`'s direct subtasks from the view model alone.
pub fn display_waves(vm: &ViewModel, parent: TaskId) -> DisplayWaves {
    let status = |id: &TaskId| vm.rows.get(id).map(|r| r.status);
    let nodes: Vec<TaskId> = vm
        .index
        .children_of(parent)
        .iter()
        .copied()
        .filter(|id| {
            !matches!(
                status(id),
                Some(StatusDto::Dropped | StatusDto::Proposed) | None
            )
        })
        .collect();
    let rank: HashMap<TaskId, usize> = nodes.iter().enumerate().map(|(i, &id)| (id, i)).collect();

    // Which station each task under the parent belongs to; and the whole subtree.
    let mut top: HashMap<TaskId, TaskId> = HashMap::new();
    for &node in &nodes {
        for id in vm.index.subtree(node) {
            top.insert(id, node);
        }
    }
    let inside: HashSet<TaskId> = vm.index.subtree(parent).into_iter().collect();
    let open = |id: &TaskId| vm.node(*id).is_some_and(|n| is_open(n.task.status));

    // Lifted edges (waiting station → blocker station), done only if every blocker behind it is.
    let mut lifted: BTreeMap<(usize, usize), bool> = BTreeMap::new();
    let mut outside: BTreeMap<usize, BTreeSet<TaskId>> = BTreeMap::new();
    for &(waiting, waits_on) in &vm.snapshot.edges {
        let Some(&wn) = top.get(&waiting) else {
            continue;
        };
        match top.get(&waits_on) {
            Some(&bn) if bn != wn => {
                let done = lifted.entry((rank[&wn], rank[&bn])).or_insert(true);
                *done &= !open(&waits_on);
            }
            Some(_) => {}
            None if !inside.contains(&waits_on) && open(&waits_on) => {
                outside.entry(rank[&wn]).or_default().insert(waits_on);
            }
            None => {}
        }
    }

    // Kahn layering: each placed station's wave is one past its latest inside blocker's.
    let mut blockers: Vec<HashSet<usize>> = vec![HashSet::new(); nodes.len()];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for &(w, b) in lifted.keys() {
        if blockers[w].insert(b) {
            dependents[b].push(w);
        }
    }
    let mut remaining: Vec<usize> = blockers.iter().map(HashSet::len).collect();
    let mut wave_of: Vec<Option<usize>> = vec![None; nodes.len()];
    let mut topo: Vec<usize> = Vec::new();
    let mut ready: Vec<usize> = (0..nodes.len()).filter(|&i| remaining[i] == 0).collect();
    while let Some(i) = ready.pop() {
        topo.push(i);
        wave_of[i] = Some(
            blockers[i]
                .iter()
                .filter_map(|&b| wave_of[b])
                .map(|w| w + 1)
                .max()
                .unwrap_or(0),
        );
        for &d in &dependents[i] {
            remaining[d] = remaining[d].saturating_sub(1);
            if remaining[d] == 0 {
                ready.push(d);
            }
        }
    }
    // A station held by an open task outside the parent goes in the last wave, as the design
    // draws it (DESIGN's Refunds example: "Backfill old refunds" in wave 4), and anything
    // waiting on it follows it. One pass in topological order.
    let last = wave_of.iter().flatten().copied().max().unwrap_or(0);
    for &i in &topo {
        let Some(mut w) = wave_of[i] else { continue };
        if outside.contains_key(&i) {
            w = w.max(last);
        }
        if let Some(after) = blockers[i]
            .iter()
            .filter_map(|&b| wave_of[b])
            .map(|bw| bw + 1)
            .max()
        {
            w = w.max(after);
        }
        wave_of[i] = Some(w);
    }
    let count = wave_of.iter().flatten().max().map_or(0, |m| m + 1);
    let mut waves: Vec<Vec<TaskId>> = vec![Vec::new(); count];
    for (i, w) in wave_of.iter().enumerate() {
        if let Some(w) = w {
            waves[*w].push(nodes[i]);
        }
    }
    let tangled = (0..nodes.len())
        .filter(|&i| wave_of[i].is_none())
        .map(|i| nodes[i])
        .collect();

    let mut edges: Vec<RunEdgeDto> = lifted
        .iter()
        .map(|(&(w, b), &done)| RunEdgeDto {
            from: nodes[b],
            to: nodes[w],
            done,
        })
        .collect();
    edges.sort_by_key(|e| (rank[&e.from], rank[&e.to]));

    DisplayWaves {
        waves,
        edges,
        outside: outside
            .into_iter()
            .map(|(i, blockers)| OutsideDto {
                task: nodes[i],
                blockers: blockers.into_iter().collect(),
            })
            .collect(),
        tangled,
        nodes,
    }
}

/// The run-order map, with outside blockers from `core`'s `run_order` (rule 18), which also
/// counts a blocker reached through a sibling.
pub fn run_map(vm: &ViewModel, parent: TaskId, order: &RunOrder, req: u32) -> RunMap {
    let dw = display_waves(vm, parent);
    let stations: HashSet<TaskId> = dw.nodes.iter().copied().collect();
    let nodes = dw
        .waves
        .iter()
        .enumerate()
        .flat_map(|(w, wave)| {
            wave.iter().enumerate().map(move |(i, &id)| RunNodeDto {
                id,
                wave: u32::try_from(w).unwrap_or(u32::MAX),
                index: u32::try_from(i).unwrap_or(u32::MAX),
            })
        })
        .collect();
    let outside = order
        .waiting_outside
        .iter()
        .filter(|o| stations.contains(&o.task.id))
        .map(|o| OutsideDto {
            task: o.task.id,
            blockers: o.blockers.clone(),
        })
        .collect();
    RunMap {
        req,
        parent,
        fits: map_fits(dw.nodes.len(), dw.waves.len()),
        nodes,
        edges: dw.edges,
        waves: dw.waves,
        outside,
        tangled: dw.tangled,
    }
}

/// Whether the map is drawn, or the page falls back to the Waves list.
pub fn map_fits(nodes: usize, waves: usize) -> bool {
    nodes <= MAP_MAX_NODES && waves <= MAP_MAX_WAVES
}
