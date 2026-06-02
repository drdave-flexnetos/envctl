//! Graph intelligence over the component dependency DAG.
//!
//! Every component declares `requires` edges; this module reasons about the whole
//! graph: structural summary (roots/leaves/orphans, critical path, fan-in/out),
//! per-component IMPACT (install pulls in the closure; reset --cascade removes the
//! reverse-dependents), dependency PATHS ("why is X needed"), and exports to
//! Graphviz DOT + JSON — optionally annotated with live detect/drift state from an
//! `EnvReport`. Pure + read-only.
use crate::model::{EnvReport, Registry, Severity};
use std::collections::{BTreeMap, HashMap};

/// Structural summary of the dependency graph.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphReport {
    pub nodes: usize,
    pub edges: usize,
    /// Components with no `requires` (foundations).
    pub roots: Vec<String>,
    /// Components nothing requires (top-level installs).
    pub leaves: Vec<String>,
    /// Neither depended on nor depending (standalone).
    pub orphans: Vec<String>,
    /// The longest requires-chain (critical path), root → leaf.
    pub critical_path: Vec<String>,
    /// (component, count) with the most direct dependents.
    pub max_dependents: Option<(String, usize)>,
    /// (component, count) with the most direct prerequisites.
    pub max_requires: Option<(String, usize)>,
    /// Meta/group components (`group-*` / `meta-*`).
    pub groups: Vec<String>,
}

/// Per-component impact (blast radius).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Impact {
    pub component: String,
    /// What `install <id>` pulls in (id + transitive prerequisites), in order.
    pub install_closure: Vec<String>,
    /// What `reset <id> --cascade` would also remove (transitive dependents).
    pub cascade_removes: Vec<String>,
    /// Direct prerequisites.
    pub requires: Vec<String>,
    /// Direct dependents.
    pub required_by: Vec<String>,
}

/// Direct reverse edges: id → components that directly require it.
fn reverse_edges(reg: &Registry) -> BTreeMap<String, Vec<String>> {
    let mut rev: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in reg.ordered() {
        for dep in &c.requires {
            rev.entry(dep.clone()).or_default().push(c.id.clone());
        }
    }
    rev
}

pub fn analyze(reg: &Registry) -> GraphReport {
    let rev = reverse_edges(reg);
    let ids: Vec<String> = reg.ids().cloned().collect();
    let edges: usize = reg.ordered().map(|c| c.requires.len()).sum();

    let roots: Vec<String> = reg.ordered().filter(|c| c.requires.is_empty()).map(|c| c.id.clone()).collect();
    let leaves: Vec<String> = ids.iter().filter(|id| !rev.contains_key(*id)).cloned().collect();
    let orphans: Vec<String> = reg
        .ordered()
        .filter(|c| c.requires.is_empty() && !rev.contains_key(&c.id))
        .map(|c| c.id.clone())
        .collect();
    let groups: Vec<String> = ids.iter().filter(|id| id.starts_with("group-") || id.starts_with("meta-")).cloned().collect();

    // Longest requires-chain via DP over the topo order (reg.ids() is topo-sorted).
    let mut dist: HashMap<&str, usize> = HashMap::new();
    let mut pred: HashMap<&str, Option<&str>> = HashMap::new();
    let mut best = ("", 0usize);
    for id in reg.ids() {
        let comp = reg.get(id).unwrap();
        let (mut d, mut p) = (1usize, None);
        for dep in &comp.requires {
            let dd = dist.get(dep.as_str()).copied().unwrap_or(1) + 1;
            if dd > d {
                d = dd;
                p = Some(dep.as_str());
            }
        }
        dist.insert(id.as_str(), d);
        pred.insert(id.as_str(), p);
        if d > best.1 {
            best = (id.as_str(), d);
        }
    }
    let mut critical_path = Vec::new();
    let mut cur = if best.1 > 0 { Some(best.0) } else { None };
    while let Some(n) = cur {
        critical_path.push(n.to_string());
        cur = pred.get(n).copied().flatten();
    }
    critical_path.reverse();

    let max_dependents = rev.iter().max_by_key(|(_, v)| v.len()).map(|(k, v)| (k.clone(), v.len()));
    let max_requires = reg
        .ordered()
        .filter(|c| !c.requires.is_empty())
        .max_by_key(|c| c.requires.len())
        .map(|c| (c.id.clone(), c.requires.len()));

    GraphReport {
        nodes: reg.len(),
        edges,
        roots,
        leaves,
        orphans,
        critical_path,
        max_dependents,
        max_requires,
        groups,
    }
}

pub fn impact(reg: &Registry, id: &str) -> Option<Impact> {
    let comp = reg.get(id)?;
    let install_closure = reg.closure(id).ok()?.iter().map(|c| c.id.clone()).collect();
    let cascade_removes = reg.reverse_dependents(id).iter().map(|c| c.id.clone()).collect();
    let required_by = reverse_edges(reg).get(id).cloned().unwrap_or_default();
    Some(Impact {
        component: id.into(),
        install_closure,
        cascade_removes,
        requires: comp.requires.clone(),
        required_by,
    })
}

