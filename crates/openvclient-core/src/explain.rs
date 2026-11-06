//! Query plans in a common tree shape, built from each engine's native
//! EXPLAIN output.

use crate::ResultSet;

#[derive(Clone, Debug, Default)]
pub struct PlanNode {
    /// Operation, e.g. `Seq Scan on users`.
    pub title: String,
    pub startup_cost: Option<f64>,
    pub total_cost: Option<f64>,
    pub plan_rows: Option<f64>,
    pub actual_rows: Option<f64>,
    /// Inclusive time spent in this node, milliseconds.
    pub actual_ms: Option<f64>,
    pub loops: Option<f64>,
    /// Remaining engine-specific properties.
    pub props: Vec<(String, String)>,
    pub children: Vec<PlanNode>,
}

impl PlanNode {
    pub fn max_total_cost(&self) -> f64 {
        self.children.iter().map(|c| c.max_total_cost()).fold(self.total_cost.unwrap_or(0.0), f64::max)
    }
    pub fn max_actual_ms(&self) -> f64 {
        self.children.iter().map(|c| c.max_actual_ms()).fold(self.actual_ms.unwrap_or(0.0), f64::max)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ExplainResult {
    /// The exact statement sent to the engine.
    pub statement: String,
    /// Raw engine output (JSON or text).
    pub raw: String,
    pub tree: Option<PlanNode>,
    /// Tabular plans (MySQL classic EXPLAIN).
    pub table: Option<ResultSet>,
    /// Planning/execution totals reported by the engine.
    pub summary: Vec<(String, String)>,
}

/// Parse PostgreSQL `EXPLAIN (FORMAT JSON)` output.
pub fn parse_pg_json_plan(raw: &str) -> Option<(PlanNode, Vec<(String, String)>)> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let top = v.as_array()?.first()?;
    let plan = top.get("Plan")?;
    let mut summary = Vec::new();
    for key in ["Planning Time", "Execution Time"] {
        if let Some(t) = top.get(key).and_then(|x| x.as_f64()) {
            summary.push((key.to_string(), format!("{t:.3} ms")));
        }
    }
    Some((pg_node(plan), summary))
}

fn pg_node(p: &serde_json::Value) -> PlanNode {
    let s = |k: &str| p.get(k).and_then(|x| x.as_str()).map(str::to_owned);
    let f = |k: &str| p.get(k).and_then(|x| x.as_f64());
    let mut title = s("Node Type").unwrap_or_else(|| "?".into());
    if let Some(rel) = s("Relation Name") {
        title.push_str(" on ");
        title.push_str(&rel);
        if let Some(alias) = s("Alias").filter(|a| a != &rel) {
            title.push(' ');
            title.push_str(&alias);
        }
    } else if let Some(idx) = s("Index Name") {
        title.push_str(" using ");
        title.push_str(&idx);
    }
    let loops = f("Actual Loops");
    let actual_ms = f("Actual Total Time").map(|t| t * loops.unwrap_or(1.0));
    let mut props = Vec::new();
    const SKIP: &[&str] = &[
        "Node Type", "Plans", "Startup Cost", "Total Cost", "Plan Rows", "Actual Rows", "Actual Total Time",
        "Actual Loops", "Relation Name", "Alias", "Parallel Aware", "Async Capable",
    ];
    if let Some(obj) = p.as_object() {
        for (k, v) in obj {
            if SKIP.contains(&k.as_str()) {
                continue;
            }
            let text = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Array(a) => {
                    a.iter().map(|x| x.as_str().map(str::to_owned).unwrap_or_else(|| x.to_string())).collect::<Vec<_>>().join(", ")
                }
                other => other.to_string(),
            };
            props.push((k.clone(), text));
        }
    }
    let children = p
        .get("Plans")
        .and_then(|x| x.as_array())
        .map(|a| a.iter().map(pg_node).collect())
        .unwrap_or_default();
    PlanNode {
        title,
        startup_cost: f("Startup Cost"),
        total_cost: f("Total Cost"),
        plan_rows: f("Plan Rows"),
        actual_rows: f("Actual Rows"),
        actual_ms,
        loops,
        props,
        children,
    }
}

