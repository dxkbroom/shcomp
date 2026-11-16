//! Normalize the strings users write to identify tools and workspaces.

use eyre::{Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Host {
    GitHub,
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    Bare,
    Prefix,
    Url,
    Ssh,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Repo {
    pub owner: String,
    pub name: String,
}

impl Repo {
    pub fn new(owner: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            name: name.into(),
        }
    }

    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub raw: String,
    pub host: Option<Host>,
    pub repo: Option<Repo>,
    pub tool: Option<String>,
}

impl Reference {
    pub fn parse(input: &str) -> Result<Self> {
        let raw = input.trim();
        if raw.is_empty() {
            bail!("empty tool reference");
        }

        let (host, rest, form) = split_host(raw)?;
        let rest = rest.trim().trim_end_matches('/');
        let rest = rest.strip_suffix(".git").unwrap_or(rest);
        let parts: Vec<&str> = rest.split('/').filter(|part| !part.is_empty()).collect();

        match parts.as_slice() {
            [] => bail!("invalid tool reference `{raw}`"),
            [name] => Ok(Self {
                raw: raw.to_string(),
                host: None,
                repo: None,
                tool: Some((*name).to_string()),
            }),
            [owner, name] => Ok(Self {
                raw: raw.to_string(),
                host: host.or(Some(Host::GitHub)),
                repo: Some(Repo::new(*owner, *name)),
                tool: None,
            }),
            [owner, name, tool, ..] => {
                // A URL or SSH path may carry `/tree/main` or similar. Only a
                // bare path or an explicit `gh:`/`github:` prefix names a
                // tool after the workspace.
                let tool = matches!(form, Form::Bare | Form::Prefix).then(|| (*tool).to_string());
                Ok(Self {
                    raw: raw.to_string(),
                    host: host.or(Some(Host::GitHub)),
                    repo: Some(Repo::new(*owner, *name)),
                    tool,
                })
            }
        }
    }
}

/// Parse a registry item's `repo` field into `owner/name`.
pub fn normalize_repo(input: &str) -> Result<Repo> {
    let reference = Reference::parse(input)?;
    match reference.repo {
        Some(repo) => match reference.host {
            Some(Host::GitHub) | None => Ok(repo),
            Some(Host::Other(host)) => bail!("unsupported host `{host}` in `{input}`"),
        },
        None => bail!("`{input}` is not a repository"),
    }
}

fn split_host(input: &str) -> Result<(Option<Host>, String, Form)> {
    for (prefix, host) in [
        ("gh:", Host::GitHub),
        ("github:", Host::GitHub),
        ("gitlab:", Host::Other("gitlab.com".to_string())),
        ("codeberg:", Host::Other("codeberg.org".to_string())),
    ] {
        if let Some(rest) = input.strip_prefix(prefix) {
            return Ok((Some(host), rest.to_string(), Form::Prefix));
        }
    }

    if let Some(rest) = input.strip_prefix("git@") {
        let (host, path) = rest
            .split_once(':')
            .ok_or_else(|| eyre::eyre!("invalid SSH repository `{input}`"))?;
        return Ok((Some(host_from_name(host)), path.to_string(), Form::Ssh));
    }

    if input.starts_with("https://") || input.starts_with("http://") || input.starts_with("ssh://")
    {
        let url = url::Url::parse(input)?;
        let host = url
            .host_str()
            .ok_or_else(|| eyre::eyre!("repository URL `{input}` has no host"))?
            .to_string();
        let path = url.path().trim_start_matches('/').to_string();
        return Ok((Some(host_from_name(&host)), path, Form::Url));
    }

    Ok((None, input.to_string(), Form::Bare))
}

fn host_from_name(host: &str) -> Host {
    match host {
        "github.com" | "www.github.com" => Host::GitHub,
        other => Host::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_name_is_a_tool() {
        let reference = Reference::parse("gh").unwrap();
        assert_eq!(reference.tool.as_deref(), Some("gh"));
        assert!(reference.repo.is_none());
    }

    #[test]
    fn owner_repo_is_a_workspace() {
        let reference = Reference::parse("cli/cli").unwrap();
        assert_eq!(reference.repo, Some(Repo::new("cli", "cli")));
        assert!(reference.tool.is_none());
    }

    #[test]
    fn owner_repo_tool_selects_a_tool() {
        let reference = Reference::parse("cli/cli/gh").unwrap();
        assert_eq!(reference.repo, Some(Repo::new("cli", "cli")));
        assert_eq!(reference.tool.as_deref(), Some("gh"));
    }

    #[test]
    fn github_forms_normalize_to_the_same_workspace() {
        for input in [
            "https://github.com/cli/cli",
            "https://github.com/cli/cli.git",
            "http://github.com/cli/cli/",
            "git@github.com:cli/cli.git",
            "gh:cli/cli",
            "github:cli/cli",
            "cli/cli",
        ] {
            let reference = Reference::parse(input).unwrap();
            assert_eq!(
                reference.repo,
                Some(Repo::new("cli", "cli")),
                "input: {input}"
            );
        }
    }

    #[test]
    fn host_prefix_can_select_a_tool() {
        let reference = Reference::parse("gh:cli/cli/gh").unwrap();
        assert_eq!(reference.repo, Some(Repo::new("cli", "cli")));
        assert_eq!(reference.tool.as_deref(), Some("gh"));
    }

    #[test]
    fn url_paths_do_not_become_tool_names() {
        let reference = Reference::parse("https://github.com/cli/cli/tree/main").unwrap();
        assert_eq!(reference.repo, Some(Repo::new("cli", "cli")));
        assert!(reference.tool.is_none());
    }

    #[test]
    fn other_hosts_are_reported() {
        let reference = Reference::parse("https://gitlab.com/owner/repo").unwrap();
        assert_eq!(reference.host, Some(Host::Other("gitlab.com".to_string())));
    }

    #[test]
    fn normalize_repo_rejects_other_hosts() {
        assert!(normalize_repo("https://gitlab.com/owner/repo").is_err());
        assert_eq!(
            normalize_repo("https://github.com/cli/cli").unwrap(),
            Repo::new("cli", "cli")
        );
    }
}
