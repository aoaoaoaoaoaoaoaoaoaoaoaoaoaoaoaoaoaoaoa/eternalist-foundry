use std::{
    env,
    fs::{self, File},
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{
    Result,
    contract::{Contract, Coordinate, Law, workspace_of},
    error::{Error, io},
    plan::Plan,
};

pub const RECEIPT_SCHEMA: u32 = 2;

pub fn execute(
    contract_path: &Path,
    proof_name: &str,
    coordinate: Option<Coordinate>,
    evidence_root: &Path,
) -> Result<()> {
    let contract = Contract::load(contract_path)?;
    let plan = Plan::forge(&contract)?;
    let node_id = node_id(proof_name, coordinate);
    let node = plan
        .node(&node_id)
        .ok_or_else(|| Error::Contract(format!("proof plan contains no node `{node_id}`")))?;
    let proof = contract
        .proof(proof_name)
        .ok_or_else(|| Error::Contract(format!("contract contains no proof `{proof_name}`")))?;
    if let Some(coordinate) = coordinate
        && !coordinate.inhabits_current_host()
    {
        return Err(Error::Contract(format!(
            "coordinate `{coordinate}` cannot be proved by {}-{}",
            env::consts::OS,
            env::consts::ARCH
        )));
    }
    let workspace = workspace_of(contract_path);
    let source = source_identity(workspace)?;
    let root = evidence_root.join(&node.id);
    let receipt_path = root.join("receipt.json");
    if receipt_path.exists() {
        return Err(Error::Contract(format!(
            "receipt `{}` already exists",
            receipt_path.display()
        )));
    }
    let product_evidence = root.join("product");
    let artifact_root = root.join("artifacts");
    for path in [&product_evidence, &artifact_root] {
        fs::create_dir_all(path).map_err(|source| io("create evidence directory", path, source))?;
    }

    let begun_at = unix_seconds()?;
    let begun = Instant::now();
    let command_name = proof
        .run
        .first()
        .ok_or_else(|| Error::Contract(format!("proof `{proof_name}` has no executable")))?;
    let mut command = Command::new(command_name);
    let _command = command
        .args(&proof.run[1..])
        .current_dir(workspace)
        .env("FOUNDRY_EVIDENCE_DIR", &product_evidence)
        .env("FOUNDRY_ARTIFACT_DIR", &artifact_root)
        .env("FOUNDRY_PROOF", proof_name)
        .env(
            "FOUNDRY_COORDINATE",
            coordinate.map_or_else(|| "global".to_owned(), |value| value.to_string()),
        );
    let status = command.status().map_err(|source| Error::Spawn {
        command: command_display(&proof.run),
        source,
    })?;

    let receipt = Receipt {
        schema: RECEIPT_SCHEMA,
        node: node.id.clone(),
        proof: proof.name.clone(),
        coordinate,
        host: HostWitness::current(),
        laws: proof.laws.clone(),
        source,
        command: proof.run.clone(),
        begun_unix_seconds: begun_at,
        elapsed_milliseconds: begun.elapsed().as_millis(),
        success: status.success(),
        exit_code: status.code(),
        run_url: github_run_url(),
        artifacts: digest_tree(&artifact_root)?,
    };
    write_json_atomic(&receipt_path, &receipt)?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::ProofFailed {
            proof: node.id.clone(),
            status,
        })
    }
}

pub fn node_id(proof: &str, coordinate: Option<Coordinate>) -> String {
    format!(
        "{proof}--{}",
        coordinate.map_or_else(|| "global".to_owned(), |value| value.to_string())
    )
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: u32,
    pub node: String,
    pub proof: String,
    pub coordinate: Option<Coordinate>,
    pub host: HostWitness,
    pub laws: Vec<Law>,
    pub source: String,
    pub command: Vec<String>,
    pub begun_unix_seconds: u64,
    pub elapsed_milliseconds: u128,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub run_url: Option<String>,
    pub artifacts: Vec<Artifact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostWitness {
    pub os: String,
    pub arch: String,
}

impl HostWitness {
    fn current() -> Self {
        Self {
            os: env::consts::OS.to_owned(),
            arch: env::consts::ARCH.to_owned(),
        }
    }
}

impl Receipt {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).map_err(|source| io("read proof receipt", path, source))?;
        serde_json::from_slice(&bytes).map_err(Error::from)
    }

    pub fn validate_artifacts(&self, root: &Path) -> Result<()> {
        if !root.exists() {
            return if self.artifacts.is_empty() {
                Ok(())
            } else {
                Err(Error::Contract(format!(
                    "artifact directory for `{}` is absent despite its nonempty receipt",
                    self.node
                )))
            };
        }
        let actual = digest_tree(root)?;
        if actual == self.artifacts {
            Ok(())
        } else {
            Err(Error::Contract(format!(
                "artifact inventory for `{}` differs from its receipt",
                self.node
            )))
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

pub fn source_identity(workspace: &Path) -> Result<String> {
    if let Some(source) = env::var_os("FOUNDRY_SOURCE") {
        let value = source.to_string_lossy().trim().to_owned();
        if !value.is_empty() {
            return Ok(value);
        }
    }
    if let Some(sha) = env::var_os("GITHUB_SHA") {
        let value = sha.to_string_lossy().trim().to_owned();
        if !value.is_empty() {
            return Ok(value);
        }
    }
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace)
        .output()
        .map_err(|source| Error::Spawn {
            command: "git rev-parse HEAD".to_owned(),
            source,
        })?;
    if !output.status.success() {
        return Err(Error::Contract(
            "cannot resolve source identity with `git rev-parse HEAD`".to_owned(),
        ));
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if value.is_empty() {
        Err(Error::MissingOutput {
            command: "git rev-parse HEAD".to_owned(),
        })
    } else {
        Ok(value)
    }
}

fn github_run_url() -> Option<String> {
    let server = env::var("GITHUB_SERVER_URL").ok()?;
    let repository = env::var("GITHUB_REPOSITORY").ok()?;
    let run = env::var("GITHUB_RUN_ID").ok()?;
    Some(format!("{server}/{repository}/actions/runs/{run}"))
}

fn unix_seconds() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| Error::Contract("system clock predates the Unix epoch".to_owned()))
}

