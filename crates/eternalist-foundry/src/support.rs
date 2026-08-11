use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};

use crate::{
    Result,
    contract::{
        Contract, Coordinate, Delivery, Exclusion, Law, Platform, Profile, Trust, workspace_of,
    },
    error::{Error, io},
    plan::Plan,
    proof::{Artifact, RECEIPT_SCHEMA, Receipt, source_identity, write_json_atomic},
};

pub const SUPPORT_SCHEMA: u32 = 1;

pub struct Adjudication {
    pub contract: Contract,
    pub receipts: Vec<Receipt>,
    pub source: String,
}

impl Adjudication {
    pub fn judge(contract_path: &Path, evidence_root: &Path) -> Result<Self> {
        let contract = Contract::load(contract_path)?;
        let plan = Plan::forge(&contract)?;
        let workspace = workspace_of(contract_path);
        let source = source_identity(workspace)?;
        reject_alien_cells(evidence_root, &plan)?;
        let mut receipts = Vec::with_capacity(plan.nodes.len());
        for node in &plan.nodes {
            let cell = evidence_root.join(&node.id);
            let path = cell.join("receipt.json");
            let receipt = Receipt::load(&path)?;
            if receipt.schema != RECEIPT_SCHEMA {
                return Err(Error::Contract(format!(
                    "receipt `{}` has schema {}, expected {RECEIPT_SCHEMA}",
                    node.id, receipt.schema
                )));
            }
            if receipt.node != node.id
                || receipt.proof != node.proof
                || receipt.coordinate != node.coordinate
                || receipt.laws != node.laws
                || receipt.command != proof_command(&contract, &node.proof)?
            {
                return Err(Error::Contract(format!(
                    "receipt `{}` does not describe its planned node",
                    node.id
                )));
            }
            if receipt.source != source {
                return Err(Error::Contract(format!(
                    "receipt `{}` proves source `{}`, expected `{source}`",
                    node.id, receipt.source
                )));
            }
            if !receipt.success {
                return Err(Error::Contract(format!(
                    "receipt `{}` records a failed proof",
                    node.id
                )));
            }
            receipt.validate_artifacts(&cell.join("artifacts"))?;
            receipts.push(receipt);
        }
        Ok(Self {
            contract,
            receipts,
            source,
        })
    }

    pub fn support(&self, workspace: &Path) -> Result<SupportManifest> {
        let package = cargo_package(workspace, &self.contract.product.package)?;
        let release_tested = self
            .contract
            .coordinates
            .release_tested
            .iter()
            .map(|coordinate| self.coordinate(*coordinate))
            .collect();
        let supported = self
            .contract
            .coordinates
            .supported
            .iter()
            .map(|coordinate| self.coordinate(*coordinate))
            .collect();
        let global_proofs = self
            .receipts
            .iter()
            .filter(|receipt| receipt.coordinate.is_none())
            .map(ProofEvidence::from)
            .collect();
        let artifacts = self
            .receipts
            .iter()
            .flat_map(|receipt| {
                receipt
                    .artifacts
                    .iter()
                    .cloned()
                    .map(|artifact| ArtifactEvidence {
                        node: receipt.node.clone(),
                        proof: receipt.proof.clone(),
                        coordinate: receipt.coordinate,
                        artifact,
                    })
            })
            .collect();
        Ok(SupportManifest {
            schema: SUPPORT_SCHEMA,
            product: SupportProduct {
                name: self.contract.product.name.clone(),
                package: self.contract.product.package.clone(),
                version: package.version,
                profile: self.contract.product.profile,
                identifier: self.contract.product.identifier.clone(),
            },
            source: SupportSource {
                repository: package.repository,
                commit: self.source.clone(),
                tag: github_tag(),
            },
            release_tested,
            supported,
            exclusions: self.contract.exclusions.clone(),
            global_proofs,
            artifacts,
        })
    }

    pub fn write_support(&self, workspace: &Path, output: &Path) -> Result<SupportManifest> {
        let manifest = self.support(workspace)?;
        write_json_atomic(output, &manifest)?;
        Ok(manifest)
    }

