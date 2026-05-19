mod cli;
mod depgraph;
mod error;
mod query;

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
        Some(applet) => run_applet(applet, &cli),
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

fn run_applet(applet: &Applet, globals: &cli::Cli) -> Result<()> {
    match applet {
        Applet::Ebuild { ebuild_path, phase } => {
            eprintln!("ebuild: path={} phases={:?}", ebuild_path, phase);
            Err(error::Error::NotImplemented("ebuild".into()))
        }
        Applet::Maint { command } => run_maint(command),
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
        Applet::Regen { repos } => {
            eprintln!("regen: repos={:?}", repos);
            Err(error::Error::NotImplemented("regen".into()))
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
        Applet::Use { args } => {
            eprintln!("use: args={:?}", args);
            Err(error::Error::NotImplemented("use".into()))
        }
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
        Applet::File { paths } => {
            eprintln!("file: paths={:?}", paths);
            Err(error::Error::NotImplemented("file".into()))
        }
        Applet::List { atoms } => {
            let parsed = parse_atoms(atoms);
            eprintln!("list: atoms={:?}", parsed);
            Err(error::Error::NotImplemented("list".into()))
        }
        Applet::Size { atoms } => {
            let parsed = parse_atoms(atoms);
            eprintln!("size: atoms={:?}", parsed);
            Err(error::Error::NotImplemented("size".into()))
        }
        Applet::Check { atoms } => {
            let parsed = parse_atoms(atoms);
            eprintln!("check: atoms={:?}", parsed);
            Err(error::Error::NotImplemented("check".into()))
        }
        Applet::Log { command } => run_log(command),
        Applet::Grep { pattern, paths } => {
            eprintln!("grep: pattern={} paths={:?}", pattern, paths);
            Err(error::Error::NotImplemented("grep".into()))
        }
        Applet::Search {
            pattern,
            description,
        } => {
            eprintln!("search: pattern={} description={}", pattern, description);
            Err(error::Error::NotImplemented("search".into()))
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

fn run_maint(command: &Option<MaintCommand>) -> Result<()> {
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
        Some(MaintCommand::Moveinst) => Err(error::Error::NotImplemented("emaint moveinst".into())),
        Some(MaintCommand::Revisions) => {
            Err(error::Error::NotImplemented("emaint revisions".into()))
        }
        Some(MaintCommand::Sync { repos }) => {
            eprintln!("emaint: sync repos={:?}", repos);
            Err(error::Error::NotImplemented("emaint sync".into()))
        }
        Some(MaintCommand::World) => Err(error::Error::NotImplemented("emaint world".into())),
    }
}

fn run_query(command: &QueryCommand, globals: &cli::Cli) -> Result<()> {
    match command {
        QueryCommand::Belongs { file } => {
            eprintln!("equery belongs: {:?}", file);
            Err(error::Error::NotImplemented("equery belongs".into()))
        }
        QueryCommand::Check { atom } => {
            let parsed = parse_atoms(atom);
            eprintln!("equery check: {:?}", parsed);
            Err(error::Error::NotImplemented("equery check".into()))
        }
        QueryCommand::Depends { atom } => {
            let parsed = parse_atoms(atom);
            eprintln!("equery depends: {:?}", parsed);
            Err(error::Error::NotImplemented("equery depends".into()))
        }
        QueryCommand::Depgraph { atom } => {
            let parsed = parse_atoms(atom);
            let atoms: Vec<String> = parsed.iter().map(|d| d.to_string()).collect();
            if atoms.is_empty() {
                return Err(error::Error::NotImplemented("equery depgraph: no valid atoms".into()));
            }
            let repo_path = std::path::PathBuf::from(&globals.repo);
            if !repo_path.is_dir() {
                return Err(error::Error::Other(
                    format!("repo not found at {}", globals.repo),
                ));
            }
            depgraph::depgraph(&repo_path, &atoms, &globals.arch, None)
        }
        QueryCommand::Files { atom } => {
            let parsed = parse_atoms(atom);
            eprintln!("equery files: {:?}", parsed);
            Err(error::Error::NotImplemented("equery files".into()))
        }
        QueryCommand::Has { atom } => {
            let parsed = parse_atoms(atom);
            eprintln!("equery has: {:?}", parsed);
            Err(error::Error::NotImplemented("equery has".into()))
        }
        QueryCommand::Hasuse { flag } => {
            eprintln!("equery hasuse: {:?}", flag);
            Err(error::Error::NotImplemented("equery hasuse".into()))
        }
        QueryCommand::Keywords { atom } => {
            let parsed = parse_atoms(atom);
            eprintln!("equery keywords: {:?}", parsed);
            Err(error::Error::NotImplemented("equery keywords".into()))
        }
        QueryCommand::List { pattern } => {
            query::list::run(&std::path::PathBuf::from(&globals.repo), pattern)
        }
        QueryCommand::Meta { atom } => {
            let parsed = parse_atoms(atom);
            eprintln!("equery meta: {:?}", parsed);
            Err(error::Error::NotImplemented("equery meta".into()))
        }
        QueryCommand::Size { atom } => {
            let parsed = parse_atoms(atom);
            eprintln!("equery size: {:?}", parsed);
            Err(error::Error::NotImplemented("equery size".into()))
        }
        QueryCommand::Uses { atom } => {
            let parsed = parse_atoms(atom);
            eprintln!("equery uses: {:?}", parsed);
            Err(error::Error::NotImplemented("equery uses".into()))
        }
        QueryCommand::Which { atom } => {
            query::which::run(&std::path::PathBuf::from(&globals.repo), atom)
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

fn run_atom(atoms: &[String]) -> Result<()> {
    for raw in atoms {
        match portage_atom::Dep::from_str(raw) {
            Ok(dep) => println!("{dep}"),
            Err(e) => eprintln!("error: '{}': {}", raw, e),
        }
    }
    Ok(())
}
