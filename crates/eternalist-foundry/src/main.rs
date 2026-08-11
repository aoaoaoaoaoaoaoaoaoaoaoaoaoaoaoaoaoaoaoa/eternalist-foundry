use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand, ValueEnum};

use crate::{
    contract::{Contract, Coordinate, workspace_of},
    error::Result,
    plan::Plan,
    support::{Adjudication, support_path},
    toolchain::ToolchainFile,
};

mod contract;
mod error;
mod plan;
mod proof;
mod support;
mod toolchain;

#[derive(Debug, Parser)]
#[command(version, about)]
struct Invocation {
    #[arg(long, global = true, default_value = "foundry.toml")]
    contract: PathBuf,
    #[command(subcommand)]
    directive: Directive,
}

#[derive(Debug, Subcommand)]
enum Directive {
    /// Validate the complete product contract.
    Check,
    /// Expand the contract into its exact proof graph.
    Plan {
        #[arg(long, value_enum, default_value_t = PlanFormat::Human)]
        format: PlanFormat,
        #[arg(long)]
        github_output: Option<PathBuf>,
    },
    /// Execute one planned proof and seal its receipt.
    Prove {
        proof: String,
        #[arg(long)]
        coordinate: Option<Coordinate>,
        #[arg(long, default_value = "evidence")]
        evidence_root: PathBuf,
    },
    /// Demand one successful, source-matched receipt for every planned proof.
    Judge {
        #[arg(default_value = "evidence")]
        evidence_root: PathBuf,
    },
    /// Judge the evidence and emit the public support ledger.
    Support {
        #[arg(default_value = "evidence")]
        evidence_root: PathBuf,
        #[arg()]
        output: Option<PathBuf>,
    },
    /// Judge evidence and stage collision-free release assets.
    Stage {
        #[arg(default_value = "evidence")]
        evidence_root: PathBuf,
        output: PathBuf,
    },
    /// Install the repository's declared Rust toolchain through rustup.
    Toolchain {
        #[arg(long, default_value = "rust-toolchain.toml")]
        file: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PlanFormat {
    Human,
    Json,
    Matrix,
}

fn main() -> ExitCode {
    match execute(Invocation::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("foundry: {error}");
            ExitCode::FAILURE
        }
    }
}

fn execute(invocation: Invocation) -> Result<()> {
    let workspace = workspace_of(&invocation.contract);
    match invocation.directive {
        Directive::Check => {
            let contract = Contract::load(&invocation.contract)?;
            println!(
                "{}: contract schema {} carries {} coordinate(s)",
                contract.product.name,
                contract.schema,
                contract.carried_coordinates().count()
            );
        }
        Directive::Plan {
            format,
            github_output,
        } => {
            let contract = Contract::load(&invocation.contract)?;
            let plan = Plan::forge(&contract)?;
            if let Some(path) = github_output {
                append_github_output(&path, &plan)?;
            }
            match format {
                PlanFormat::Human => print_plan(&plan),
                PlanFormat::Json => println!("{}", serde_json::to_string_pretty(&plan)?),
                PlanFormat::Matrix => {
                    println!("{}", serde_json::to_string(&plan.github_matrix())?);
                }
            }
        }
        Directive::Prove {
            proof: name,
            coordinate,
            evidence_root,
        } => proof::execute(&invocation.contract, &name, coordinate, &evidence_root)?,
        Directive::Judge { evidence_root } => {
            let judgment = Adjudication::judge(&invocation.contract, &evidence_root)?;
            println!(
                "{}: {} proof receipt(s) bind source {}",
                judgment.contract.product.name,
                judgment.receipts.len(),
                judgment.source
            );
        }
        Directive::Support {
            evidence_root,
            output,
        } => {
            let judgment = Adjudication::judge(&invocation.contract, &evidence_root)?;
            let output = output.unwrap_or_else(|| support_path(&evidence_root));
            let manifest = judgment.write_support(workspace, &output)?;
            println!(
                "{} {}: wrote {} release-tested coordinate(s) to {}",
                manifest.product.name,
                manifest.product.version,
                manifest.release_tested.len(),
                output.display()
            );
        }
        Directive::Stage {
            evidence_root,
            output,
        } => {
            let judgment = Adjudication::judge(&invocation.contract, &evidence_root)?;
            let manifest = judgment.stage(workspace, &evidence_root, &output)?;
            println!(
                "{} {}: staged {} artifact(s) in {}",
                manifest.product.name,
                manifest.product.version,
                manifest.artifacts.len() + 1,
                output.display()
            );
        }
        Directive::Toolchain { file } => ToolchainFile::load(&file)?.install()?,
    }
    Ok(())
}

fn print_plan(plan: &Plan) {
    println!("{}: {} proof node(s)", plan.product, plan.nodes.len());
    for node in &plan.nodes {
        println!(
            "{:<54} {:<18} {}",
            node.id,
            node.runner,
            node.laws
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
    }
}

fn append_github_output(path: &Path, plan: &Plan) -> Result<()> {
    use std::{fs::OpenOptions, io::Write as _};

    let matrix = serde_json::to_string(&plan.github_matrix())?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| error::io("open GitHub output", path, source))?;
    writeln!(file, "proofs={matrix}")
        .map_err(|source| error::io("write GitHub output", path, source))
}