/// Build a tree from SQLite `EXPLAIN QUERY PLAN` rows `(id, parent, detail)`.
pub fn sqlite_plan_tree(rows: &[(i64, i64, String)]) -> PlanNode {
    fn build(parent: i64, rows: &[(i64, i64, String)]) -> Vec<PlanNode> {
        rows.iter()
            .filter(|r| r.1 == parent)
            .map(|r| PlanNode { title: r.2.clone(), children: build(r.0, rows), ..Default::default() })
            .collect()
    }
    PlanNode { title: "QUERY PLAN".into(), children: build(0, rows), ..Default::default() }
}

/// Build a tree from MySQL `EXPLAIN FORMAT=TREE` / `EXPLAIN ANALYZE` text,
/// where nesting is expressed by `->` indentation.
pub fn mysql_tree_text(raw: &str) -> PlanNode {
    let mut root = PlanNode { title: "Query plan".into(), ..Default::default() };
    // Stack of (indent, path of child indexes from root).
    let mut stack: Vec<(usize, Vec<usize>)> = Vec::new();
    for line in raw.lines() {
        let Some(pos) = line.find("-> ") else { continue };
        let indent = pos;
        let text = line[pos + 3..].trim().to_string();
        let mut node = PlanNode::default();
        // Split "(cost=… rows=…) (actual time=a..b rows=… loops=…)".
        let (title, rest) = match text.find(" (") {
            Some(p) => (text[..p].to_string(), text[p..].to_string()),
            None => (text.clone(), String::new()),
        };
        node.title = title;
        if let Some(c) = grab(&rest, "cost=") {
            node.total_cost = c.parse().ok();
        }
        if let Some(r) = grab(&rest, "rows=") {
            node.plan_rows = r.parse().ok();
        }
        if let Some(t) = grab(&rest, "actual time=") {
            node.actual_ms = t.split("..").nth(1).and_then(|x| x.parse().ok());
        }
        if !rest.is_empty() {
            node.props.push(("Estimates".into(), rest.trim().to_string()));
        }
        while stack.last().is_some_and(|(i, _)| *i >= indent) {
            stack.pop();
        }
        let parent_path = stack.last().map(|(_, p)| p.clone()).unwrap_or_default();
        let parent = node_at(&mut root, &parent_path);
        parent.children.push(node);
        let mut path = parent_path;
        path.push(parent.children.len() - 1);
        stack.push((indent, path));
    }
    root
}

fn node_at<'a>(root: &'a mut PlanNode, path: &[usize]) -> &'a mut PlanNode {
    let mut n = root;
    for &i in path {
        n = &mut n.children[i];
    }
    n
}

fn grab<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    let p = s.find(key)? + key.len();
    let rest = &s[p..];
    let end = rest.find([' ', ')']).unwrap_or(rest.len());
    Some(&rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_tree() {
        let raw = "-> Nested loop inner join  (cost=2.5 rows=3)\n    -> Table scan on a  (cost=0.5 rows=3)\n    -> Single-row index lookup on b using PRIMARY (id=a.id)  (cost=0.3 rows=1)\n";
        let t = mysql_tree_text(raw);
        assert_eq!(t.children.len(), 1);
        assert_eq!(t.children[0].children.len(), 2);
        assert_eq!(t.children[0].total_cost, Some(2.5));
    }

    #[test]
    fn pg_json() {
        let raw = r#"[{"Plan":{"Node Type":"Seq Scan","Relation Name":"t","Alias":"t","Startup Cost":0.0,"Total Cost":35.5,"Plan Rows":2550,"Plan Width":4},"Planning Time":0.05}]"#;
        let (n, s) = parse_pg_json_plan(raw).unwrap();
        assert_eq!(n.title, "Seq Scan on t");
        assert_eq!(n.total_cost, Some(35.5));
        assert_eq!(s.len(), 1);
    }
}