/// All root→id dependency paths ("why is `id` needed / how is it reached"). Each
/// path follows `requires` from a root to `id`.
pub fn dependency_paths(reg: &Registry, id: &str) -> Vec<Vec<String>> {
    if reg.get(id).is_none() {
        return vec![];
    }
    // walk requires recursively, collecting paths root..id.
    // AUDIT FIX (#15): the cycle guard must be per-path (the current ancestor
    // chain in `acc`), not a persistent global edge-set. A global `seen` set
    // skips a shared lower sub-DAG on the second arrival from a different upper
    // branch, silently dropping valid paths (e.g. a diamond yields 2 of 4
    // paths). Guarding against `acc.contains(r)` prevents only true cycles
    // while the DAG guarantee bounds the number of distinct paths.
    fn walk(reg: &Registry, id: &str, acc: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
        acc.push(id.to_string());
        let comp = reg.get(id);
        let reqs = comp.map(|c| c.requires.clone()).unwrap_or_default();
        if reqs.is_empty() {
            let mut p = acc.clone();
            p.reverse();
            out.push(p);
        } else {
            for r in reqs {
                if !acc.contains(&r) {
                    walk(reg, &r, acc, out);
                }
            }
        }
        acc.pop();
    }
    let mut out = Vec::new();
    walk(reg, id, &mut Vec::new(), &mut out);
    out
}

// ── exports ──────────────────────────────────────────────────────────────────

fn state_for<'a>(report: Option<&'a EnvReport>, id: &str) -> Option<&'a crate::model::ComponentState> {
    report.and_then(|r| r.components.iter().find(|c| c.id == id))
}

/// Graphviz DOT. With a report, nodes are colored by state
/// (green=healthy, amber=detected-only, grey=absent) and a red border on drift.
pub fn to_dot(reg: &Registry, report: Option<&EnvReport>) -> String {
    let sev_color = |s: &crate::model::ComponentState| {
        if s.detected && s.healthy == Some(true) {
            "#2ea043" // green
        } else if s.detected {
            "#d29922" // amber
        } else {
            "#8b949e" // grey
        }
    };
    let drift_sev: HashMap<&str, Severity> = report
        .map(|r| r.drift.iter().map(|d| (d.component.as_str(), d.severity)).collect())
        .unwrap_or_default();

    let mut s = String::from("digraph envctl {\n  rankdir=LR;\n  node [shape=box, style=\"rounded,filled\", fontname=\"monospace\", fillcolor=\"#161b22\", color=\"#30363d\", fontcolor=\"#c9d1d9\"];\n  edge [color=\"#484f58\"];\n  bgcolor=\"#0d1117\";\n");
    for c in reg.ordered() {
        let mut attrs = vec![format!("label=\"{}\"", c.id)];
        if let Some(st) = state_for(report, &c.id) {
            attrs.push(format!("color=\"{}\"", sev_color(st)));
            if matches!(drift_sev.get(c.id.as_str()), Some(Severity::High)) {
                attrs.push("penwidth=2".into());
                attrs.push("color=\"#f85149\"".into());
            }
        }
        if c.id.starts_with("group-") || c.id.starts_with("meta-") {
            attrs.push("shape=octagon".into());
        }
        s.push_str(&format!("  \"{}\" [{}];\n", c.id, attrs.join(", ")));
    }
    for c in reg.ordered() {
        for dep in &c.requires {
            s.push_str(&format!("  \"{}\" -> \"{}\";\n", dep, c.id));
        }
    }
    s.push_str("}\n");
    s
}

/// JSON nodes/edges (optionally annotated with state).
pub fn to_json(reg: &Registry, report: Option<&EnvReport>) -> serde_json::Value {
    let nodes: Vec<serde_json::Value> = reg
        .ordered()
        .map(|c| {
            let st = state_for(report, &c.id);
            serde_json::json!({
                "id": c.id,
                "name": c.name,
                "requires": c.requires,
                "group": c.id.starts_with("group-") || c.id.starts_with("meta-"),
                "detected": st.map(|s| s.detected),
                "healthy": st.and_then(|s| s.healthy),
            })
        })
        .collect();
    let edges: Vec<serde_json::Value> = reg
        .ordered()
        .flat_map(|c| c.requires.iter().map(move |d| serde_json::json!({"from": d, "to": c.id})))
        .collect();
    serde_json::json!({ "nodes": nodes, "edges": edges, "summary": analyze(reg) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    fn reg() -> Registry {
        Registry::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../manifest")).unwrap()
    }
    #[test]
    fn analyze_finds_roots_and_critical_path() {
        let g = analyze(&reg());
        assert!(g.nodes > 0 && g.edges > 0);
        assert!(g.roots.contains(&"rustup".to_string()), "rustup is a root: {:?}", g.roots);
        // critical path is a real chain (each step requires the previous)
        assert!(g.critical_path.len() >= 2);
    }
    #[test]
    fn impact_closure_and_cascade() {
        let im = impact(&reg(), "bun").unwrap();
        assert!(im.install_closure.contains(&"bun".to_string()));
        assert!(im.cascade_removes.contains(&"node-via-bun".to_string()));
    }
    #[test]
    fn dot_and_json_export() {
        let r = reg();
        assert!(to_dot(&r, None).starts_with("digraph"));
        assert!(to_json(&r, None)["nodes"].as_array().unwrap().len() > 0);
    }
}
