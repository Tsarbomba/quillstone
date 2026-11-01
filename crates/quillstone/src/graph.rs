//! `quillstone graph`: the story's structure as a flowchart (Mermaid or Graphviz dot). Built from the
//! compiled ops, not by playing: the edges are the ones the writer wrote.

use std::collections::HashSet;

use quillstone_script::{Expr, HudAction, Op, Program, ONCE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Mermaid,
    Dot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    /// Running on into the next heading, or the condition of an `::if` that skips ahead.
    Flow,
    Jump,
    Call,
    Choice,
    /// The inventory's Use button, or a HUD button.
    Use,
}

/// The made-up node that item uses ("Inventory") or HUD buttons ("HUD") hang off.
fn is_hub(n: &Node, name: &str) -> bool {
    n.pseudo && n.text == name
}

struct Node {
    text: String,
    /// The included file the heading is in; `None` for the main script.
    file: Option<String>,
    /// The label contains `::end` (or the script's own end).
    ends: bool,
    unreachable: bool,
    /// Not a heading: the start before the first heading, or the inventory.
    pseudo: bool,
}

struct Graph {
    nodes: Vec<Node>,
    edges: Vec<(usize, usize, Kind, String)>,
}

const EDGE_TEXT: usize = 32;

/// Collapsed to one line and cut to `EDGE_TEXT` characters.
fn shorten(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= EDGE_TEXT {
        return one;
    }
    one.chars().take(EDGE_TEXT - 1).collect::<String>().trim_end().to_string() + "\u{2026}"
}

/// `if a and b`, or nothing when there is no condition.
fn when(conds: &[String]) -> String {
    if conds.is_empty() { String::new() } else { format!("if {}", conds.join(" and ")) }
}

/// A condition's text; the hidden variable of a `{once}` option reads as `once`.
fn cond_text(c: &Expr) -> String {
    match c {
        Expr::Not(v) if matches!(&**v, Expr::Var(n) if n.starts_with(ONCE)) => "once".into(),
        _ => c.to_string(),
    }
}

