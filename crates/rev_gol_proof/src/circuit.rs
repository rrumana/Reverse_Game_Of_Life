//! Boolean circuit and CNF input IR for `SAT -> Rev_GOL` construction work.

use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Literal {
    pub variable: String,
    pub negated: bool,
}

impl Literal {
    pub fn positive(variable: impl Into<String>) -> Self {
        Self {
            variable: variable.into(),
            negated: false,
        }
    }

    pub fn negative(variable: impl Into<String>) -> Self {
        Self {
            variable: variable.into(),
            negated: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clause {
    pub literals: Vec<Literal>,
}

impl Clause {
    pub fn new(literals: Vec<Literal>) -> Self {
        Self { literals }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CnfFormula {
    pub clauses: Vec<Clause>,
}

impl CnfFormula {
    pub fn new(clauses: Vec<Clause>) -> Self {
        Self { clauses }
    }

    pub fn variables(&self) -> Vec<String> {
        let mut vars = self
            .clauses
            .iter()
            .flat_map(|clause| clause.literals.iter().map(|lit| lit.variable.clone()))
            .collect::<Vec<_>>();
        vars.sort();
        vars.dedup();
        vars
    }

    pub fn evaluate(&self, assignment: &BTreeMap<String, bool>) -> bool {
        self.clauses.iter().all(|clause| {
            clause.literals.iter().any(|literal| {
                let value = assignment.get(&literal.variable).copied().unwrap_or(false);
                if literal.negated {
                    !value
                } else {
                    value
                }
            })
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CircuitNodeKind {
    Input { name: String },
    Const { value: bool },
    Not { input: NodeId },
    Or { lhs: NodeId, rhs: NodeId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircuitNode {
    pub id: NodeId,
    pub kind: CircuitNodeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Circuit {
    nodes: Vec<CircuitNode>,
    inputs: BTreeMap<String, NodeId>,
    output: NodeId,
}

impl Circuit {
    pub fn from_cnf(formula: &CnfFormula) -> Self {
        let mut builder = CircuitBuilder::default();
        let mut inputs = BTreeMap::new();

        for variable in formula.variables() {
            let input = builder.input(variable.clone());
            inputs.insert(variable, input);
        }

        let clause_nodes = formula
            .clauses
            .iter()
            .map(|clause| {
                let literal_nodes = clause
                    .literals
                    .iter()
                    .map(|literal| {
                        let input = inputs[&literal.variable];
                        if literal.negated {
                            builder.not(input)
                        } else {
                            input
                        }
                    })
                    .collect::<Vec<_>>();
                builder.or_many(&literal_nodes)
            })
            .collect::<Vec<_>>();

        let output = builder.and_many(&clause_nodes);
        Self {
            nodes: builder.nodes,
            inputs,
            output,
        }
    }

    pub fn nodes(&self) -> &[CircuitNode] {
        &self.nodes
    }

    pub fn inputs(&self) -> &BTreeMap<String, NodeId> {
        &self.inputs
    }

    pub fn output(&self) -> NodeId {
        self.output
    }

    pub fn node(&self, id: NodeId) -> &CircuitNode {
        &self.nodes[id.0]
    }

    pub fn fanout_counts(&self) -> HashMap<NodeId, usize> {
        let mut counts = HashMap::new();
        for node in &self.nodes {
            counts.entry(node.id).or_insert(0);
            match node.kind {
                CircuitNodeKind::Input { .. } | CircuitNodeKind::Const { .. } => {}
                CircuitNodeKind::Not { input } => {
                    *counts.entry(input).or_insert(0) += 1;
                }
                CircuitNodeKind::Or { lhs, rhs } => {
                    *counts.entry(lhs).or_insert(0) += 1;
                    *counts.entry(rhs).or_insert(0) += 1;
                }
            }
        }
        counts
    }

    pub fn depths(&self) -> HashMap<NodeId, usize> {
        let mut depths = HashMap::<NodeId, usize>::new();
        for node in &self.nodes {
            let depth = match node.kind {
                CircuitNodeKind::Input { .. } | CircuitNodeKind::Const { .. } => 0,
                CircuitNodeKind::Not { input } => depths[&input] + 1,
                CircuitNodeKind::Or { lhs, rhs } => depths[&lhs].max(depths[&rhs]) + 1,
            };
            depths.insert(node.id, depth);
        }
        depths
    }
}

#[derive(Debug, Default)]
struct CircuitBuilder {
    nodes: Vec<CircuitNode>,
}

impl CircuitBuilder {
    fn push(&mut self, kind: CircuitNodeKind) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(CircuitNode { id, kind });
        id
    }

    fn input(&mut self, name: impl Into<String>) -> NodeId {
        self.push(CircuitNodeKind::Input { name: name.into() })
    }

    fn constant(&mut self, value: bool) -> NodeId {
        self.push(CircuitNodeKind::Const { value })
    }

    fn not(&mut self, input: NodeId) -> NodeId {
        self.push(CircuitNodeKind::Not { input })
    }

    fn or(&mut self, lhs: NodeId, rhs: NodeId) -> NodeId {
        self.push(CircuitNodeKind::Or { lhs, rhs })
    }

    fn or_many(&mut self, nodes: &[NodeId]) -> NodeId {
        match nodes {
            [] => self.constant(false),
            [single] => *single,
            _ => self.reduce_balanced(nodes, |builder, lhs, rhs| builder.or(lhs, rhs)),
        }
    }

    fn and_many(&mut self, nodes: &[NodeId]) -> NodeId {
        match nodes {
            [] => self.constant(true),
            [single] => *single,
            _ => {
                let negated = nodes.iter().map(|&node| self.not(node)).collect::<Vec<_>>();
                let or = self.or_many(&negated);
                self.not(or)
            }
        }
    }

    fn reduce_balanced<F>(&mut self, nodes: &[NodeId], mut combine: F) -> NodeId
    where
        F: FnMut(&mut Self, NodeId, NodeId) -> NodeId,
    {
        let mut current = nodes.to_vec();
        while current.len() > 1 {
            let mut next = Vec::new();
            let mut iter = current.into_iter();
            while let Some(lhs) = iter.next() {
                if let Some(rhs) = iter.next() {
                    next.push(combine(self, lhs, rhs));
                } else {
                    next.push(lhs);
                }
            }
            current = next;
        }
        current[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cnf_variables_are_collected_sorted() {
        let formula = CnfFormula::new(vec![
            Clause::new(vec![Literal::positive("b"), Literal::negative("a")]),
            Clause::new(vec![Literal::positive("c")]),
        ]);

        assert_eq!(
            formula.variables(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn test_circuit_from_cnf_reuses_input_nodes() {
        let formula = CnfFormula::new(vec![
            Clause::new(vec![Literal::positive("x"), Literal::negative("y")]),
            Clause::new(vec![Literal::negative("x")]),
        ]);
        let circuit = Circuit::from_cnf(&formula);

        assert_eq!(circuit.inputs().len(), 2);
        let fanout = circuit.fanout_counts();
        assert!(fanout[&circuit.inputs()["x"]] >= 2);
        assert!(fanout[&circuit.inputs()["y"]] >= 1);
    }

    #[test]
    fn test_cnf_evaluation_matches_expected_truth_values() {
        let formula = CnfFormula::new(vec![
            Clause::new(vec![Literal::positive("x"), Literal::negative("y")]),
            Clause::new(vec![Literal::positive("y")]),
        ]);

        let false_case = BTreeMap::from([("x".to_string(), false), ("y".to_string(), false)]);
        let true_case = BTreeMap::from([("x".to_string(), true), ("y".to_string(), true)]);

        assert!(!formula.evaluate(&false_case));
        assert!(formula.evaluate(&true_case));
    }

    #[test]
    fn test_circuit_depths_are_topological() {
        let formula = CnfFormula::new(vec![Clause::new(vec![
            Literal::positive("x"),
            Literal::negative("y"),
            Literal::positive("z"),
        ])]);
        let circuit = Circuit::from_cnf(&formula);
        let depths = circuit.depths();

        for node in circuit.nodes() {
            match node.kind {
                CircuitNodeKind::Input { .. } | CircuitNodeKind::Const { .. } => {
                    assert_eq!(depths[&node.id], 0);
                }
                CircuitNodeKind::Not { input } => assert!(depths[&node.id] > depths[&input]),
                CircuitNodeKind::Or { lhs, rhs } => {
                    assert!(depths[&node.id] > depths[&lhs]);
                    assert!(depths[&node.id] > depths[&rhs]);
                }
            }
        }
    }
}
