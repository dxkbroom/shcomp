//! Tera rendering with a closed context.
//!
//! The syntax is Tera v2, matching mise, so registry authors write
//! `{{ shell }}` rather than a private placeholder language. Only pure
//! helpers are registered: a template cannot execute a command or read a file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr};
use serde::Serialize;
use tera::{Context, Kwargs, State, Tera, TeraResult, Value};

#[derive(Debug, Clone, Serialize)]
pub struct TemplateContext {
    pub bin: String,
    pub root: String,
    pub version: Option<String>,
    pub command: String,
    pub shell: String,
    pub cword: usize,
    pub line: String,
    pub words: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub config_root: String,
    pub config_source: Option<String>,
}

impl TemplateContext {
    pub fn new(command: &str, shell: &str, bin: &Path) -> Self {
        Self {
            bin: bin.to_string_lossy().into_owned(),
            root: install_root(bin).to_string_lossy().into_owned(),
            version: None,
            command: command.to_string(),
            shell: shell.to_string(),
            cword: 0,
            line: String::new(),
            words: Vec::new(),
            env: std::env::vars().collect(),
            config_root: String::new(),
            config_source: None,
        }
    }

    pub fn with_config(mut self, root: &Path, source: Option<&Path>) -> Self {
        self.config_root = root.to_string_lossy().into_owned();
        self.config_source = source.map(|path| path.to_string_lossy().into_owned());
        self
    }

    pub fn with_words(mut self, words: &[String]) -> Self {
        self.line = words.join(" ");
        self.words = words.to_vec();
        self.cword = words.len().saturating_sub(1);
        self
    }

    pub fn with_version(mut self, version: Option<String>) -> Self {
        self.version = version;
        self
    }
}

/// The directory a tool's install root is likely to be.
///
/// mise installs `<root>/<version>/bin/<command>`; the version is the parent
/// of `bin`. For a system binary such as `/usr/bin/gh`, the root becomes
/// `/usr`, which is the closest useful answer.
pub fn install_root(bin: &Path) -> PathBuf {
    let parent = bin.parent().unwrap_or_else(|| Path::new("/"));
    if parent.file_name().and_then(|name| name.to_str()) == Some("bin") {
        parent.parent().unwrap_or(parent).to_path_buf()
    } else {
        parent.to_path_buf()
    }
}

pub fn render(template: &str, context: &TemplateContext) -> Result<String> {
    let mut tera = Tera::default();
    tera.autoescape_on(Vec::<&str>::new());
    register_helpers(&mut tera);
    tera.add_raw_template("shcomp", template)
        .wrap_err("invalid template")?;
    let context = Context::from_serialize(context).wrap_err("invalid template context")?;
    tera.render("shcomp", &context)
        .wrap_err("cannot render template")
}

fn register_helpers(tera: &mut Tera) {
    tera.register_function("os", |_: Kwargs, _: &State| -> TeraResult<Value> {
        Ok(Value::from(std::env::consts::OS))
    });
    tera.register_function("arch", |_: Kwargs, _: &State| -> TeraResult<Value> {
        Ok(Value::from(std::env::consts::ARCH))
    });
    tera.register_function("os_family", |_: Kwargs, _: &State| -> TeraResult<Value> {
        let family = if cfg!(windows) { "windows" } else { "unix" };
        Ok(Value::from(family))
    });
    tera.register_function(
        "get_env",
        |kwargs: Kwargs, _: &State| -> TeraResult<Value> {
            let name: String = kwargs.must_get("name")?;
            let value = std::env::var(&name).ok();
            let value = match value {
                Some(value) => Value::from(value),
                None => kwargs
                    .get::<String>("default")?
                    .map(Value::from)
                    .unwrap_or_else(Value::none),
            };
            Ok(value)
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> TemplateContext {
        TemplateContext::new("gh", "zsh", Path::new("/opt/gh/2.0.0/bin/gh"))
            .with_version(Some("2.0.0".to_string()))
            .with_words(&["gh".to_string(), "comp".to_string()])
    }

    #[test]
    fn uses_tera_syntax() {
        assert_eq!(render("{{ shell }}", &context()).unwrap(), "zsh");
        assert_eq!(
            render("{{ bin }}:{{ version }}", &context()).unwrap(),
            "/opt/gh/2.0.0/bin/gh:2.0.0"
        );
    }

    #[test]
    fn supports_filters() {
        let context = context().with_words(&["gh".to_string(), "co".to_string()]);
        assert_eq!(
            render("{{ words | last | upper }}", &context).unwrap(),
            "CO"
        );
        assert_eq!(
            render("{{ os() }}", &context).unwrap(),
            std::env::consts::OS
        );
    }

    #[test]
    fn computes_root_from_bin() {
        assert_eq!(
            install_root(Path::new("/opt/gh/2.0.0/bin/gh")),
            Path::new("/opt/gh/2.0.0")
        );
        assert_eq!(install_root(Path::new("/usr/bin/gh")), Path::new("/usr"));
    }

    #[test]
    fn does_not_expose_exec() {
        assert!(render("{{ exec(command='echo hi') }}", &context()).is_err());
    }
}
