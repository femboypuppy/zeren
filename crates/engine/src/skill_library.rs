//! GitHub Agent Skills installation. Copies skill folders only; no repository
//! commands, hooks, package managers or setup scripts run during installation.
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

const RECEIPT: &str = ".zeren-skill.json";
const MAX_FILES: usize = 4096;
const MAX_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledSkill {
    pub name: String,
    pub description: String,
    pub repository: String,
    pub revision: String,
    pub enabled: bool,
}

#[derive(Debug)]
struct Source {
    repository: String,
    reference: Option<String>,
    subdir: PathBuf,
}
fn source(value: &str) -> anyhow::Result<Source> {
    let value = value.trim().trim_end_matches('/');
    let value = value.strip_prefix("https://github.com/").unwrap_or(value);
    let parts: Vec<_> = value.split('/').collect();
    let valid = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    if parts.len() < 2
        || !parts.iter().all(|part| valid(part))
        || (parts.len() > 2 && (parts.len() < 4 || parts[2] != "tree"))
    {
        bail!("Use a GitHub repository URL, owner/repo, or a GitHub tree URL.");
    }
    let repository = format!(
        "https://github.com/{}/{}",
        parts[0],
        parts[1].trim_end_matches(".git")
    );
    Ok(Source {
        repository,
        reference: parts.get(3).map(|s| s.to_string()),
        subdir: parts.iter().skip(4).collect(),
    })
}

pub fn root() -> PathBuf {
    crate::repos::home_dir().join(".agents/skills")
}
fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
fn plain_dir(path: &Path) -> anyhow::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("Skill folder must be a real directory: {}", path.display());
    }
    Ok(())
}
pub fn list_at(root: &Path) -> anyhow::Result<Vec<InstalledSkill>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut skills = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| valid_name(name))
        else {
            continue;
        };
        if let Ok(bytes) = std::fs::read(path.join(RECEIPT))
            && let Ok(mut skill) = serde_json::from_slice::<InstalledSkill>(&bytes)
        {
            if skill.name != name {
                continue;
            }
            skill.enabled = path.join("SKILL.md").is_file();
            skills.push(skill);
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skills)
}

