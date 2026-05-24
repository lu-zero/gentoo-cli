//! Rust builtins for PMS utility functions: eapi predicates, phase setup,
//! output helpers, build helpers, and PMS 12.3 USE/has/ver functions.
//!
//! These reimplement the functionality of portage's `eapi.sh`,
//! `isolated-functions.sh`, `phase-helpers.sh`, and `phase-functions.sh`
//! without sourcing those files.  Rust builtins sidestep brush parser gaps
//! (notably bare `[[ ]]` function bodies used as EAPI predicate bodies).
//!
//! See [PMS 12.3](https://projects.gentoo.org/pms/9/pms.html#available-commands).

pub(crate) mod die;
pub(crate) mod econf;
pub(crate) mod emake;
pub(crate) mod export_functions;
pub(crate) mod has;
pub mod inherit;
pub(crate) mod output;
pub(crate) mod phase_funcs;
pub(crate) mod unpack;
pub(crate) mod use_flag;

pub(crate) use die::DieCommand;
pub(crate) use econf::EconfCommand;
pub(crate) use emake::EmakeCommand;
pub(crate) use export_functions::ExportFunctionsCommand;
pub(crate) use has::{HasCommand, HasvCommand, InIuseCommand};
pub(crate) use output::{EbeginCommand, EchoMessageCommand, EendCommand};
pub(crate) use phase_funcs::{EAPI_PREDICATE_NAMES, EapiPredicateCommand, EbuildPhaseFuncsCommand};
pub(crate) use unpack::UnpackCommand;
pub(crate) use use_flag::{UseCommand, UseEnableCommand, UseWithCommand, UsevCommand, UsexCommand};
