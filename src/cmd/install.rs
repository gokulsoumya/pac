use crate::git;
use crate::package::{self, Package};
use crate::task::{TaskManager, TaskType};
use crate::{Error, Result};

use clap::{value_t, ArgMatches};
use num_cpus;

#[derive(Debug)]
struct InstallArgs {
    plugins: Vec<String>,
    on: Option<String>,
    for_: Option<String>,
    threads: Option<usize>,
    opt: bool,
    category: String,
    build: Option<String>,
}

impl InstallArgs {
    fn from_matches(m: &ArgMatches) -> InstallArgs {
        InstallArgs {
            plugins: m.values_of_lossy("package").unwrap_or_else(|| vec![]),
            on: value_t!(m, "on", String).ok(),
            for_: value_t!(m, "for", String).ok(),
            threads: value_t!(m, "threads", usize).ok(),
            opt: m.is_present("opt"),
            category: value_t!(m, "category", String).unwrap_or_default(),
            build: value_t!(m, "build", String).ok(),
        }
    }
}

struct Plugins {
    names: Vec<String>,
    category: String,
    opt: bool,
    on: Option<String>,
    types: Option<Vec<String>>,
    build: Option<String>,
    threads: usize,
}

pub fn exec(matches: &ArgMatches) {
    let args = InstallArgs::from_matches(matches);

    let threads = match args.threads {
        Some(t) => t,
        _ => num_cpus::get(),
    };

    if threads < 1 {
        die!("Threads should be greater than 0");
    }

    let opt = args.on.is_some() || args.for_.is_some() || args.opt;
    let types = args
        .for_
        .map(|e| e.split(',').map(|e| e.to_string()).collect::<Vec<String>>());

    let plugins = Plugins {
        names: args.plugins,
        category: args.category,
        opt,
        on: args.on,
        types,
        build: args.build,
        threads,
    };

    if let Err(e) = install_plugins(&plugins) {
        die!("Err: {}", e);
    }
}

fn install_plugins(plugins: &Plugins) -> Result<()> {
    let mut packs = package::fetch()?;
    {
        let mut manager = TaskManager::new(TaskType::Install, plugins.threads);

        if plugins.names.is_empty() {
            for pack in &packs {
                manager.add(pack.clone());
            }
        } else {
            let targets = plugins.names.iter().map(|n| {
                let mut p = Package::new(n, &plugins.category, plugins.opt);
                if let Some(ref c) = plugins.on {
                    p.set_load_command(c);
                }
                if let Some(ref t) = plugins.types {
                    p.set_types(t.clone());
                }
                if let Some(ref c) = plugins.build {
                    p.set_build_command(c);
                }
                p
            });
            for mut pack in targets {
                let having = match packs.iter_mut().find(|x| x.name == pack.name) {
                    Some(x) => {
                        if !x.is_installed() {
                            x.set_category(pack.category.as_str());
                            x.set_opt(pack.opt);
                            x.set_types(pack.for_types.clone());

                            x.load_command = pack.load_command.clone();
                            x.build_command = pack.build_command.clone();
                        } else {
                            pack.set_category(x.category.as_str());
                            pack.set_opt(x.opt);
                        }
                        true
                    }
                    None => false,
                };
                if !having {
                    packs.push(pack.clone());
                }
                manager.add(pack);
            }
        }

        for fail in manager.run(install_plugin) {
            packs.retain(|e| e.name != fail);
        }
    }

    packs.sort_by(|a, b| a.name.cmp(&b.name));

    package::update_pack_plugin(&packs)?;
    package::save(packs)
}

fn install_plugin(pack: &Package) -> (Result<()>, bool) {
    let res = do_install(pack);
    let status = match res {
        Err(Error::PluginInstalled(_)) => true,
        Err(_) => false,
        _ => true,
    };
    (res, status)
}

fn do_install(pack: &Package) -> Result<()> {
    let path = pack.path();
    if path.is_dir() {
        Err(Error::plugin_installed(&path))
    } else {
        git::clone(&pack.name, &path)
    }
}
