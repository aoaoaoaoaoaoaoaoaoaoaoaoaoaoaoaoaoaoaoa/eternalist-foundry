use std::{fs, path::Path, process::Command};

use serde::Deserialize;

use crate::{
    Result,
    error::{Error, io},
};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolchainFile {
    pub toolchain: Toolchain,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Toolchain {
    pub channel: String,
    #[serde(default = "minimal")]
    pub profile: String,
    #[serde(default)]
    pub components: Vec<String>,
    #[serde(default)]
    pub targets: Vec<String>,
}

impl ToolchainFile {
    pub fn load(path: &Path) -> Result<Self> {
        let source =
            fs::read_to_string(path).map_err(|source| io("read toolchain", path, source))?;
        let parsed = toml::from_str::<Self>(&source).map_err(|source| Error::Toml {
            path: path.to_path_buf(),
            source,
        })?;
        parsed.validate()?;
        Ok(parsed)
    }

    pub fn install(&self) -> Result<()> {
        let spec = &self.toolchain;
        let mut install = Command::new("rustup");
        let _install = install.args([
            "toolchain",
            "install",
            &spec.channel,
            "--profile",
            &spec.profile,
        ]);
        if !spec.components.is_empty() {
            let _install = install.arg("--component").arg(spec.components.join(","));
        }
        execute(&mut install, "rustup toolchain install")?;
        if !spec.targets.is_empty() {
            let mut targets = Command::new("rustup");
            let _targets = targets
                .args(["target", "add", "--toolchain", &spec.channel])
                .args(&spec.targets);
            execute(&mut targets, "rustup target add")?;
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        let spec = &self.toolchain;
        for (name, value) in [("channel", &spec.channel), ("profile", &spec.profile)] {
            if value.trim().is_empty() {
                return Err(Error::Contract(format!("toolchain.{name} is empty")));
            }
        }
        if spec.components.iter().any(String::is_empty) || spec.targets.iter().any(String::is_empty)
        {
            return Err(Error::Contract(
                "toolchain components and targets must be nonempty".to_owned(),
            ));
        }
        Ok(())
    }
}

fn minimal() -> String {
    "minimal".to_owned()
}

fn execute(command: &mut Command, name: &str) -> Result<()> {
    let status = command.status().map_err(|source| Error::Spawn {
        command: name.to_owned(),
        source,
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Contract(format!("{name} failed with {status}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_toolchain_contract_parses() {
        let file = toml::from_str::<ToolchainFile>(
            "[toolchain]\nchannel = \"1.97.1\"\nprofile = \"minimal\"\ncomponents = [\"clippy\"]\n",
        )
        .expect("parse toolchain");
        file.validate().expect("validate toolchain");
        assert_eq!(file.toolchain.channel, "1.97.1");
    }
}
