use std::{
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
};

use toml::{Table, Value};

use crate::{
    Result,
    error::{Error, io},
};

const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
const PRUNED_DIRECTORIES: [&str; 3] = [".git", ".worktrees", "target"];

pub fn enforce(workspace: &Path) -> Result<()> {
    let root =
        fs::canonicalize(workspace).map_err(|source| io("resolve workspace", workspace, source))?;
    inspect_cargo_config(&root)?;
    inspect_tree(&root, &root)
}

fn inspect_tree(root: &Path, directory: &Path) -> Result<()> {
    let mut entries = fs::read_dir(directory)
        .map_err(|source| io("read directory", directory, source))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|source| io("read directory entry", directory, source))?;
    entries.sort_unstable_by_key(fs::DirEntry::file_name);

    for entry in entries {
        let kind = entry
            .file_type()
            .map_err(|source| io("inspect directory entry", entry.path(), source))?;
        if kind.is_dir() {
            if !PRUNED_DIRECTORIES.contains(&entry.file_name().to_string_lossy().as_ref()) {
                inspect_tree(root, &entry.path())?;
            }
        } else if kind.is_file() && entry.file_name() == OsStr::new("Cargo.toml") {
            inspect_manifest(root, &entry.path())?;
        }
    }
    Ok(())
}

fn inspect_manifest(root: &Path, manifest: &Path) -> Result<()> {
    let source = fs::read_to_string(manifest).map_err(|source| io("read", manifest, source))?;
    let document = toml::from_str::<Value>(&source).map_err(|source| Error::Toml {
        path: manifest.to_path_buf(),
        source,
    })?;
    let Some(table) = document.as_table() else {
        return Ok(());
    };

    inspect_dependency_tables(root, manifest, table)?;
    if let Some(workspace) = table.get("workspace").and_then(Value::as_table) {
        inspect_dependency_tables(root, manifest, workspace)?;
    }
    if let Some(targets) = table.get("target").and_then(Value::as_table) {
        for target in targets.values().filter_map(Value::as_table) {
            inspect_dependency_tables(root, manifest, target)?;
        }
    }
    if let Some(patches) = table.get("patch").and_then(Value::as_table) {
        for (source, dependencies) in patches {
            inspect_dependencies(root, manifest, &format!("patch.{source}"), dependencies)?;
        }
    }
    if let Some(replacements) = table.get("replace") {
        inspect_dependencies(root, manifest, "replace", replacements)?;
    }
    Ok(())
}

fn inspect_dependency_tables(root: &Path, manifest: &Path, table: &Table) -> Result<()> {
    for name in DEPENDENCY_TABLES {
        if let Some(dependencies) = table.get(name) {
            inspect_dependencies(root, manifest, name, dependencies)?;
        }
    }
    Ok(())
}

fn inspect_dependencies(
    root: &Path,
    manifest: &Path,
    table: &str,
    dependencies: &Value,
) -> Result<()> {
    let Some(dependencies) = dependencies.as_table() else {
        return Ok(());
    };
    for (name, dependency) in dependencies {
        let Some(path) = dependency
            .as_table()
            .and_then(|fields| fields.get("path"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let candidate = manifest.parent().unwrap_or(root).join(path);
        let resolved = if candidate.exists() {
            fs::canonicalize(&candidate)
                .map_err(|source| io("resolve dependency path", &candidate, source))?
        } else {
            normalize(&candidate)
        };
        if !resolved.starts_with(root) {
            return Err(Error::Manifest(format!(
                "`{}` declares `{table}.{name}` through repository-external path `{path}`",
                manifest.strip_prefix(root).unwrap_or(manifest).display()
            )));
        }
    }
    Ok(())
}

fn inspect_cargo_config(root: &Path) -> Result<()> {
    for relative in [".cargo/config.toml", ".cargo/config"] {
        let path = root.join(relative);
        if !path.is_file() {
            continue;
        }
        let source = fs::read_to_string(&path).map_err(|source| io("read", &path, source))?;
        let document = toml::from_str::<Value>(&source).map_err(|source| Error::Toml {
            path: path.clone(),
            source,
        })?;
        if document
            .get("paths")
            .and_then(Value::as_array)
            .is_some_and(|paths| !paths.is_empty())
        {
            return Err(Error::Manifest(format!(
                "`{relative}` declares Cargo `paths`; repository-local source overrides are forbidden"
            )));
        }
    }
    Ok(())
}

fn normalize(path: &Path) -> PathBuf {
    path.components()
        .fold(PathBuf::new(), |mut path, component| {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    let _ = path.pop();
                }
                component => path.push(component.as_os_str()),
            }
            path
        })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::enforce;

    #[test]
    fn repository_boundary_admits_internal_paths_and_severs_external_ones() {
        let root = tempdir().expect("create repository");
        fs::create_dir(root.path().join("inside")).expect("create internal dependency");
        let manifest = root.path().join("Cargo.toml");
        fs::write(
            &manifest,
            "[package]\nname = \"specimen\"\nversion = \"0.1.0\"\n[dependencies]\ninside = { path = \"inside\" }\n",
        )
        .expect("write internal manifest");
        enforce(root.path()).expect("admit repository-owned dependency");

        fs::write(
            manifest,
            "[package]\nname = \"specimen\"\nversion = \"0.1.0\"\n[dependencies]\noutside = { path = \"../outside\" }\n",
        )
        .expect("write external manifest");
        let error = enforce(root.path()).expect_err("sever external dependency");
        assert!(error.to_string().contains("repository-external path"));
    }
}
