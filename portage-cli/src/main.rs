mod cli;
mod depgraph;
mod error;
mod maint;
mod query;
mod regen;
mod search;
mod use_flags;
mod vdb;

#[cfg(feature = "mimalloc")]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::str::FromStr;

use clap::Parser;
use error::Result;

use cli::{Applet, CleanTarget, GlsaCommand, LogCommand, MaintCommand, NewsCommand, QueryCommand};

fn parse_atoms(raw: &[String]) -> Vec<portage_atom::Dep> {
    raw.iter()
        .filter_map(|s| match portage_atom::Dep::from_str(s) {
            Ok(dep) => Some(dep),
            Err(e) => {
                eprintln!("warning: skipping invalid atom '{}': {}", s, e);
                None
            }
        })
        .collect()
}

#[tokio::main]
async fn main() {
    let cli = cli::Cli::parse();

    let result = match &cli.applet {
        Some(applet) => run_applet(applet, &cli).await,
        None => {
            if cli.atoms.is_empty() {
                eprintln!("em: no atoms or applet specified. Use --help for usage.");
                std::process::exit(1);
            }
            run_emerge(&cli)
        }
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run_emerge(cli: &cli::Cli) -> Result<()> {
    let atoms = parse_atoms(&cli.atoms);
    if cli.pretend || cli.verbose {
        for atom in &atoms {
            println!("{atom}");
        }
    }
    Err(error::Error::NotImplemented("emerge".into()))
}

async fn run_applet(applet: &Applet, globals: &cli::Cli) -> Result<()> {
    match applet {
        Applet::Ebuild { ebuild_path, phase } => {
            eprintln!("ebuild: path={} phases={:?}", ebuild_path, phase);
            Err(error::Error::NotImplemented("ebuild".into()))
        }
        Applet::Maint { command } => run_maint(command, globals),
        Applet::Portageq { command, args } => {
            eprintln!("portageq: command={} args={:?}", command, args);
            Err(error::Error::NotImplemented("portageq".into()))
        }
        Applet::Sync { repos } => {
            eprintln!("sync: repos={:?}", repos);
            Err(error::Error::NotImplemented("sync".into()))
        }
        Applet::Depclean { atoms } => {
            let parsed = parse_atoms(atoms);
            eprintln!("depclean: atoms={:?}", parsed);
            Err(error::Error::NotImplemented("depclean".into()))
        }
        Applet::Regen {
            repos,
            output,
            repos_dir,
            jobs,
            dedup,
        } => {
            let resolved = globals.repo_path();
            let repo_path = repos.first().map(|r| r.as_str()).unwrap_or(&resolved);
            regen::run(
                repo_path,
                repos_dir.as_deref(),
                output.clone(),
                *jobs,
                *dedup,
            )
            .await
        }
        Applet::Quickpkg { atoms } => {
            let parsed = parse_atoms(atoms);
            eprintln!("quickpkg: atoms={:?}", parsed);
            Err(error::Error::NotImplemented("quickpkg".into()))
        }
        Applet::Mirror { args } => {
            eprintln!("mirror: args={:?}", args);
            Err(error::Error::NotImplemented("mirror".into()))
        }
        Applet::Query { command } => run_query(command, globals),
        Applet::Clean { target } => run_clean(target),
        Applet::Use {
            add,
            remove,
            make_conf,
        } => use_flags::run(add, remove, make_conf.as_deref()),
        Applet::Revdep { args } => {
            eprintln!("revdep: args={:?}", args);
            Err(error::Error::NotImplemented("revdep".into()))
        }
        Applet::Read { args } => {
            eprintln!("read: args={:?}", args);
            Err(error::Error::NotImplemented("read".into()))
        }
        Applet::News { command } => run_news(command),
        Applet::Glsa { command } => run_glsa(command),
        Applet::Log { command } => run_log(command),
        Applet::Grep { pattern, paths } => {
            eprintln!("grep: pattern={} paths={:?}", pattern, paths);
            Err(error::Error::NotImplemented("grep".into()))
        }
        Applet::Search {
            all,
            desc,
            name_only,
            homepage,
            pattern,
        } => {
            search::run(
                &globals.search_repos(),
                pattern.as_deref(),
                *all,
                *desc,
                *name_only,
                *homepage,
            )
            .await
        }
        Applet::Atom { atoms } => run_atom(atoms),
        Applet::Select { module, args } => {
            eprintln!("select: module={} args={:?}", module, args);
            Err(error::Error::NotImplemented("select".into()))
        }
        Applet::Dispatch => {
            eprintln!("dispatch-conf");
            Err(error::Error::NotImplemented("dispatch-conf".into()))
        }
        Applet::Etc => {
            eprintln!("etc-update");
            Err(error::Error::NotImplemented("etc-update".into()))
        }
        Applet::Env => {
            eprintln!("env-update");
            Err(error::Error::NotImplemented("env-update".into()))
        }
    }
}

fn run_maint(command: &Option<MaintCommand>, globals: &cli::Cli) -> Result<()> {
    match command {
        None => Err(error::Error::NotImplemented(
            "emaint (no subcommand)".into(),
        )),
        Some(MaintCommand::All) => Err(error::Error::NotImplemented("emaint all".into())),
        Some(MaintCommand::Binhost) => Err(error::Error::NotImplemented("emaint binhost".into())),
        Some(MaintCommand::Cleanconfmem) => {
            Err(error::Error::NotImplemented("emaint cleanconfmem".into()))
        }
        Some(MaintCommand::Cleanresume) => {
            Err(error::Error::NotImplemented("emaint cleanresume".into()))
        }
        Some(MaintCommand::Logs) => Err(error::Error::NotImplemented("emaint logs".into())),
        Some(MaintCommand::Merges) => Err(error::Error::NotImplemented("emaint merges".into())),
        Some(MaintCommand::Movebin) => Err(error::Error::NotImplemented("emaint movebin".into())),
        Some(MaintCommand::Moveinst) => {
            let vdb = open_vdb(globals)?;
            let resolved = globals.repo_path();
            let repo_path = camino::Utf8Path::new(&resolved);
            maint::moveinst::run(repo_path, &vdb)
        }
        Some(MaintCommand::RegenUse) => {
            let resolved = globals.repo_path();
            let repo_path = camino::Utf8Path::new(&resolved);
            let repo = portage_repo::Repository::open(repo_path)
                .map_err(|e| error::Error::Other(e.to_string()))?;
            let use_db = portage_repo::UseDb::build_local_from_repo(&repo)
                .map_err(|e| error::Error::Other(e.to_string()))?;
            let out_path = repo.path().join("profiles/use.local.desc");
            use_db
                .write_use_local_desc(&out_path)
                .map_err(|e| error::Error::Other(e.to_string()))?;
            let count: usize = use_db
                .packages_with_local_flags()
                .map(|_| 1)
                .sum();
            println!("Wrote {count} packages to {out_path}.");
            Ok(())
        }
        Some(MaintCommand::Revisions { repos }) => {
            let root = globals.root.as_deref().map(camino::Utf8Path::new);
            maint::revisions::run(repos, root)
        }
        Some(MaintCommand::Sync { repos }) => {
            eprintln!("emaint: sync repos={:?}", repos);
            Err(error::Error::NotImplemented("emaint sync".into()))
        }
        Some(MaintCommand::World { fix }) => {
            let vdb = open_vdb(globals)?;
            let root = globals.root.as_deref().map(camino::Utf8Path::new);
            maint::world::run(&vdb, *fix, root)
        }
    }
}

fn run_query(command: &QueryCommand, globals: &cli::Cli) -> Result<()> {
    match command {
        QueryCommand::Belongs { file } => {
            let vdb = open_vdb(globals)?;
            vdb::query_belongs(&vdb, file)
        }
        QueryCommand::Check { atom } => {
            let vdb = open_vdb(globals)?;
            query::check::run(&vdb, atom)
        }
        QueryCommand::Depends { atom } => {
            query::depends::run(&std::path::PathBuf::from(globals.repo_path()), atom)
        }
        QueryCommand::Depgraph { atom } => {
            let parsed = parse_atoms(atom);
            let atoms: Vec<String> = parsed.iter().map(|d| d.to_string()).collect();
            if atoms.is_empty() {
                return Err(error::Error::NotImplemented(
                    "equery depgraph: no valid atoms".into(),
                ));
            }
            let resolved = globals.repo_path();
            let repo_path = std::path::Path::new(&resolved);
            if !repo_path.is_dir() {
                return Err(error::Error::Other(format!("repo not found at {resolved}")));
            }
            depgraph::depgraph(repo_path, &atoms, &globals.arch, None)
        }
        QueryCommand::Files { atom } => {
            let vdb = open_vdb(globals)?;
            vdb::query_files(&vdb, atom)
        }
        QueryCommand::Has { atom } => {
            let vdb = open_vdb(globals)?;
            query::has::run(&vdb, atom)
        }
        QueryCommand::Hasuse { flag } => {
            query::hasuse::run(&std::path::PathBuf::from(globals.repo_path()), flag)
        }
        QueryCommand::Keywords { atom } => {
            query::keywords::run(&std::path::PathBuf::from(globals.repo_path()), atom)
        }
        QueryCommand::List { installed, pattern } => {
            if *installed {
                let vdb = open_vdb(globals)?;
                query::list::run_installed(&vdb, pattern)
            } else {
                query::list::run(&std::path::PathBuf::from(globals.repo_path()), pattern)
            }
        }
        QueryCommand::Meta { atom } => {
            let vdb = open_vdb(globals).ok();
            query::meta::run(
                &std::path::PathBuf::from(globals.repo_path()),
                vdb.as_ref(),
                atom,
            )
        }
        QueryCommand::Size { atom } => {
            let vdb = open_vdb(globals)?;
            vdb::query_size(&vdb, atom)
        }
        QueryCommand::Uses { atom } => {
            let vdb = open_vdb(globals).ok();
            query::uses::run(
                &std::path::PathBuf::from(globals.repo_path()),
                vdb.as_ref(),
                atom,
            )
        }
        QueryCommand::Which { atom } => {
            query::which::run(&std::path::PathBuf::from(globals.repo_path()), atom)
        }
    }
}

fn run_clean(target: &Option<CleanTarget>) -> Result<()> {
    match target {
        None => Err(error::Error::NotImplemented("eclean (no target)".into())),
        Some(CleanTarget::Dist) => Err(error::Error::NotImplemented("eclean dist".into())),
        Some(CleanTarget::Pkg) => Err(error::Error::NotImplemented("eclean pkg".into())),
    }
}

fn run_news(command: &Option<NewsCommand>) -> Result<()> {
    match command {
        None => Err(error::Error::NotImplemented("news (no subcommand)".into())),
        Some(NewsCommand::Count) => Err(error::Error::NotImplemented("news count".into())),
        Some(NewsCommand::List) => Err(error::Error::NotImplemented("news list".into())),
        Some(NewsCommand::Read { id }) => {
            eprintln!("news: read {:?}", id);
            Err(error::Error::NotImplemented("news read".into()))
        }
        Some(NewsCommand::Purge) => Err(error::Error::NotImplemented("news purge".into())),
    }
}

fn run_glsa(command: &Option<GlsaCommand>) -> Result<()> {
    match command {
        None => Err(error::Error::NotImplemented("glsa (no subcommand)".into())),
        Some(GlsaCommand::List) => Err(error::Error::NotImplemented("glsa list".into())),
        Some(GlsaCommand::Check { ids }) => {
            eprintln!("glsa: check {:?}", ids);
            Err(error::Error::NotImplemented("glsa check".into()))
        }
        Some(GlsaCommand::Fix { ids }) => {
            eprintln!("glsa: fix {:?}", ids);
            Err(error::Error::NotImplemented("glsa fix".into()))
        }
    }
}

fn run_log(command: &Option<LogCommand>) -> Result<()> {
    match command {
        None => Err(error::Error::NotImplemented("log (no subcommand)".into())),
        Some(LogCommand::Current) => Err(error::Error::NotImplemented("log current".into())),
        Some(LogCommand::List { limit }) => {
            eprintln!("log: list limit={:?}", limit);
            Err(error::Error::NotImplemented("log list".into()))
        }
        Some(LogCommand::Time { atom }) => {
            eprintln!("log: time atom={:?}", atom);
            Err(error::Error::NotImplemented("log time".into()))
        }
    }
}

fn open_vdb(globals: &cli::Cli) -> Result<portage_vdb::Vdb> {
    let vdb_path = globals
        .vdb
        .as_deref()
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            globals
                .root
                .as_deref()
                .map(|r| format!("{}/var/db/pkg", r.trim_end_matches('/')))
                .unwrap_or_else(|| "/var/db/pkg".to_string())
        });
    portage_vdb::Vdb::open(vdb_path.as_str())
        .map_err(|e| error::Error::Other(format!("failed to open VDB at {}: {}", vdb_path, e)))
}

fn run_atom(atoms: &[String]) -> Result<()> {
    for raw in atoms {
        match portage_atom::Dep::from_str(raw) {
            Ok(dep) => println!("{dep}"),
            Err(e) => eprintln!("error: '{}': {}", raw, e),
        }
    }
    Ok(())
}
