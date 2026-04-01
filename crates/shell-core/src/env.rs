use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Default, Clone)]
pub struct ShellEnv {
    pub vars: HashMap<String, String>,
    pub cwd: PathBuf,
    pub aliases: HashMap<String, String>,
}

impl ShellEnv {
    pub fn new() -> anyhow::Result<Self> {
        #[allow(unused_mut)]
        let mut env = Self {
            vars: std::env::vars().collect(),
            cwd: std::env::current_dir()?,
            aliases: HashMap::new(),
        };

        #[cfg(windows)]
        {
            // obvious aliases since Windows ships different command names
            env.set_alias("ls", "dir");
            env.set_alias("cat", "type");
            env.set_alias("clear", "cls");
        }

        Ok(env)
    }

    pub fn get(&self, k: &str) -> Option<&str> {
        self.vars.get(k).map(|s| s.as_str())
    }

    pub fn set(&mut self, k: impl Into<String>, v: impl Into<String>) {
        let k = k.into();
        let v = v.into();
        std::env::set_var(&k, &v);
        self.vars.insert(k, v);
    }

    pub fn unset(&mut self, k: &str) {
        std::env::remove_var(k);
        self.vars.remove(k);
    }

    pub fn chdir(&mut self, to: impl Into<PathBuf>) -> anyhow::Result<()> {
        let to = to.into();
        std::env::set_current_dir(&to)?;
        self.cwd = std::env::current_dir()?;
        Ok(())
    }

    pub fn set_alias(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.aliases.insert(name.into(), value.into());
    }

    pub fn unset_alias(&mut self, name: &str) {
        self.aliases.remove(name);
    }

    pub fn get_alias(&self, name: &str) -> Option<&str> {
        self.aliases.get(name).map(|s| s.as_str())
    }

    pub fn list_aliases(&self) -> Vec<(String, String)> {
        let mut v: Vec<_> = self.aliases.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }
}