    fn coordinate(&self, coordinate: Coordinate) -> CoordinateSupport {
        let platform = coordinate.platform();
        CoordinateSupport {
            coordinate,
            platform,
            delivery: self.contract.delivery.for_platform(platform),
            trust: self.contract.trust.for_platform(platform),
            proofs: self
                .receipts
                .iter()
                .filter(|receipt| receipt.coordinate == Some(coordinate))
                .map(ProofEvidence::from)
                .collect(),
        }
    }
}

fn proof_command<'a>(contract: &'a Contract, proof: &str) -> Result<&'a [String]> {
    contract
        .proof(proof)
        .map(|proof| proof.run.as_slice())
        .ok_or_else(|| Error::Contract(format!("contract contains no proof `{proof}`")))
}

fn reject_alien_cells(evidence_root: &Path, plan: &Plan) -> Result<()> {
    let expected = plan
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<BTreeSet<_>>();
    let entries = fs::read_dir(evidence_root)
        .map_err(|source| io("read evidence root", evidence_root, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| io("read evidence cell", evidence_root, source))?;
        if !entry
            .file_type()
            .map_err(|source| io("inspect evidence cell", entry.path(), source))?
            .is_dir()
        {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| {
            Error::Contract(format!(
                "evidence cell `{}` is not UTF-8",
                entry.path().display()
            ))
        })?;
        if !expected.contains(name) {
            return Err(Error::Contract(format!(
                "evidence contains unplanned cell `{name}`"
            )));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
pub struct SupportManifest {
    pub schema: u32,
    pub product: SupportProduct,
    pub source: SupportSource,
    pub release_tested: Vec<CoordinateSupport>,
    pub supported: Vec<CoordinateSupport>,
    pub exclusions: Vec<Exclusion>,
    pub global_proofs: Vec<ProofEvidence>,
    pub artifacts: Vec<ArtifactEvidence>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SupportProduct {
    pub name: String,
    pub package: String,
    pub version: String,
    pub profile: Profile,
    pub identifier: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SupportSource {
    pub repository: Option<String>,
    pub commit: String,
    pub tag: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CoordinateSupport {
    pub coordinate: Coordinate,
    pub platform: Platform,
    pub delivery: Option<Delivery>,
    pub trust: Option<Trust>,
    pub proofs: Vec<ProofEvidence>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProofEvidence {
    pub node: String,
    pub proof: String,
    pub laws: Vec<Law>,
    pub run_url: Option<String>,
    pub elapsed_milliseconds: u128,
}

impl From<&Receipt> for ProofEvidence {
    fn from(receipt: &Receipt) -> Self {
        Self {
            node: receipt.node.clone(),
            proof: receipt.proof.clone(),
            laws: receipt.laws.clone(),
            run_url: receipt.run_url.clone(),
            elapsed_milliseconds: receipt.elapsed_milliseconds,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ArtifactEvidence {
    pub node: String,
    pub proof: String,
    pub coordinate: Option<Coordinate>,
    #[serde(flatten)]
    pub artifact: Artifact,
}

#[derive(Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
}

#[derive(Clone, Deserialize)]
struct CargoPackage {
    name: String,
    version: String,
    repository: Option<String>,
}

fn cargo_package(workspace: &Path, name: &str) -> Result<CargoPackage> {
    let output = Command::new("cargo")
        .args(["metadata", "--locked", "--no-deps", "--format-version", "1"])
        .current_dir(workspace)
        .output()
        .map_err(|source| Error::Spawn {
            command: "cargo metadata --locked --no-deps --format-version 1".to_owned(),
            source,
        })?;
    if !output.status.success() {
        return Err(Error::Contract(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let metadata = serde_json::from_slice::<CargoMetadata>(&output.stdout)?;
    let mut packages = metadata
        .packages
        .into_iter()
        .filter(|package| package.name == name);
    let package = packages
        .next()
        .ok_or_else(|| Error::Contract(format!("Cargo metadata contains no package `{name}`")))?;
    if packages.next().is_some() {
        return Err(Error::Contract(format!(
            "Cargo metadata contains multiple packages named `{name}`"
        )));
    }
    Ok(package)
}

fn github_tag() -> Option<String> {
    if env::var("GITHUB_REF_TYPE").ok().as_deref() == Some("tag") {
        env::var("GITHUB_REF_NAME").ok()
    } else {
        None
    }
}

pub fn support_path(root: &Path) -> PathBuf {
    root.join("support.json")
}