fn build(program: &Program) -> Graph {
    let hs = &program.headings;
    let at = |k: usize| program.labels[&hs[k].label];
    // A start node holds the ops before the first heading (all of them, in a script with no headings).
    let off = usize::from(hs.first().is_none_or(|_| at(0) > 0));
    let mut nodes: Vec<Node> = Vec::new();
    if off == 1 {
        nodes.push(Node { text: "(start)".into(), file: None, ends: false, unreachable: false, pseudo: true });
    }
    for h in hs {
        let file = program.locate(h.line).0.map(str::to_string);
        nodes.push(Node { text: h.text.clone(), file, ends: false, unreachable: false, pseudo: false });
    }
    // The node a heading's ops belong to, and the node a jump to op `t` lands in (the first label there).
    let owner = |i: usize| (0..hs.len()).rev().find(|&k| at(k) <= i).map_or(0, |k| k + off);
    let target = |t: usize| (0..hs.len()).find(|&k| at(k) == t).map(|k| k + off);
    let by_label = |l: &str| hs.iter().position(|h| h.label == l).map(|k| k + off);

    let mut edges: Vec<(usize, usize, Kind, String)> = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |from: usize, to: usize, kind: Kind, label: String| {
        if seen.insert((from, to, kind, label.clone())) {
            edges.push((from, to, kind, label));
        }
    };
    let terminates = |op: &Op| matches!(op, Op::Jump { cond: None, .. } | Op::Return | Op::End | Op::Choice { .. });

    for k in 0..hs.len() {
        // Falls in from above: the label before it, unless that ends in a jump, return or choice.
        let prev = if k == 0 { (off == 1).then_some(0) } else { Some(k - 1 + off) };
        let before = at(k).checked_sub(1).map(|i| &program.ops[i]);
        if let Some(prev) = prev.filter(|_| before.is_none_or(|op| !terminates(op)) || k > 0 && at(k - 1) == at(k)) {
            add(prev, k + off, Kind::Flow, String::new());
        }
    }
    let label_starts: HashSet<usize> = (0..hs.len()).map(at).collect();
    let mut jump_targets: HashSet<usize> = HashSet::new();
    for op in &program.ops {
        match op {
            Op::Jump { target, .. } => drop(jump_targets.insert(*target)),
            Op::Choice { options, .. } => jump_targets.extend(options.iter().map(|o| o.target)),
            _ => {}
        }
    }
    // The open `::if` branches: (condition, op index where the branch ends).
    let mut guards: Vec<(String, usize)> = Vec::new();
    for (i, op) in program.ops.iter().enumerate() {
        guards.retain(|(_, end)| *end > i);
        let from = owner(i);
        if matches!(op, Op::End) {
            nodes[from].ends = true;
        }
        // Ops after a jump that nothing leads to can't run.
        let dead = i > 0 && terminates(&program.ops[i - 1]) && !label_starts.contains(&i) && !jump_targets.contains(&i);
        if dead {
            continue;
        }
        let g: Vec<String> = guards.iter().map(|(c, _)| c.clone()).collect();
        let with = |own: Option<String>| when(&g.iter().cloned().chain(own).collect::<Vec<_>>());
        match op {
            // `::if c` skips past its branch when `c` is false; inside it, `c` holds.
            Op::Jump { target: t, cond: Some(Expr::Not(c)) } if program.block_jumps.contains(&i) => {
                if let Some(to) = target(*t) {
                    add(from, to, Kind::Flow, with(Some(format!("not {c}"))));
                }
                guards.push((c.to_string(), *t));
            }
            // The jump over an `::else`: the fall-through edges already say it.
            Op::Jump { .. } if program.block_jumps.contains(&i) => {}
            Op::Jump { target: t, cond } => {
                if let Some(to) = target(*t) {
                    add(from, to, Kind::Jump, with(cond.as_ref().map(cond_text)));
                }
            }
            Op::Call { target: t, .. } => {
                if let Some(to) = target(*t) {
                    add(from, to, Kind::Call, with(None));
                }
            }
            Op::Choice { options, .. } => {
                for o in options {
                    let Some(to) = target(o.target) else { continue };
                    let mut text = shorten(&o.text);
                    if o.default {
                        text += " (default)";
                    }
                    let cond = when(&g.iter().cloned().chain(o.cond.as_ref().map(cond_text)).collect::<Vec<_>>());
                    add(from, to, Kind::Choice, if cond.is_empty() { text } else { format!("{text} [{cond}]") });
                }
            }
            _ => {}
        }
    }
    if program.items.values().any(|it| it.use_label.is_some()) {
        let bag = nodes.len();
        nodes.push(Node { text: "Inventory".into(), file: None, ends: false, unreachable: false, pseudo: true });
        for it in program.items.values() {
            if let Some(to) = it.use_label.as_deref().and_then(by_label) {
                add(bag, to, Kind::Use, shorten(&it.name));
            }
        }
    }
    // Each HUD button that calls a label hangs off one "HUD" node, like item uses off Inventory.
    let hud_calls: Vec<(&str, &str)> = program
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::Hud { elem, .. } => match &elem.action {
                Some(HudAction::Call(l)) => Some((elem.id.as_str(), l.as_str())),
                _ => None,
            },
            _ => None,
        })
        .collect();
    if !hud_calls.is_empty() {
        let hud = nodes.len();
        nodes.push(Node { text: "HUD".into(), file: None, ends: false, unreachable: false, pseudo: true });
        for (id, label) in hud_calls {
            if let Some(to) = by_label(label) {
                add(hud, to, Kind::Use, id.to_string());
            }
        }
    }
    // Static reachability from where the story starts, the inventory, HUD buttons and the replay scenes.
    let mut live = vec![false; nodes.len()];
    let mut todo: Vec<usize> = vec![owner(0)];
    todo.extend(nodes.iter().position(|n| is_hub(n, "Inventory")));
    todo.extend(nodes.iter().position(|n| is_hub(n, "HUD")));
    todo.extend(program.replays.iter().filter_map(|r| by_label(&r.label)));
    while let Some(n) = todo.pop() {
        if !std::mem::replace(&mut live[n], true) {
            todo.extend(edges.iter().filter(|e| e.0 == n).map(|e| e.1));
        }
    }
    for (n, node) in nodes.iter_mut().enumerate() {
        node.unreachable = !live[n];
    }
    Graph { nodes, edges }
}