pub async fn install(value: &str) -> anyhow::Result<Vec<InstalledSkill>> {
    let source = source(value)?;
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("repository");
    let mut git = tokio::process::Command::new("git");
    git.args([
        "-c",
        "core.hooksPath=",
        "clone",
        "--depth=1",
        "--single-branch",
    ]);
    if let Some(reference) = &source.reference {
        git.args(["--branch", reference]);
    }
    git.arg("--")
        .arg(format!("{}.git", source.repository))
        .arg(&checkout)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    git.creation_flags(0x08000000);
    let output = tokio::time::timeout(Duration::from_secs(90), git.output())
        .await
        .context("GitHub download timed out. Try again.")?
        .context("Git is required to import skills.")?;
    if !output.status.success() {
        bail!(
            "Couldn't download that GitHub repository: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(600)
                .collect::<String>()
        );
    }
    let mut command = tokio::process::Command::new("git");
    command.arg("-C").arg(&checkout).args(["rev-parse", "HEAD"]);
    let revision = revision(command).await?;
    let root = root();
    tokio::task::spawn_blocking(move || {
        let _temp = temp;
        let selected = checkout.join(&source.subdir).canonicalize()?;
        if !selected.starts_with(checkout.canonicalize()?) {
            bail!("Skill source must stay inside the downloaded repository.");
        }
        install_checkout(&selected, &root, &source.repository, &revision)
    })
    .await?
}
async fn revision(mut command: tokio::process::Command) -> anyhow::Result<String> {
    command.stdin(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = tokio::time::timeout(Duration::from_secs(10), command.output()).await??;
    if !output.status.success() {
        bail!("Couldn't identify downloaded revision.");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn discover(
    path: &Path,
    depth: usize,
    visited: &mut usize,
    found: &mut Vec<PathBuf>,
) -> anyhow::Result<()> {
    plain_dir(path)?;
    *visited += 1;
    if *visited > MAX_FILES {
        bail!("Repository has too many folders.");
    }
    if path.join("SKILL.md").is_file() {
        found.push(path.to_owned());
        return Ok(());
    }
    if depth >= 5 {
        return Ok(());
    }
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name();
        if matches!(
            name.to_str(),
            Some(".git" | "node_modules" | ".venv" | "target")
        ) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            discover(&entry.path(), depth + 1, visited, found)?;
        }
    }
    Ok(())
}
fn copy_tree(
    source: &Path,
    target: &Path,
    files: &mut usize,
    bytes: &mut u64,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            bail!("Skill contains a symbolic link: {}", entry.path().display());
        }
        if entry.file_name() == ".git" {
            continue;
        }
        *files += 1;
        if *files > MAX_FILES {
            bail!("Skills contain too many files.");
        }
        let destination = target.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &destination, files, bytes)?;
        } else if kind.is_file() {
            *bytes += entry.metadata()?.len();
            if *bytes > MAX_BYTES {
                bail!("Skill files exceed the 32 MB import limit.");
            }
            std::fs::copy(entry.path(), destination)?;
        } else {
            bail!("Unsupported skill file.");
        }
    }
    Ok(())
}
fn install_checkout(
    checkout: &Path,
    root: &Path,
    repository: &str,
    revision: &str,
) -> anyhow::Result<Vec<InstalledSkill>> {
    let mut found = Vec::new();
    // Prefer the canonical library instead of importing adapter copies or test
    // fixtures with the same skill names from elsewhere in the repository.
    let library = if checkout.join("SKILL.md").is_file() {
        checkout.to_owned()
    } else if checkout.join("skills").is_dir() {
        checkout.join("skills")
    } else if checkout.join(".agents/skills").is_dir() {
        checkout.join(".agents/skills")
    } else {
        checkout.to_owned()
    };
    if !library
        .canonicalize()?
        .starts_with(checkout.canonicalize()?)
    {
        bail!("Skill library points outside the downloaded repository.");
    }
    discover(&library, 0, &mut 0, &mut found)?;
    if found.is_empty() {
        bail!("No SKILL.md files found. Choose a repository containing Agent Skills.");
    }
    std::fs::create_dir_all(root)?;
    let stage = tempfile::tempdir_in(root)?;
    let mut skills = Vec::new();
    let mut files = 0;
    let mut bytes = 0;
    for directory in found {
        let path = directory.join("SKILL.md");
        if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
            bail!("SKILL.md cannot be a symbolic link.");
        }
        let metadata = zeren_harness::read_skill_metadata(&path)?.context(
            "Invalid skill metadata. SKILL.md must contain valid Agent Skills frontmatter.",
        )?;
        let name = metadata.name;
        if !valid_name(&name) {
            bail!("Unsupported skill name: {name}");
        }
        // Many repositories ship duplicate adapters. Identical names are installed once.
        if skills
            .iter()
            .any(|skill: &InstalledSkill| skill.name == name)
        {
            continue;
        }
        if root.join(&name).exists() {
            bail!("Skill '{name}' already exists. Remove it before importing a replacement.");
        }
        let target = stage.path().join(&name);
        copy_tree(&directory, &target, &mut files, &mut bytes)?;
        let skill = InstalledSkill {
            name,
            description: metadata.description,
            repository: repository.into(),
            revision: revision.into(),
            enabled: true,
        };
        std::fs::write(target.join(RECEIPT), serde_json::to_vec_pretty(&skill)?)?;
        skills.push(skill);
    }
    let mut installed = Vec::new();
    for skill in &skills {
        let destination = root.join(&skill.name);
        if let Err(error) = std::fs::rename(stage.path().join(&skill.name), &destination) {
            for path in installed {
                let _ = std::fs::remove_dir_all(path);
            }
            return Err(error.into());
        }
        installed.push(destination);
    }
    Ok(skills)
}
pub fn change_at(
    root: &Path,
    name: &str,
    enabled: Option<bool>,
) -> anyhow::Result<Vec<InstalledSkill>> {
    if !valid_name(name) {
        bail!("Invalid skill name.");
    }
    let path = root.join(name);
    plain_dir(&path)?;
    let receipt: InstalledSkill = serde_json::from_slice(&std::fs::read(path.join(RECEIPT))?)?;
    if receipt.name != name {
        bail!("This skill is not managed by Zeren.");
    }
    match enabled {
        Some(enabled) => {
            let (from, to) = if enabled {
                ("SKILL.disabled.md", "SKILL.md")
            } else {
                ("SKILL.md", "SKILL.disabled.md")
            };
            if !path.join(to).exists() {
                std::fs::rename(path.join(from), path.join(to))?;
            }
        }
        None => {
            std::fs::remove_dir_all(&path)?;
        }
    }
    list_at(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_github_sources_and_rejects_ambiguous_or_unsafe_paths() {
        assert_eq!(
            source("https://github.com/DietrichGebert/ponytail")
                .unwrap()
                .repository,
            "https://github.com/DietrichGebert/ponytail"
        );
        assert_eq!(
            source("owner/repo/tree/main/skills/review").unwrap().subdir,
            Path::new("skills/review")
        );
        for bad in [
            "https://example.com/a/b",
            "owner/repo/tree/main/../escape",
            "owner/repo;whoami",
            "owner/repo?token=secret",
            "file:///repo",
        ] {
            assert!(source(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn installs_resources_and_toggle_and_remove_only_managed_skills() {
        let repo = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let skill = repo.path().join("skills/ponytail");
        std::fs::create_dir_all(skill.join("references")).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: ponytail\ndescription: >-\n  Read before\n  editing\n---\nInstructions",
        )
        .unwrap();
        std::fs::write(skill.join("references/notes.md"), "resource").unwrap();
        let installed = install_checkout(
            repo.path(),
            target.path(),
            "https://github.com/owner/repo",
            "revision",
        )
        .unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].description, "Read before editing");
        assert!(target.path().join("ponytail/references/notes.md").is_file());
        assert!(install_checkout(repo.path(), target.path(), "repo", "revision").is_err());
        assert!(!change_at(target.path(), "ponytail", Some(false)).unwrap()[0].enabled);
        assert!(change_at(target.path(), "ponytail", Some(true)).unwrap()[0].enabled);
        assert!(
            change_at(target.path(), "ponytail", None)
                .unwrap()
                .is_empty()
        );
        assert!(change_at(target.path(), "../escape", None).is_err());
        std::fs::create_dir(target.path().join("unmanaged")).unwrap();
        assert!(change_at(target.path(), "unmanaged", None).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_library_symlink_outside_the_repository() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), repo.path().join("skills")).unwrap();
        assert!(install_checkout(repo.path(), target.path(), "repo", "revision").is_err());
        assert_eq!(std::fs::read_dir(target.path()).unwrap().count(), 0);
    }
}
