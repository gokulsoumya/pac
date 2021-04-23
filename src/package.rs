use crate::git::{GitReference, GitRepo};
use crate::{Error, Result};

use std::env;
use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process;

use lazy_static::lazy_static;
use yaml_rust::yaml::Hash;
use yaml_rust::{Yaml, YamlEmitter, YamlLoader};

const PAC_PLUGIN_FILENAME: &str = "_pac.vim";
const PAC_PLUGIN_HEADER: &str = "\" Generated by pac. DO NOT EDIT!

scriptencoding utf-8

function! s:do_cmd(cmd, bang, start, end, args)
    exec printf('%s%s%s %s', (a:start == a:end ? '' : (a:start.','.a:end)), a:cmd, a:bang, a:args)
endfunction
";

const PAC_CONFIG_HEADER: &[u8] = b"# vim: ft=yaml
#
# Generated by pac.

";

lazy_static! {
    static ref VIM_BASE_DIR: PathBuf = env::var("VIM_CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = dirs::home_dir().expect("No home directory found");
            home.join(".vim")
        });
    static ref VIM_PACKAGE_DIR: PathBuf = (*VIM_BASE_DIR).join("pack");
    static ref VIM_PLUGIN_DIR: PathBuf = (*VIM_BASE_DIR).join("plugin");
    static ref PAC_CONFIG_DIR: PathBuf = (*VIM_BASE_DIR).join(".pac");
    static ref PAC_CONFIG_FILE: PathBuf = (*PAC_CONFIG_DIR).join("paconfig.yaml");
}

#[derive(Debug, Clone)]
pub struct Package {
    /// Name of local directory where plugin is installed
    /// Same as repo name of remote unless installed with --as
    pub name: String,
    /// If remote is https://github.com/username/repo then idname
    /// is username/repo. Arguments to install, update, move, etc
    /// will be the idname, *not* name.
    pub idname: String,
    /// Remote url of the repo to git clone from
    pub remote: String,
    /// The branch, tag, or commit to checkout
    pub reference: Option<GitReference>,
    /// Install package under pack/<category>/
    pub category: String,
    pub opt: bool,
    /// Load this package on this command
    pub load_command: Option<String>,
    /// Load this package for these types
    pub for_types: Vec<String>,
    /// Build command for this package
    pub build_command: Option<String>,
}

impl Package {
    pub fn new(
        name: &str,
        remote: &str,
        reference: Option<GitReference>,
        category: &str,
        opt: bool,
    ) -> Package {
        Package {
            name: name.to_string(),
            idname: Self::idname_from_remote(remote),
            remote: remote.to_string(),
            reference,
            category: category.to_string(),
            opt,
            load_command: None,
            for_types: Vec::new(),
            build_command: None,
        }
    }

    /// Get username/repo from a git remote
    pub fn idname_from_remote(remote: &str) -> String {
        let parts = remote.split('/').collect::<Vec<_>>();
        parts[parts.len() - 2..].join("/")
    }

    pub fn is_installed(&self) -> bool {
        self.path().is_dir()
    }

    pub fn set_category<T: Into<String>>(&mut self, cat: T) {
        self.category = cat.into();
    }

    /// Set package to be installed under pack/*/opt
    pub fn set_opt(&mut self, opt: bool) {
        self.opt = opt;
    }

    /// Set filetype(s) to load the package for
    pub fn set_types(&mut self, types: Vec<String>) {
        self.for_types = types
    }

