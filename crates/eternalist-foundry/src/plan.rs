use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    Result,
    contract::{Contract, Coordinate, Law, Setup},
    error::Error,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: u32,
    pub product: String,
    pub nodes: Vec<Node>,
}

impl Plan {
    pub fn forge(contract: &Contract) -> Result<Self> {
        let mut nodes = Vec::new();
        for proof in &contract.proofs {
            if proof.coordinates.is_empty() {
                nodes.push(Node::forge(proof, None));
            } else {
                nodes.extend(
                    proof
                        .coordinates
                        .iter()
                        .copied()
                        .map(|coordinate| Node::forge(proof, Some(coordinate))),
                );
            }
        }
        let mut ids = BTreeSet::new();
        for node in &nodes {
            if !ids.insert(&node.id) {
                return Err(Error::Contract(format!(
                    "proof plan contains duplicate node `{}`",
                    node.id
                )));
            }
        }
        Ok(Self {
            schema: contract.schema,
            product: contract.product.name.clone(),
            nodes,
        })
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn github_matrix(&self) -> Matrix<'_> {
        Matrix {
            include: &self.nodes,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub proof: String,
    pub coordinate: Option<Coordinate>,
    pub laws: Vec<Law>,
    pub runner: String,
    pub setup: Setup,
    pub target: Option<String>,
    pub timeout_minutes: u16,
}

impl Node {
    fn forge(proof: &crate::contract::Proof, coordinate: Option<Coordinate>) -> Self {
        let target = coordinate.map(|value| value.target_triple().to_owned());
        let suffix = coordinate.map_or_else(|| "global".to_owned(), |value| value.to_string());
        let coordinate_name =
            coordinate.map_or_else(|| "global".to_owned(), |value| value.to_string());
        Self {
            id: format!("{}--{suffix}", proof.name),
            name: format!("{} · {coordinate_name}", proof.name),
            proof: proof.name.clone(),
            coordinate,
            laws: proof.laws.clone(),
            runner: coordinate
                .map_or("ubuntu-24.04", Coordinate::runner)
                .to_owned(),
            setup: proof.setup(coordinate),
            target,
            timeout_minutes: proof.timeout_minutes,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Matrix<'a> {
    pub include: &'a [Node],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::Contract;

    #[test]
    fn one_proof_expands_across_its_coordinates() {
        let contract =
            toml::from_str::<Contract>(include_str!("../../../tests/fixtures/library.toml"))
                .expect("parse fixture");
        contract.validate().expect("validate fixture");
        let plan = Plan::forge(&contract).expect("forge plan");
        assert!(plan.node("host--windows-x86_64").is_some());
        assert_eq!(plan.nodes.len(), 7);
    }
}