/// Mermaid: inside `"..."` only `"` and `#` (the entity marker) are special, plus `<`, `>`, `&` and a
/// leading backtick, which HTML labels and markdown strings would read; all become `#code;` entities.
fn mermaid_text(s: &str) -> String {
    let mut out = String::new();
    for c in s.split_whitespace().collect::<Vec<_>>().join(" ").chars() {
        match c {
            '"' | '#' | '<' | '>' | '&' | '`' => out += &format!("#{};", c as u32),
            _ => out.push(c),
        }
    }
    out
}

fn dot_text(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").replace('\\', "\\\\").replace('"', "\\\"")
}

/// Included files in the order the nodes first use them.
fn files(g: &Graph) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for f in g.nodes.iter().filter_map(|n| n.file.as_deref()) {
        if !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

fn mermaid(g: &Graph) -> String {
    let node = |i: usize| {
        let n = &g.nodes[i];
        let t = mermaid_text(&n.text);
        match (is_hub(n, "Inventory") || is_hub(n, "HUD"), n.ends) {
            (true, _) => format!("n{i}{{{{\"{t}\"}}}}"),
            (_, true) => format!("n{i}([\"{t}\"])"),
            _ if n.pseudo => format!("n{i}((\"{t}\"))"),
            _ => format!("n{i}[\"{t}\"]"),
        }
    };
    let mut out = String::from("flowchart TD\n");
    for i in (0..g.nodes.len()).filter(|&i| g.nodes[i].file.is_none()) {
        out += &format!("    {}\n", node(i));
    }
    for (k, f) in files(g).into_iter().enumerate() {
        out += &format!("    subgraph f{k}[\"{}\"]\n", mermaid_text(f));
        for i in (0..g.nodes.len()).filter(|&i| g.nodes[i].file.as_deref() == Some(f)) {
            out += &format!("        {}\n", node(i));
        }
        out += "    end\n";
    }
    for (from, to, kind, label) in &g.edges {
        let arrow = match kind {
            Kind::Jump => "==>",
            Kind::Call | Kind::Use => "-.->",
            Kind::Flow | Kind::Choice => "-->",
        };
        let label = match (kind, label.is_empty()) {
            (Kind::Call, true) => "call".to_string(),
            (Kind::Call, false) => format!("call, {label}"),
            _ => label.clone(),
        };
        let label = if label.is_empty() { String::new() } else { format!("|\"{}\"|", mermaid_text(&label)) };
        out += &format!("    n{from} {arrow}{label} n{to}\n");
    }
    let lost: Vec<String> = (0..g.nodes.len()).filter(|&i| g.nodes[i].unreachable).map(|i| format!("n{i}")).collect();
    if !lost.is_empty() {
        out += "    classDef unreachable stroke:#c00,stroke-dasharray:4 3\n";
        out += &format!("    class {} unreachable\n", lost.join(","));
    }
    out
}

fn dot(g: &Graph) -> String {
    let node = |i: usize, pad: &str| {
        let n = &g.nodes[i];
        let mut attrs = format!("label=\"{}\"", dot_text(&n.text));
        match (n.pseudo, n.ends) {
            (true, _) if n.text == "Inventory" || n.text == "HUD" => attrs += ", shape=hexagon",
            (true, _) => attrs += ", shape=circle",
            (_, true) => attrs += ", peripheries=2",
            _ => {}
        }
        if n.unreachable {
            attrs += ", style=dashed, color=red";
        }
        format!("{pad}n{i} [{attrs}];\n")
    };
    let mut out = String::from("digraph story {\n    node [shape=box];\n");
    for i in (0..g.nodes.len()).filter(|&i| g.nodes[i].file.is_none()) {
        out += &node(i, "    ");
    }
    for (k, f) in files(g).into_iter().enumerate() {
        out += &format!("    subgraph cluster_{k} {{\n        label=\"{}\";\n", dot_text(f));
        for i in (0..g.nodes.len()).filter(|&i| g.nodes[i].file.as_deref() == Some(f)) {
            out += &node(i, "        ");
        }
        out += "    }\n";
    }
    for (from, to, kind, label) in &g.edges {
        let mut attrs: Vec<String> = Vec::new();
        let label = match (kind, label.is_empty()) {
            (Kind::Call, true) => "call".to_string(),
            (Kind::Call, false) => format!("call, {label}"),
            _ => label.clone(),
        };
        if !label.is_empty() {
            attrs.push(format!("label=\"{}\"", dot_text(&label)));
        }
        match kind {
            Kind::Jump => attrs.push("penwidth=2".into()),
            Kind::Call => attrs.push("style=dashed".into()),
            Kind::Use => attrs.push("style=dotted".into()),
            Kind::Flow | Kind::Choice => {}
        }
        let attrs = if attrs.is_empty() { String::new() } else { format!(" [{}]", attrs.join(", ")) };
        out += &format!("    n{from} -> n{to}{attrs};\n");
    }
    out + "}\n"
}

pub fn render(program: &Program, format: Format) -> String {
    let g = build(program);
    match format {
        Format::Mermaid => mermaid(&g),
        Format::Dot => dot(&g),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn both(src: &str) -> (String, String) {
        let p = quillstone_script::parse(src).unwrap();
        (render(&p, Format::Mermaid), render(&p, Format::Dot))
    }

    /// Every edge kind once: fall-through, jump, conditional jump, `::if` jump, call, choices (timed
    /// default, conditional), hotspots, item use, an ending, and a label nothing leads to.
    const EVERY_KIND: &str = "---\nvars: { gold: 0, bag: [], name: \"\" }\ninventory: bag\n---\n::item{id=tonic name=Tonic use=#drink}\n\
Prologue.\n# Start\nHi.\n::jump{to=shop if=\"gold > 2\"}\n# Shop\n::if{gold == 1}\n::jump{to=cellar}\n::endif\n\
::call{to=drink}\n::choice{time=5}\n- [Go on a very long walk through the dark and quiet forest](#cellar){default}\n\
- [Rest](#cellar){if gold > 0}\n::\n# Cellar\n::hotspots\n- [Door](#end){rect=0,0,.5,.5}\n::\n# Drink\nGulp.\n::return\n# End\nBye.\n::end\n# Lost\nNobody comes.\n::end\n";

    #[test]
    fn mermaid_has_a_node_per_heading_and_an_edge_per_way_on() {
        let expected = "flowchart TD
    n0((\"(start)\"))
    n1[\"Start\"]
    n2[\"Shop\"]
    n3[\"Cellar\"]
    n4[\"Drink\"]
    n5([\"End\"])
    n6([\"Lost\"])
    n7{{\"Inventory\"}}
    n0 --> n1
    n1 --> n2
    n1 ==>|\"if gold #62; 2\"| n2
    n2 ==>|\"if gold == 1\"| n3
    n2 -.->|\"call\"| n4
    n2 -->|\"Go on a very long walk through\u{2026} (default)\"| n3
    n2 -->|\"Rest [if gold #62; 0]\"| n3
    n3 -->|\"Door\"| n5
    n7 -.->|\"Tonic\"| n4
    classDef unreachable stroke:#c00,stroke-dasharray:4 3
    class n6 unreachable
";
        let m = both(EVERY_KIND).0;
                assert_eq!(m, expected, "{m}");
    }

    #[test]
    fn dot_has_the_same_nodes_and_edges() {
        let expected = "digraph story {
    node [shape=box];
    n0 [label=\"(start)\", shape=circle];
    n1 [label=\"Start\"];
    n2 [label=\"Shop\"];
    n3 [label=\"Cellar\"];
    n4 [label=\"Drink\"];
    n5 [label=\"End\", peripheries=2];
    n6 [label=\"Lost\", peripheries=2, style=dashed, color=red];
    n7 [label=\"Inventory\", shape=hexagon];
    n0 -> n1;
    n1 -> n2;
    n1 -> n2 [label=\"if gold > 2\", penwidth=2];
    n2 -> n3 [label=\"if gold == 1\", penwidth=2];
    n2 -> n4 [label=\"call\", style=dashed];
    n2 -> n3 [label=\"Go on a very long walk through\u{2026} (default)\"];
    n2 -> n3 [label=\"Rest [if gold > 0]\"];
    n3 -> n5 [label=\"Door\"];
    n7 -> n4 [label=\"Tonic\", style=dotted];
}
";
        let d = both(EVERY_KIND).1;
        assert_eq!(d, expected, "{d}");
    }

    #[test]
    fn a_hud_button_hangs_off_a_hud_node_with_a_dotted_edge_to_its_label() {
        let src = "---\njournal: quests\nvars: { quests: [] }\n---\n::hud{id=phone text=Phone call=#ring}\n::hud{id=log text=Log open=journal}\n\
# Start\nHi.\n::end\n# Ring\nBrrr.\n::return\n";
        let (m, d) = both(src);
        assert!(m.contains("n3{{\"HUD\"}}") && m.contains("n3 -.->|\"phone\"| n2"), "{m}");
        assert!(d.contains("n3 [label=\"HUD\", shape=hexagon];") && d.contains("n3 -> n2 [label=\"phone\", style=dotted];"), "{d}");
        assert!(!m.contains("unreachable"), "the label is reached through the button: {m}");
        assert!(!both("# Start\nHi.\n::end\n").0.contains("HUD"), "no node without a button");
    }

    #[test]
    fn nasty_titles_are_escaped_for_each_format() {
        let (m, d) = both("# Say \"hi\" [now] #1 <b>&co `x`\nHi.\n::end\n");
        assert!(m.contains("n0([\"Say #34;hi#34; [now] #35;1 #60;b#62;#38;co #96;x#96;\"])"), "{m}");
        assert!(d.contains("label=\"Say \\\"hi\\\" [now] #1 <b>&co `x`\""), "{d}");
        let (m, d) = both("---\nvars: { name: \"\" }\n---\n# A\n::jump{to=b if=\"name == '<i>'\"}\n# B\nHi.\n");
        assert!(m.contains("name == #34;#60;i#62;#34;"), "{m}");
        assert!(d.contains("\\\""), "{d}");
    }

    #[test]
    fn a_once_option_says_once_not_its_hidden_variable() {
        let m = both("# A\n::choice\n- [Look](#a){once}\n::\n").0;
        assert!(m.contains("|\"Look [if once]\"|"), "{m}");
    }

    #[test]
    fn output_is_the_same_every_time() {
        let a = both(EVERY_KIND);
        for _ in 0..5 {
            assert_eq!(both(EVERY_KIND), a);
        }
    }

    #[test]
    fn included_files_get_a_group_each() {
        let files: HashMap<String, String> = [("ch/b.quill.md".to_string(), "# B\nTwo.\n".to_string())].into();
        let p = quillstone_script::parse_files("# A\nOne.\n::include{ch/b.quill.md}\n", "main.quill.md", &|f| {
            files.get(f).cloned().ok_or_else(|| "no".to_string())
        })
        .unwrap();
        let m = render(&p, Format::Mermaid);
        assert!(m.contains("    n0[\"A\"]\n    subgraph f0[\"ch/b.quill.md\"]\n        n1([\"B\"])\n    end\n"), "{m}");
        assert!(m.contains("n0 --> n1"), "{m}");
        let d = render(&p, Format::Dot);
        assert!(d.contains("subgraph cluster_0 {\n        label=\"ch/b.quill.md\";\n        n1 [label=\"B\", peripheries=2];"), "{d}");
    }

    #[test]
    fn a_jump_does_not_also_fall_through_and_an_if_without_jumps_adds_no_edge() {
        let (m, _) = both("# A\n::if{x}\nHi.\n::endif\n::jump{to=c}\n# B\nHi.\n::jump{to=c}\n# C\nHi.\n");
        assert!(m.contains("n0 ==> n2") && m.contains("n1 ==> n2") && !m.contains("n0 --> n1"), "{m}");
    }
}