    /// Parse a Package from a single list item in `PAC_CONFIG_FILE`
    pub fn from_yaml(doc: &Yaml) -> Result<Package> {
        let name = doc["name"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or(Error::Format)?;
        let remote = doc["remote"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or(Error::Format)?;

        let reference = match doc["ref"].as_hash() {
            None => None,
            Some(subdoc) => {
                let (reftype, refval) = subdoc.iter().next().ok_or(Error::Format)?;
                let to_str = |yaml: &Yaml| -> Result<String> {
                    yaml.as_str().map(|s| s.to_string()).ok_or(Error::Format)
                };
                Some(GitReference::new(&to_str(reftype)?, &to_str(refval)?)?)
            },
        };

        let opt = doc["opt"].as_bool().ok_or(Error::Format)?;
        let category = doc["category"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or(Error::Format)?;
        let cmd = doc["on"].as_str().map(|s| s.to_string());
        let build = doc["build"].as_str().map(|s| s.to_string());

        let types = match doc["for"].as_vec() {
            Some(f) => {
                let mut types = Vec::with_capacity(f.len());
                for e in f {
                    types.push(e.as_str().map(|s| s.to_string()).ok_or(Error::Format)?);
                }
                types
            }
            None => vec![],
        };

        Ok(Package {
            name,
            idname: Self::idname_from_remote(&remote),
            remote,
            reference,
            category,
            opt,
            load_command: cmd,
            for_types: types,
            build_command: build,
        })
    }

    /// Convert Package to a list item to be added to `PAC_CONFIG_FILE`
    pub fn into_yaml(self) -> Yaml {
        let mut doc = Hash::new();
        doc.insert(Yaml::from_str("name"), Yaml::from_str(&self.name));
        doc.insert(Yaml::from_str("remote"), Yaml::from_str(&self.remote));
        doc.insert(Yaml::from_str("category"), Yaml::from_str(&self.category));
        doc.insert(Yaml::from_str("opt"), Yaml::Boolean(self.opt));

        if let Some(ref gitref) = self.reference {
            let mut subdoc = Hash::new();
            subdoc.insert(
                Yaml::from_str(&gitref.kind.to_string()),
                Yaml::from_str(&gitref.value),
            );
            doc.insert(Yaml::from_str("ref"), Yaml::Hash(subdoc));
        }

        if let Some(ref c) = self.load_command {
            doc.insert(Yaml::from_str("on"), Yaml::from_str(c));
        }
        if let Some(ref c) = self.build_command {
            doc.insert(Yaml::from_str("build"), Yaml::from_str(c));
        }
        if !self.for_types.is_empty() {
            let types = self
                .for_types
                .iter()
                .map(|e| Yaml::from_str(e))
                .collect::<Vec<Yaml>>();
            doc.insert(Yaml::from_str("for"), Yaml::Array(types));
        }
        Yaml::Hash(doc)
    }

    /// Returns absolute path to directory where plugin can be installed
    pub fn path(&self) -> PathBuf {
        if self.opt {
            VIM_PACKAGE_DIR
                .join(&self.category)
                .join("opt")
                .join(&self.name)
        } else {
            VIM_PACKAGE_DIR
                .join(&self.category)
                .join("start")
                .join(&self.name)
        }
    }

    /// Run the build command using `sh -c ...`
    ///
    /// # Errors
    ///
    /// If the build process returns a non zero exit status, an `Error::Build`
    /// variant will be returned along with stderr.
    pub fn try_build(&self) -> Result<()> {
        if let Some(ref c) = self.build_command {
            let path = self.path();
            let p = process::Command::new("sh")
                .arg("-c")
                .arg(c)
                .stdout(process::Stdio::piped())
                .stderr(process::Stdio::piped())
                .current_dir(&path)
                .spawn()?;
            let output = p.wait_with_output()?;
            if !output.status.success() {
                let err = String::from_utf8(output.stderr)
                    .unwrap_or_else(|_| String::from("No error output!"));
                return Err(Error::Build(err));
            }
        }
        Ok(())
    }
}

impl GitRepo for Package {
    fn clone_info(&self) -> (&str, PathBuf, Option<GitReference>) {
        (&self.remote, self.path(), self.reference.clone())
    }
}

impl fmt::Display for Package {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let name = if self.opt { "opt" } else { "start" };
        let on = match self.load_command {
            Some(ref c) => format!(" [Load on `{}`]", c),
            None => "".to_string(),
        };

        let types = if !self.for_types.is_empty() {
            let types = self.for_types.join(",");
            format!(" [For {}]", types)
        } else {
            "".to_string()
        };
        write!(
            f,
            "{} => pack/{}/{}{}{}",
            &self.idname, &self.category, name, on, types
        )
    }
}

pub fn fetch() -> Result<Vec<Package>> {
    if PAC_CONFIG_FILE.is_file() {
        fetch_from_paconfig(&*PAC_CONFIG_FILE)
            .map_err(|e| Error::PaconfigFile(format!("Fail to parse paconfig: {}", e)))
    } else {
        Ok(vec![])
    }
}

/// Returns a list of packages parsed from paconfig
fn fetch_from_paconfig<P: AsRef<Path>>(paconfig: P) -> Result<Vec<Package>> {
    let mut data = String::new();
    File::open(paconfig.as_ref())?.read_to_string(&mut data)?;
    let docs = YamlLoader::load_from_str(&data)?;

    let mut ret = Vec::new();
    if !docs.is_empty() {
        if let Some(doc) = docs[0].as_vec() {
            for d in doc {
                ret.push(Package::from_yaml(d)?);
            }
        }
    }
    Ok(ret)
}

/// Write out the yaml paconfig under `PAC_CONFIG_DIR` creating it
/// if necessary.
pub fn save(packs: Vec<Package>) -> Result<()> {
    let packs = packs
        .into_iter()
        .map(|e| e.into_yaml())
        .collect::<Vec<Yaml>>();
    let doc = Yaml::Array(packs);
    let mut out = String::new();
    {
        let mut emitter = YamlEmitter::new(&mut out);
        emitter.dump(&doc)?;
    }
    if !PAC_CONFIG_DIR.is_dir() {
        fs::create_dir_all(&*PAC_CONFIG_DIR)?;
    }
    let mut f = File::create(&*PAC_CONFIG_FILE)?;
    f.write_all(PAC_CONFIG_HEADER)?;
    f.write_all(out.as_bytes())?;
    Ok(())
}

/// Update `_pac.vim` file in plugin directory.
pub fn update_pac_plugin(packs: &[Package]) -> Result<()> {
    if !VIM_PLUGIN_DIR.is_dir() {
        fs::create_dir_all(&*VIM_PLUGIN_DIR)?;
    }

    let mut f = File::create(VIM_PLUGIN_DIR.join(PAC_PLUGIN_FILENAME))?;
    f.write_all(format!("{}\n\n", PAC_PLUGIN_HEADER).as_bytes())?;

    let mut plug_setup = String::new();
    for p in packs.iter() {
        if let Some(ref c) = p.load_command {
            plug_setup += &format!(
                "command! -nargs=* -range -bang {cmd} packadd {repo} | \
                 call s:do_cmd('{cmd}', \"<bang>\", <line1>, <line2>, <q-args>)\n\n",
                cmd = c,
                repo = p.name,
            );
        }

        if !p.for_types.is_empty() {
            plug_setup += &format!(
                "autocmd FileType {} packadd {}\n\n",
                p.for_types.join(","),
                p.name,
            );
        }

        if !plug_setup.is_empty() {
            plug_setup = format!("\" {}\n", &p.name) + &plug_setup;
            f.write_all(plug_setup.as_bytes())?;

            plug_setup.clear();
        }
    }
    Ok(())
}

fn read_dir<H>(dir: &Path, mut action: H) -> Result<()>
where
    H: FnMut(&Path, String) -> Result<()>,
{
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in dir.read_dir()? {
        if let Ok(e) = entry {
            let sub = e.path();
            let item = match sub.file_name().iter().flat_map(|s| s.to_str()).next() {
                None => continue,
                Some(i) => i.to_string(),
            };
            if sub.is_dir() && !item.starts_with('.') {
                action(&sub, item)?;
            }
        }
    }
    Ok(())
}

pub fn walk_packs<F>(category: &Option<String>, start: bool, opt: bool, callback: F) -> Result<()>
where
    F: Fn(&str, &str, &str),
{
    read_dir(&VIM_PACKAGE_DIR, |path, cate| {
        let is_match = category.as_ref().map_or(true, |c| *c == cate);
        if !is_match {
            Ok(())
        } else {
            read_dir(path, |subpath, option| {
                if (start && option != "start")
                    || (opt && option != "opt")
                    || (option != "start" && option != "opt")
                {
                    Ok(())
                } else {
                    read_dir(subpath, |_, name| {
                        callback(&cate, &option, &name);
                        Ok(())
                    })
                }
            })
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_idname_from_remote() {
        let remote = "https://github.com/username/repo";
        assert_eq!(Package::idname_from_remote(remote), "username/repo");
    }
}