pub fn digest_tree(root: &Path) -> Result<Vec<Artifact>> {
    let mut paths = Vec::new();
    collect_files(root, &mut paths)?;
    paths.sort();
    paths
        .into_iter()
        .map(|path| digest_file(root, &path))
        .collect()
}

fn collect_files(root: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    let entries =
        fs::read_dir(root).map_err(|source| io("read artifact directory", root, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| io("read artifact entry", root, source))?;
        let kind = entry
            .file_type()
            .map_err(|source| io("inspect artifact entry", entry.path(), source))?;
        if kind.is_dir() {
            collect_files(&entry.path(), paths)?;
        } else if kind.is_file() {
            paths.push(entry.path());
        } else {
            return Err(Error::Contract(format!(
                "artifact `{}` is not a regular file",
                entry.path().display()
            )));
        }
    }
    Ok(())
}

fn digest_file(root: &Path, path: &Path) -> Result<Artifact> {
    let file = File::open(path).map_err(|source| io("open artifact", path, source))?;
    let bytes = file
        .metadata()
        .map_err(|source| io("inspect artifact", path, source))?
        .len();
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|source| io("hash artifact", path, source))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| Error::Contract(format!("artifact `{}` escaped its root", path.display())))?;
    let relative = relative.to_str().ok_or_else(|| {
        Error::Contract(format!(
            "artifact path `{}` is not UTF-8",
            relative.display()
        ))
    })?;
    let digest = digest.finalize();
    Ok(Artifact {
        path: relative.replace(std::path::MAIN_SEPARATOR, "/"),
        bytes,
        sha256: format!("{digest:x}"),
    })
}

pub fn write_json_atomic<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| io("create JSON parent", parent, source))?;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut file =
        File::create(&temporary).map_err(|source| io("create JSON", &temporary, source))?;
    file.write_all(&bytes)
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|source| io("commit JSON", &temporary, source))?;
    fs::rename(&temporary, path).map_err(|source| io("replace JSON", path, source))
}

fn command_display(command: &[String]) -> String {
    command
        .iter()
        .map(|part| shellish(part))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shellish(value: &str) -> String {
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b':' | b'=')
    }) {
        value.to_owned()
    } else {
        format!("{value:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_inventory_is_sorted_and_content_addressed() {
        let root = tempfile::tempdir().expect("temporary artifact root");
        fs::create_dir(root.path().join("z")).expect("create nested artifact directory");
        fs::write(root.path().join("z/b"), b"second").expect("write artifact");
        fs::write(root.path().join("a"), b"first").expect("write artifact");
        let inventory = digest_tree(root.path()).expect("digest artifacts");
        assert_eq!(inventory[0].path, "a");
        assert_eq!(inventory[1].path, "z/b");
        assert_ne!(inventory[0].sha256, inventory[1].sha256);
    }

    #[test]
    fn foreign_host_coordinate_is_rejected_before_execution() {
        let coordinate = if Coordinate::LinuxX86_64.inhabits_current_host() {
            Coordinate::WindowsX86_64
        } else {
            Coordinate::LinuxX86_64
        };
        let contract =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/library.toml");
        let evidence = tempfile::tempdir().expect("temporary evidence root");
        let error = execute(&contract, "host", Some(coordinate), evidence.path())
            .expect_err("foreign coordinate must fail");
        assert!(error.to_string().contains("cannot be proved"));
        assert!(
            fs::read_dir(evidence.path())
                .expect("read evidence root")
                .next()
                .is_none()
        );
    }

    #[test]
    fn empty_inventory_survives_archive_directory_elision() {
        let root = tempfile::tempdir().expect("temporary evidence root");
        let missing = root.path().join("artifacts");
        let mut receipt = Receipt {
            schema: RECEIPT_SCHEMA,
            node: "source--global".to_owned(),
            proof: "source".to_owned(),
            coordinate: None,
            host: HostWitness::current(),
            laws: vec![Law::Source],
            source: "source".to_owned(),
            command: vec!["true".to_owned()],
            begun_unix_seconds: 0,
            elapsed_milliseconds: 0,
            success: true,
            exit_code: Some(0),
            run_url: None,
            artifacts: Vec::new(),
        };
        receipt
            .validate_artifacts(&missing)
            .expect("archivers may omit empty directories");
        receipt.artifacts.push(Artifact {
            path: "missing".to_owned(),
            bytes: 1,
            sha256: "00".repeat(32),
        });
        assert!(receipt.validate_artifacts(&missing).is_err());
    }
}
