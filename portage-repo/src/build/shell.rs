use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use camino::Utf8PathBuf;

use brush_builtins::ShellExt;
use brush_core::parser::ParserImpl;
use brush_core::{
    ProfileLoadBehavior, RcLoadBehavior, Shell, ShellValue, ShellVariable, SourceInfo,
};
use portage_metadata::{Eapi, EbuildMetadata, Phase, SrcUriEntry};

use super::commands;
use super::commands::inherit;
use super::stubs;
use super::ver_funcs;
use crate::error::{Error, Result};
use crate::repo::ebuild::Ebuild;
use crate::repo::repository::Repository;

/// Metadata variables extracted from a sourced ebuild.
///
/// These correspond to the PMS-defined metadata variables that an ebuild
/// is expected to define after being sourced.
const METADATA_VARS: &[&str] = &[
    "DESCRIPTION",
    "HOMEPAGE",
    "SRC_URI",
    "LICENSE",
    "SLOT",
    "KEYWORDS",
    "IUSE",
    "REQUIRED_USE",
    "RESTRICT",
    "PROPERTIES",
    "DEPEND",
    "RDEPEND",
    "BDEPEND",
    "PDEPEND",
    "IDEPEND",
    "INHERIT",
    "INHERITED",
];

/// Maps a portage phase name to (EBUILD_PHASE value, function name).
fn phase_to_func(phase: &str) -> (&str, &str) {
    match phase {
        "pretend" => ("pretend", "pkg_pretend"),
        "setup" => ("setup", "pkg_setup"),
        "unpack" => ("unpack", "src_unpack"),
        "prepare" => ("prepare", "src_prepare"),
        "configure" => ("configure", "src_configure"),
        "compile" => ("compile", "src_compile"),
        "test" => ("test", "src_test"),
        "install" => ("install", "src_install"),
        "preinst" => ("preinst", "pkg_preinst"),
        "postinst" => ("postinst", "pkg_postinst"),
        "prerm" => ("prerm", "pkg_prerm"),
        "postrm" => ("postrm", "pkg_postrm"),
        "nofetch" => ("nofetch", "pkg_nofetch"),
        "info" => ("info", "pkg_info"),
        "config" => ("config", "pkg_config"),
        // accept raw function names too
        other => (other, other),
    }
}

/// PMS phase function names mapped to their [`Phase`] variants.
///
/// Used to compute `DEFINED_PHASES` by inspecting which functions are
/// defined in the shell after sourcing an ebuild.
///
/// See [PMS 7.4](https://projects.gentoo.org/pms/9/pms.html#defined-phases).
const PHASE_FUNCTIONS: &[(&str, Phase)] = &[
    ("pkg_pretend", Phase::PkgPretend),
    ("pkg_setup", Phase::PkgSetup),
    ("src_unpack", Phase::SrcUnpack),
    ("src_prepare", Phase::SrcPrepare),
    ("src_configure", Phase::SrcConfigure),
    ("src_compile", Phase::SrcCompile),
    ("src_test", Phase::SrcTest),
    ("src_install", Phase::SrcInstall),
    ("pkg_preinst", Phase::PkgPreinst),
    ("pkg_postinst", Phase::PkgPostinst),
    ("pkg_prerm", Phase::PkgPrerm),
    ("pkg_postrm", Phase::PkgPostrm),
    ("pkg_config", Phase::PkgConfig),
    ("pkg_info", Phase::PkgInfo),
    ("pkg_nofetch", Phase::PkgNofetch),
];

/// Per-EAPI default phase implementations loaded by `init_build_env`.
///
/// These bash functions are called by `__ebuild_phase_funcs` (a Rust builtin)
/// when wiring up `default()` and `default_<phase>()`.  The functions call
/// `econf` / `emake` / `eapply` which are Rust builtins.
const PHASE_DEFAULT_FUNCTIONS: &str = r#"
__eapi0_pkg_nofetch() {
    [[ -z ${A} ]] && return
    elog "The following files cannot be fetched for ${PN}:"
    local x
    for x in ${A}; do elog "   ${x}"; done
}
__eapi0_src_unpack() { [[ -n ${A} ]] && unpack ${A}; }
__eapi0_src_compile() {
    if [[ -x ./configure ]]; then econf; fi
    __eapi2_src_compile
}
__eapi0_src_test() {
    # PMS: default src_test forces -j1 for EAPI ≤ 4 to avoid parallel test races.
    local jobflag=""
    ___eapi_default_src_test_disables_parallel_jobs && jobflag="-j1"
    local emake_cmd="${MAKE:-make} ${MAKEOPTS} ${EXTRA_EMAKE}${jobflag:+ ${jobflag}}"
    if ${emake_cmd} check -n &>/dev/null; then
        ${emake_cmd} check || die "check target failed"
    elif ${emake_cmd} test -n &>/dev/null; then
        ${emake_cmd} test || die "test target failed"
    fi
}
__eapi1_src_compile() { __eapi2_src_configure; __eapi2_src_compile; }
__eapi2_src_prepare() { :; }
__eapi2_src_configure() {
    if [[ -x ${ECONF_SOURCE:-.}/configure ]]; then econf; fi
}
__eapi2_src_compile() {
    if [[ -f Makefile || -f GNUmakefile || -f makefile ]]; then
        emake || die "emake failed"
    fi
}
__eapi4_src_install() {
    if [[ -f Makefile || -f GNUmakefile || -f makefile ]]; then
        emake DESTDIR="${D}" install || die "emake install failed"
    fi
    if [[ -v DOCS ]]; then
        if [[ ${DOCS@a} == *a* ]]; then
            [[ ${#DOCS[@]} -gt 0 ]] && dodoc "${DOCS[@]}"
        elif [[ -n ${DOCS} ]]; then
            dodoc ${DOCS}
        fi
    fi
}
__eapi6_src_prepare() {
    if [[ -n ${PATCHES+set} ]]; then
        if [[ ${PATCHES@a} == *a* ]]; then
            [[ ${#PATCHES[@]} -gt 0 ]] && eapply "${PATCHES[@]}"
        elif [[ -n ${PATCHES} ]]; then
            eapply ${PATCHES}
        fi
    fi
    eapply_user
}
__eapi6_src_install() {
    if [[ -f Makefile || -f GNUmakefile || -f makefile ]]; then
        emake DESTDIR="${D}" install || die "emake install failed"
    fi
    einstalldocs
}
__eapi8_src_prepare() {
    if [[ -n ${PATCHES+set} ]]; then
        if [[ ${PATCHES@a} == *a* ]]; then
            [[ ${#PATCHES[@]} -gt 0 ]] && eapply -- "${PATCHES[@]}"
        elif [[ -n ${PATCHES} ]]; then
            eapply -- ${PATCHES}
        fi
    fi
    eapply_user
}
nonfatal() { PORTAGE_NONFATAL=1 "$@"; local _r=$?; unset PORTAGE_NONFATAL; return $_r; }
assert() {
    local pipestatus=("${PIPESTATUS[@]}")
    local x
    for x in "${pipestatus[@]}"; do
        (( x == 0 )) && continue
        [[ $# -gt 0 ]] && die "$@" || die "assert: command failed"
    done
}
eapply() {
    local f
    for f in "$@"; do
        [[ ${f} == --  ]] && continue
        patch -p1 < "${f}" || die "eapply: patch failed: ${f}"
    done
}
eapply_user() { :; }
get_libdir() {
    local libdir_var="LIBDIR_${ABI}"
    [[ -n ${ABI} && -n ${!libdir_var} ]] && echo "${!libdir_var}" || echo "lib"
}
"#;

/// P3 install helpers loaded by `init_build_env` (PMS §12.3.x).
///
/// These bash functions replace the no-op stubs from `builtins.rs` during
/// build phases.  They install files into `${D}` using the standard `install`
/// utility and track destination-directory state in shell variables.
const INSTALL_HELPERS: &str = r#"
# Destination-directory state — reset to defaults by this sourcing.
_into_dir=/usr
INSDESTTREE=
EXEDESTTREE=
DOCDESTTREE=
_insopts="-m0644"
_exeopts="-m0755"
_docompress_includes=()
_docompress_excludes=()
_dostrip_includes=()
_dostrip_excludes=()

into()    { _into_dir="$1"; }
insinto() { INSDESTTREE="$1"; }
exeinto() { EXEDESTTREE="$1"; }
docinto() { DOCDESTTREE="$1"; }
insopts() { _insopts="$*"; }
exeopts() { _exeopts="$*"; }

dodir() {
    local d
    for d in "$@"; do
        install -d "${D%/}/${d#/}" || die "dodir: failed to create ${d}"
    done
}

keepdir() {
    dodir "$@"
    local d
    for d in "$@"; do
        : > "${D%/}/${d#/}/.keep_${CATEGORY}_${PN}-${SLOT//\//_}"
    done
}

dobin() {
    [[ $# -gt 0 ]] || die "dobin: at least one argument required"
    dodir "${_into_dir}/bin"
    local f
    for f in "$@"; do
        install -m0755 "${f}" "${D%/}/${_into_dir#/}/bin/${f##*/}" \
            || die "dobin: failed to install ${f}"
    done
}

newbin() {
    [[ $# -eq 2 ]] || die "newbin: exactly two arguments required"
    dodir "${_into_dir}/bin"
    install -m0755 "$1" "${D%/}/${_into_dir#/}/bin/$2" \
        || die "newbin: failed to install $1 as $2"
}

dosbin() {
    [[ $# -gt 0 ]] || die "dosbin: at least one argument required"
    dodir "${_into_dir}/sbin"
    local f
    for f in "$@"; do
        install -m0755 "${f}" "${D%/}/${_into_dir#/}/sbin/${f##*/}" \
            || die "dosbin: failed to install ${f}"
    done
}

newsbin() {
    [[ $# -eq 2 ]] || die "newsbin: exactly two arguments required"
    dodir "${_into_dir}/sbin"
    install -m0755 "$1" "${D%/}/${_into_dir#/}/sbin/$2" \
        || die "newsbin: failed to install $1 as $2"
}

doins() {
    local recursive=0
    [[ $1 == -r ]] && { recursive=1; shift; }
    [[ $# -gt 0 ]] || die "doins: at least one argument required"
    dodir "${INSDESTTREE:-/}"
    local dest="${D%/}/${INSDESTTREE#/}"
    local f
    for f in "$@"; do
        if [[ $recursive -eq 1 && -d ${f} ]]; then
            cp -pPR "${f}" "${dest}/" || die "doins: failed to copy ${f}"
        else
            install ${_insopts} "${f}" "${dest}/${f##*/}" \
                || die "doins: failed to install ${f}"
        fi
    done
}

newins() {
    [[ $# -eq 2 ]] || die "newins: exactly two arguments required"
    dodir "${INSDESTTREE:-/}"
    install ${_insopts} "$1" "${D%/}/${INSDESTTREE#/}/$2" \
        || die "newins: failed to install $1 as $2"
}

doexe() {
    [[ $# -gt 0 ]] || die "doexe: at least one argument required"
    dodir "${EXEDESTTREE:-/}"
    local dest="${D%/}/${EXEDESTTREE#/}"
    local f
    for f in "$@"; do
        install ${_exeopts} "${f}" "${dest}/${f##*/}" \
            || die "doexe: failed to install ${f}"
    done
}

newexe() {
    [[ $# -eq 2 ]] || die "newexe: exactly two arguments required"
    dodir "${EXEDESTTREE:-/}"
    install ${_exeopts} "$1" "${D%/}/${EXEDESTTREE#/}/$2" \
        || die "newexe: failed to install $1 as $2"
}

dolib.a() {
    [[ $# -gt 0 ]] || die "dolib.a: at least one argument required"
    local libdir; libdir=$(get_libdir)
    dodir "${_into_dir}/${libdir}"
    local f
    for f in "$@"; do
        install -m0644 "${f}" "${D%/}/${_into_dir#/}/${libdir}/${f##*/}" \
            || die "dolib.a: failed to install ${f}"
    done
}

dolib.so() {
    [[ $# -gt 0 ]] || die "dolib.so: at least one argument required"
    local libdir; libdir=$(get_libdir)
    dodir "${_into_dir}/${libdir}"
    local f
    for f in "$@"; do
        install -m0755 "${f}" "${D%/}/${_into_dir#/}/${libdir}/${f##*/}" \
            || die "dolib.so: failed to install ${f}"
    done
}

dolib() {
    [[ $# -gt 0 ]] || die "dolib: at least one argument required"
    local f
    for f in "$@"; do
        case "${f}" in
            *.so|*.so.*) dolib.so "${f}" ;;
            *)           dolib.a  "${f}" ;;
        esac
    done
}

newlib.a() {
    [[ $# -eq 2 ]] || die "newlib.a: exactly two arguments required"
    local libdir; libdir=$(get_libdir)
    dodir "${_into_dir}/${libdir}"
    install -m0644 "$1" "${D%/}/${_into_dir#/}/${libdir}/$2" \
        || die "newlib.a: failed to install $1 as $2"
}

newlib.so() {
    [[ $# -eq 2 ]] || die "newlib.so: exactly two arguments required"
    local libdir; libdir=$(get_libdir)
    dodir "${_into_dir}/${libdir}"
    install -m0755 "$1" "${D%/}/${_into_dir#/}/${libdir}/$2" \
        || die "newlib.so: failed to install $1 as $2"
}

dodoc() {
    local recursive=0
    [[ $1 == -r ]] && { recursive=1; shift; }
    [[ $# -gt 0 ]] || die "dodoc: at least one argument required"
    local docdir="${D%/}/usr/share/doc/${PF}${DOCDESTTREE:+/${DOCDESTTREE}}"
    dodir "/usr/share/doc/${PF}${DOCDESTTREE:+/${DOCDESTTREE}}"
    local f
    for f in "$@"; do
        if [[ $recursive -eq 1 && -d ${f} ]]; then
            cp -pPR "${f}" "${docdir}/" || die "dodoc: failed to copy ${f}"
        else
            install -m0644 "${f}" "${docdir}/${f##*/}" \
                || die "dodoc: failed to install ${f}"
        fi
    done
}

newdoc() {
    [[ $# -eq 2 ]] || die "newdoc: exactly two arguments required"
    local docdir="${D%/}/usr/share/doc/${PF}${DOCDESTTREE:+/${DOCDESTTREE}}"
    dodir "/usr/share/doc/${PF}${DOCDESTTREE:+/${DOCDESTTREE}}"
    install -m0644 "$1" "${docdir}/$2" || die "newdoc: failed to install $1 as $2"
}

doman() {
    [[ $# -gt 0 ]] || die "doman: at least one argument required"
    local f ext
    for f in "$@"; do
        ext="${f##*.}"
        [[ -n ${ext} ]] || die "doman: cannot determine man section for ${f}"
        dodir "/usr/share/man/man${ext}"
        install -m0644 "${f}" "${D%/}/usr/share/man/man${ext}/${f##*/}" \
            || die "doman: failed to install ${f}"
    done
}

newman() {
    [[ $# -eq 2 ]] || die "newman: exactly two arguments required"
    local ext="${2##*.}"
    [[ -n ${ext} ]] || die "newman: cannot determine man section for $2"
    dodir "/usr/share/man/man${ext}"
    install -m0644 "$1" "${D%/}/usr/share/man/man${ext}/$2" \
        || die "newman: failed to install $1 as $2"
}

doheader() {
    local recursive=0
    [[ $1 == -r ]] && { recursive=1; shift; }
    [[ $# -gt 0 ]] || die "doheader: at least one argument required"
    dodir "/usr/include"
    local f
    for f in "$@"; do
        if [[ $recursive -eq 1 && -d ${f} ]]; then
            cp -pPR "${f}" "${D%/}/usr/include/" || die "doheader: failed to copy ${f}"
        else
            install -m0644 "${f}" "${D%/}/usr/include/${f##*/}" \
                || die "doheader: failed to install ${f}"
        fi
    done
}

newheader() {
    [[ $# -eq 2 ]] || die "newheader: exactly two arguments required"
    dodir "/usr/include"
    install -m0644 "$1" "${D%/}/usr/include/$2" \
        || die "newheader: failed to install $1 as $2"
}

dosym() {
    local relative=0
    [[ $1 == -r ]] && { relative=1; shift; }
    [[ $# -eq 2 ]] || die "dosym: usage: dosym [-r] target link"
    local target="$1" link="$2"
    dodir "${link%/*}"
    if [[ $relative -eq 1 ]]; then
        local rel_target
        rel_target=$(python3 -c \
            "import os,sys; print(os.path.relpath(sys.argv[1], os.path.dirname(sys.argv[2])))" \
            "$target" "$link") || die "dosym: failed to compute relative path"
        ln -snf "$rel_target" "${D%/}/${link#/}" || die "dosym: failed to create symlink"
    else
        ln -snf "$target" "${D%/}/${link#/}" || die "dosym: failed to create symlink"
    fi
}

docompress() {
    if [[ $1 == - ]]; then
        shift; _docompress_excludes+=("$@")
    else
        _docompress_includes+=("$@")
    fi
}

dostrip() {
    if [[ $1 == - ]]; then
        shift; _dostrip_excludes+=("$@")
    else
        _dostrip_includes+=("$@")
    fi
}

doinitd() {
    [[ $# -gt 0 ]] || die "doinitd: at least one argument required"
    insinto /etc/init.d
    insopts -m0755
    doins "$@"
    insopts -m0644
}

doconfd() {
    [[ $# -gt 0 ]] || die "doconfd: at least one argument required"
    insinto /etc/conf.d
    doins "$@"
}

fperms() {
    [[ $# -ge 2 ]] || die "fperms: usage: fperms mode file..."
    local mode="$1"; shift
    local f
    for f in "$@"; do
        chmod "$mode" "${D%/}/${f#/}" || die "fperms: failed to chmod ${f}"
    done
}

fowners() {
    [[ $# -ge 2 ]] || die "fowners: usage: fowners owner file..."
    local owner="$1"; shift
    local f
    for f in "$@"; do
        chown "$owner" "${D%/}/${f#/}" || die "fowners: failed to chown ${f}"
    done
}

edo() {
    einfo "$@"
    "$@" || die "edo: command failed: $*"
}

einstalldocs() {
    local f
    if [[ -v DOCS ]]; then
        if [[ ${DOCS@a} == *a* ]]; then
            [[ ${#DOCS[@]} -gt 0 ]] && dodoc -r "${DOCS[@]}"
        elif [[ -n ${DOCS} ]]; then
            dodoc -r ${DOCS}
        fi
    else
        for f in README* CHANGES* ChangeLog* CHANGELOG* AUTHORS* NEWS* TODO* THANKS*; do
            [[ -s ${f} ]] && dodoc "${f}"
        done
    fi
    if [[ -v HTML_DOCS ]]; then
        if [[ ${HTML_DOCS@a} == *a* ]]; then
            [[ ${#HTML_DOCS[@]} -gt 0 ]] && dodoc -r "${HTML_DOCS[@]}"
        elif [[ -n ${HTML_DOCS} ]]; then
            dodoc -r ${HTML_DOCS}
        fi
    fi
}
"#;

/// An embedded bash shell for sourcing ebuilds, eclasses, and `make.defaults`.
///
/// Wraps [`brush_core::Shell`] configured for Gentoo ebuild evaluation.
/// The shell has standard bash builtins registered and eclass directories
/// set up for the repository.
///
/// See [PMS 7](https://projects.gentoo.org/pms/9/pms.html#ebuilddefined-variables)
/// for the metadata variables extracted after sourcing an ebuild.
pub struct EbuildShell {
    shell: Shell,
    repo_path: Utf8PathBuf,
    eclass_dirs: Vec<Utf8PathBuf>,
    /// Active USE flags for this shell session.
    /// Used by the `use()`, `usev()`, `usex()` functions.
    use_flags: HashSet<String>,
}

impl EbuildShell {
    /// Create a new shell configured for the given repository.
    ///
    /// Registers Portage-specific bash functions (`inherit`, `die`,
    /// `EXPORT_FUNCTIONS`, etc.) and sets up eclass directories from
    /// the repository's `eclass/` directory.
    pub async fn new(repo: &Repository) -> Result<Self> {
        Self::new_with_cache(repo, Arc::new(papaya::HashMap::new())).await
    }

    /// Create a new shell with a shared eclass AST cache.
    ///
    /// When processing many ebuilds, pass the same `Arc<papaya::HashMap>` to
    /// every shell so that each eclass is parsed at most once.
    pub async fn new_with_cache(
        repo: &Repository,
        eclass_cache: Arc<papaya::HashMap<String, brush_parser::ast::Program>>,
    ) -> Result<Self> {
        let mut shell = Shell::builder()
            .do_not_inherit_env(true)
            .profile(ProfileLoadBehavior::Skip)
            .rc(RcLoadBehavior::Skip)
            .parser(ParserImpl::Winnow)
            .build()
            .await
            .map_err(|e| Error::Shell(e.to_string()))?;

        shell.register_default_builtins(brush_builtins::BuiltinSet::BashMode);

        let eclass_dir: Utf8PathBuf = repo.path().join("eclass");
        let eclass_dirs: Vec<Utf8PathBuf> = if eclass_dir.is_dir() {
            vec![eclass_dir]
        } else {
            Vec::new()
        };

        // Register Portage-specific shell functions (die, EXPORT_FUNCTIONS, etc.)
        stubs::register(&mut shell).await?;

        // Register `inherit` with a shared eclass AST cache.
        let inherit_reg = brush_core::builtins::builtin::<inherit::InheritCommand, _>().with_state(
            inherit::InheritState {
                inherited: Vec::new(),
                cache: eclass_cache,
            },
        );
        shell.register_builtin("inherit", inherit_reg);

        // Register PMS 12.3 utility builtins (has, use, usev, usex, etc.).
        for (name, builtin) in [
            (
                "die",
                brush_core::builtins::builtin::<commands::DieCommand, _>(),
            ),
            (
                "EXPORT_FUNCTIONS",
                brush_core::builtins::builtin::<commands::ExportFunctionsCommand, _>(),
            ),
            (
                "has",
                brush_core::builtins::builtin::<commands::HasCommand, _>(),
            ),
            (
                "hasv",
                brush_core::builtins::builtin::<commands::HasvCommand, _>(),
            ),
            (
                "hasq",
                brush_core::builtins::builtin::<commands::HasCommand, _>(),
            ),
            (
                "use",
                brush_core::builtins::builtin::<commands::UseCommand, _>(),
            ),
            (
                "usev",
                brush_core::builtins::builtin::<commands::UsevCommand, _>(),
            ),
            (
                "usex",
                brush_core::builtins::builtin::<commands::UsexCommand, _>(),
            ),
            (
                "use_enable",
                brush_core::builtins::builtin::<commands::UseEnableCommand, _>(),
            ),
            (
                "use_with",
                brush_core::builtins::builtin::<commands::UseWithCommand, _>(),
            ),
            (
                "in_iuse",
                brush_core::builtins::builtin::<commands::InIuseCommand, _>(),
            ),
        ] {
            shell.register_builtin(name, builtin);
        }

        // Register 74 ___eapi_* EAPI predicate builtins (portage eapi.sh).
        for &name in commands::EAPI_PREDICATE_NAMES {
            shell.register_builtin(
                name,
                brush_core::builtins::builtin::<commands::EapiPredicateCommand, _>(),
            );
        }

        // Register phase-setup builtin (__ebuild_phase_funcs).
        shell.register_builtin(
            "__ebuild_phase_funcs",
            brush_core::builtins::builtin::<commands::EbuildPhaseFuncsCommand, _>(),
        );

        // Register P1 output helper builtins (einfo, ewarn, …).
        for &name in &["einfo", "elog", "ewarn", "eerror", "eqawarn", "einfon"] {
            shell.register_builtin(
                name,
                brush_core::builtins::builtin::<commands::EchoMessageCommand, _>(),
            );
        }
        shell.register_builtin(
            "ebegin",
            brush_core::builtins::builtin::<commands::EbeginCommand, _>(),
        );
        shell.register_builtin(
            "eend",
            brush_core::builtins::builtin::<commands::EendCommand, _>(),
        );

        // Register P2 build helper builtins (emake, econf).
        shell.register_builtin(
            "emake",
            brush_core::builtins::builtin::<commands::EmakeCommand, _>(),
        );
        shell.register_builtin(
            "econf",
            brush_core::builtins::builtin::<commands::EconfCommand, _>(),
        );

        // Register P4 unpack builtin.
        shell.register_builtin(
            "unpack",
            brush_core::builtins::builtin::<commands::UnpackCommand, _>(),
        );

        // Register PMS 12.3.14 version manipulation builtins.
        // ver_cut and ver_test are Rust builtins to avoid bash arithmetic
        // issues in array slice expressions (brush limitation).
        // ver_rs is kept as a bash function because brush silently drops
        // empty-string args when calling Rust builtins.
        shell.register_builtin(
            "ver_cut",
            brush_core::builtins::builtin::<ver_funcs::VerCutCommand, _>(),
        );
        shell.register_builtin(
            "ver_rs",
            brush_core::builtins::builtin::<ver_funcs::VerRsCommand, _>(),
        );
        shell.register_builtin(
            "ver_test",
            brush_core::builtins::builtin::<ver_funcs::VerTestCommand, _>(),
        );
        // ver_replacing (EAPI 9): outputs versions being replaced; always
        // empty during metadata extraction.
        shell.register_builtin(
            "ver_replacing",
            brush_core::builtins::builtin::<ver_funcs::VerReplacingCommand, _>(),
        );

        let mut ebuild_shell = EbuildShell {
            shell,
            repo_path: repo.path().to_path_buf(),
            eclass_dirs,
            use_flags: HashSet::new(),
        };
        ebuild_shell.sync_eclass_dirs_var();

        Ok(ebuild_shell)
    }

    /// Append an eclass directory (searched after existing dirs).
    pub fn add_eclass_dir(&mut self, dir: Utf8PathBuf) {
        self.eclass_dirs.push(dir);
        self.sync_eclass_dirs_var();
    }

    /// Prepend an eclass directory (searched before existing dirs).
    ///
    /// Used to add master repository eclass directories so they are
    /// searched before the overlay's own eclasses.
    pub fn prepend_eclass_dir(&mut self, dir: Utf8PathBuf) {
        self.eclass_dirs.insert(0, dir);
        self.sync_eclass_dirs_var();
    }

    /// Update the `__PORTAGE_ECLASS_DIRS` shell variable to reflect the
    /// current set of eclass directories.  Called after any mutation of
    /// [`Self::eclass_dirs`].
    fn sync_eclass_dirs_var(&mut self) {
        let value: String = self
            .eclass_dirs
            .iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join(":");
        self.set_var("__PORTAGE_ECLASS_DIRS", &value);
    }

    /// Source an ebuild file and extract its metadata.
    ///
    /// This performs the following steps:
    /// 1. Set PM-provided variables (`CATEGORY`, `PN`, `PV`, `PVR`, `PF`, `P`,
    ///    `FILESDIR`, `WORKDIR`, etc.)
    /// 2. Source the ebuild — the `inherit` shell function handles eclass
    ///    sourcing, line continuations, and nesting automatically
    /// 3. Extract metadata variables from the shell environment
    ///
    /// See [PMS 7.2](https://projects.gentoo.org/pms/9/pms.html#mandatory-ebuilddefined-variables).
    pub async fn source_ebuild(&mut self, ebuild: &Ebuild) -> Result<crate::source::SourcedEbuild> {
        // Set PM-provided variables
        let category = ebuild.category();
        let pn = ebuild.name();
        let version = ebuild.version();
        // Use the raw filename version string to preserve leading zeros per PMS §7.2.
        // Version::to_string() normalises numeric components (26.04.0 → 26.4.0).
        let pvr = version.to_string();
        let pr = format!("r{}", version.revision.0);
        let pv = if version.revision.0 > 0 {
            pvr.strip_suffix(&format!("-{pr}"))
                .unwrap_or(&pvr)
                .to_owned()
        } else {
            pvr.clone()
        };
        let p = format!("{pn}-{pv}");
        let pf = format!("{pn}-{pvr}");

        self.set_var("CATEGORY", category);
        self.set_var("PN", pn);
        self.set_var("PV", &pv);
        self.set_var("PR", &pr);
        self.set_var("PVR", &pvr);
        self.set_var("P", &p);
        self.set_var("PF", &pf);

        let filesdir = self.repo_path.join(category).join(pn).join("files");
        self.set_var("FILESDIR", filesdir.as_str());

        // Detect EAPI before sourcing per PMS 7.3.1
        let eapi = ebuild.detect_eapi()?;
        self.set_var("EAPI", &eapi.to_string());

        // Absolute path to the ebuild file (PMS 11.1)
        let ebuild_abs = std::fs::canonicalize(ebuild.path())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ebuild.path().to_string());
        self.set_var("EBUILD", &ebuild_abs);

        // Build-directory variables (PMS 11.1)
        // Deterministic placeholders — no temp directories are created.
        let base = format!("/var/tmp/portage/{category}/{pf}");
        let workdir = format!("{base}/work");
        self.set_var("WORKDIR", &workdir);
        self.set_var("S", &format!("{workdir}/{p}"));
        self.set_var("T", &format!("{base}/temp"));
        self.set_var("TMPDIR", &format!("{base}/temp"));
        self.set_var("HOME", &format!("{base}/homedir"));
        self.set_var("D", &format!("{base}/image/"));
        self.set_var("DISTDIR", "/var/cache/distfiles");

        // Phase/merge variables (PMS 11.1)
        self.set_var("EBUILD_PHASE", "depend");
        self.set_var("EBUILD_PHASE_FUNC", "");
        self.set_var("ROOT", "/");
        self.set_var("MERGE_TYPE", "source");

        // EAPI 3+ prefix variables (PMS 11.1)
        if eapi >= Eapi::Three {
            self.set_var("EPREFIX", "");
            self.set_var("ED", &format!("{base}/image/"));
            self.set_var("EROOT", "/");
        }

        // EAPI 7+ sysroot variables (PMS 11.1)
        if eapi >= Eapi::Seven {
            self.set_var("SYSROOT", "/");
            self.set_var("ESYSROOT", "/");
            self.set_var("BROOT", "/");
        }

        // PMS 10.2 accumulating variables (EAPI-dependent).
        // Cleared before sourcing so the ebuild's inherit calls populate E_*
        // from scratch.  Combined with ebuild values after sourcing.
        // Mirrors Portage's B_*/E_* pattern in ebuild.sh.
        let accum_vars: &[&str] = if eapi >= Eapi::Eight {
            &[
                "IUSE",
                "REQUIRED_USE",
                "DEPEND",
                "BDEPEND",
                "RDEPEND",
                "PDEPEND",
                "IDEPEND",
                "PROPERTIES",
                "RESTRICT",
            ]
        } else {
            &[
                "IUSE",
                "REQUIRED_USE",
                "DEPEND",
                "BDEPEND",
                "RDEPEND",
                "PDEPEND",
                "IDEPEND",
            ]
        };

        // Clear accumulating vars and their E_* counterparts before sourcing.
        // The ebuild's own inherit calls will repopulate E_* during sourcing.
        let e_accum_pre: &[&str] = if eapi >= Eapi::Eight {
            inherit::E_VARS_ALL
        } else {
            inherit::E_VARS_BASE
        };
        for (&var, &e_var) in accum_vars.iter().zip(e_accum_pre.iter()) {
            self.set_var(var, "");
            self.set_var(e_var, "");
        }
        self.set_var("INHERIT", "");
        self.set_var("INHERITED", "");
        if let Some(state) = self
            .shell
            .builtin_state_mut_of::<inherit::InheritCommand>("inherit")
        {
            state.inherited.clear();
        }

        // EAPI 6+ requires failglob in global scope (PMS 6, Table 6.1).
        // Reset each call so re-used shells get the right state per ebuild.
        if eapi >= Eapi::Six {
            self.run_string("shopt -s failglob").await?;
        } else {
            self.run_string("shopt -u failglob").await?;
        }

        // Source the ebuild — `inherit` is a Rust builtin that accumulates
        // each eclass's contribution into E_{VAR} and restores the var after
        // each eclass (PMS 10.2 / Portage B_*/E_* pattern).
        let params = self.shell.default_exec_params();
        self.shell
            .source_script(
                ebuild.path().as_std_path(),
                std::iter::empty::<&str>(),
                &params,
            )
            .await
            .map_err(|e| Error::Shell(format!("sourcing {}: {e}", ebuild.path())))?;

        // PMS 10.2: combine ebuild-defined values with eclass contributions.
        // After sourcing, `var` holds only what the ebuild set; `E_{var}` holds
        // the total of all eclass contributions.  Append eclass total to ebuild value.
        let e_accum_vars: &[&str] = if eapi >= Eapi::Eight {
            inherit::E_VARS_ALL
        } else {
            inherit::E_VARS_BASE
        };
        for (&var, &e_var) in accum_vars.iter().zip(e_accum_vars.iter()) {
            let ebuild_val = self.get_var(var).unwrap_or_default();
            let eclass_val = self.get_var(e_var).unwrap_or_default();
            let combined = match (ebuild_val.is_empty(), eclass_val.is_empty()) {
                (true, true) => String::new(),
                (true, false) => eclass_val.trim().to_string(),
                (false, true) => ebuild_val,
                (false, false) => format!("{} {}", ebuild_val, eclass_val.trim()),
            };
            self.set_var(var, &combined);
            self.set_var(e_var, ""); // clean up E_*
        }

        // Extract metadata, then override EAPI with the pre-detected value
        // (the authoritative source per PMS 7.3.1)
        let mut metadata = self.extract_metadata()?;
        metadata.eapi = eapi;

        // CacheEntry::parse derives `inherited` from `_eclasses_`, which doesn't
        // exist yet during regen. Read the transitive list directly from the
        // `inherit` builtin's Rust state — no bash-string parsing needed. The
        // resolved file paths come along too so the cache writer can md5 each
        // eclass without re-resolving the name (which would miss masters).
        let inherited = self
            .shell
            .builtin_state_of::<inherit::InheritCommand>("inherit")
            .map(|s| s.inherited.clone())
            .unwrap_or_default();
        metadata.inherited = inherited.iter().map(|e| e.name.clone()).collect();
        let eclasses = inherited.into_iter().map(|e| (e.name, e.path)).collect();

        Ok(crate::source::SourcedEbuild { metadata, eclasses })
    }

    /// Locate portage's script directory under `/usr/lib/portage`.
    ///
    /// Scans for a subdirectory (typically `pythonX.Y`) that contains
    /// `isolated-functions.sh`, and returns the highest-sorted match.
    fn find_portage_bin_path() -> Option<PathBuf> {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir("/usr/lib/portage")
            .ok()?
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                if p.is_dir() && p.join("isolated-functions.sh").exists() {
                    Some(p)
                } else {
                    None
                }
            })
            .collect();
        dirs.sort();
        dirs.pop()
    }

    /// Set up the build environment.
    ///
    /// Prepends portage's `ebuild-helpers/` to `PATH` (for `doins`, `dosbin`, …),
    /// configures a writable `DISTDIR`, passes through build-tool variables
    /// (`CFLAGS`, `MAKEOPTS`, …) from the caller's environment, and defines
    /// the per-EAPI default phase implementation bash functions
    /// (`__eapi0_src_unpack`, `__eapi2_src_compile`, …) that `__ebuild_phase_funcs`
    /// wires together.
    ///
    /// The output helpers (`einfo`, `ewarn`, …), predicates (`___eapi_*`),
    /// `emake`, `econf`, and `__ebuild_phase_funcs` are registered as Rust
    /// builtins in `new_with_cache` and therefore never sourced from portage.
    pub async fn init_build_env(&mut self) -> Result<()> {
        // Prepend portage's ebuild-helpers to PATH for do*/new* install helpers.
        if let Some(bin_path) = Self::find_portage_bin_path() {
            self.set_var("PORTAGE_BIN_PATH", &bin_path.to_string_lossy());
            let helpers = bin_path.join("ebuild-helpers");
            let cur_path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_string());
            self.set_var("PATH", &format!("{}:{cur_path}", helpers.display()));
        }

        // Writable DISTDIR: honour env override, fall back to ~/.cache/distfiles.
        let distdir = std::env::var("DISTDIR").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{home}/.cache/distfiles")
        });
        std::fs::create_dir_all(&distdir).ok();
        self.set_var("DISTDIR", &distdir);

        // Pass through build-tool variables from the caller's environment.
        for var in &[
            "MAKEOPTS",
            "CFLAGS",
            "CXXFLAGS",
            "CPPFLAGS",
            "LDFLAGS",
            "CC",
            "CXX",
            "AR",
            "RANLIB",
            "NM",
            "STRIP",
            "PKG_CONFIG",
        ] {
            if let Ok(val) = std::env::var(var) {
                self.set_var(var, &val);
            }
        }
        // Strip GNU make jobserver tokens from MAKEFLAGS if set.  The fds
        // (--jobserver-auth=R,W or legacy --jobserver-fds=R,W) belong to a
        // make process in the caller's tree and are not valid here.  Leaving
        // them in causes every make invocation in every phase to try to open
        // dead file descriptors.
        if let Ok(flags) = std::env::var("MAKEFLAGS") {
            let clean = strip_jobserver_tokens(&flags);
            self.set_var("MAKEFLAGS", &clean);
        }

        // Remove bash stub no-ops that were installed for metadata extraction.
        // These stubs shadow the Rust builtins for econf, emake, einfo, etc.
        // Unsetting them lets the Rust builtin registry take over during build.
        self.run_string(
            "unset -f econf emake unpack einfo einfon elog ewarn eerror eqawarn ebegin eend nonfatal",
        )
        .await
        .ok();

        // Define per-EAPI default phase implementations as bash functions.
        // These are called by __ebuild_phase_funcs (a Rust builtin) to set up
        // default() and default_<phase>() for the currently executing phase.
        self.run_string(PHASE_DEFAULT_FUNCTIONS).await?;

        // Define real P3 install helpers (dobin, doins, dodoc, dosym, …).
        // These override the no-op stubs from builtins.rs that were needed
        // for metadata extraction.
        self.run_string(INSTALL_HELPERS).await?;

        Ok(())
    }

    /// Source an ebuild and run a single phase function.
    ///
    /// Creates the standard build directories under `work_root` if they don't
    /// exist, sets all PMS environment variables, sources the ebuild (which
    /// triggers `inherit` and populates eclass functions), then calls the
    /// phase function if it is defined.
    ///
    /// Unlike [`source_ebuild`], no metadata extraction is performed.  Output
    /// from the phase (stdout/stderr) is passed through to the caller's
    /// terminal.
    ///
    /// # Arguments
    /// * `ebuild`    – the ebuild to source
    /// * `phase`     – portage phase name (`"compile"`, `"install"`, …) or raw
    ///                 function name (`"src_compile"`)
    /// * `work_root` – root for build dirs; `work/`, `temp/`, `image/` are
    ///                 created beneath it
    pub async fn run_phase(
        &mut self,
        ebuild: &Ebuild,
        phase: &str,
        work_root: &Path,
    ) -> Result<()> {
        let category = ebuild.category();
        let pn = ebuild.name();
        let version = ebuild.version();
        let pvr = version.to_string();
        let pr = format!("r{}", version.revision.0);
        let pv = if version.revision.0 > 0 {
            pvr.strip_suffix(&format!("-{pr}"))
                .unwrap_or(&pvr)
                .to_owned()
        } else {
            pvr.clone()
        };
        let p = format!("{pn}-{pv}");
        let pf = format!("{pn}-{pvr}");

        self.set_var("CATEGORY", category);
        self.set_var("PN", pn);
        self.set_var("PV", &pv);
        self.set_var("PR", &pr);
        self.set_var("PVR", &pvr);
        self.set_var("P", &p);
        self.set_var("PF", &pf);

        let filesdir = self.repo_path.join(category).join(pn).join("files");
        self.set_var("FILESDIR", filesdir.as_str());

        let eapi = ebuild.detect_eapi()?;
        self.set_var("EAPI", &eapi.to_string());

        let ebuild_abs = std::fs::canonicalize(ebuild.path())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ebuild.path().to_string());
        self.set_var("EBUILD", &ebuild_abs);

        // Source portage's function libraries and configure build environment.
        // Must happen after EAPI is set so phase-functions.sh registers the
        // right EAPI-specific defaults.
        self.init_build_env().await?;

        // Create and set real build directories.
        let workdir = work_root.join("work");
        let t = work_root.join("temp");
        let d = work_root.join("image");
        let homedir = work_root.join("homedir");
        for dir in [&workdir, &t, &d, &homedir] {
            std::fs::create_dir_all(dir)
                .map_err(|e| Error::Shell(format!("creating {}: {e}", dir.display())))?;
        }
        self.set_var("WORKDIR", &workdir.to_string_lossy());
        self.set_var("S", &workdir.join(&p).to_string_lossy());
        self.set_var("T", &t.to_string_lossy());
        self.set_var("TMPDIR", &t.to_string_lossy());
        self.set_var("HOME", &homedir.to_string_lossy());
        self.set_var("D", &format!("{}/", d.display()));
        // DISTDIR is already set by init_build_env() from env or ~/.cache/distfiles;
        // do not override it here.

        // Phase and merge variables.
        let (phase_val, func_name) = phase_to_func(phase);
        self.set_var("EBUILD_PHASE", phase_val);
        self.set_var("EBUILD_PHASE_FUNC", func_name);
        self.set_var("ROOT", "/");
        self.set_var("MERGE_TYPE", "source");

        if eapi >= Eapi::Three {
            self.set_var("EPREFIX", "");
            self.set_var("ED", &format!("{}/", d.display()));
            self.set_var("EROOT", "/");
        }
        if eapi >= Eapi::Seven {
            self.set_var("SYSROOT", "/");
            self.set_var("ESYSROOT", "/");
            self.set_var("BROOT", "/");
        }

        // Clear eclass accumulation state (same as source_ebuild).
        let accum_vars: &[&str] = if eapi >= Eapi::Eight {
            &[
                "IUSE",
                "REQUIRED_USE",
                "DEPEND",
                "BDEPEND",
                "RDEPEND",
                "PDEPEND",
                "IDEPEND",
                "PROPERTIES",
                "RESTRICT",
            ]
        } else {
            &[
                "IUSE",
                "REQUIRED_USE",
                "DEPEND",
                "BDEPEND",
                "RDEPEND",
                "PDEPEND",
                "IDEPEND",
            ]
        };
        let e_vars: &[&str] = if eapi >= Eapi::Eight {
            inherit::E_VARS_ALL
        } else {
            inherit::E_VARS_BASE
        };
        for (&var, &e_var) in accum_vars.iter().zip(e_vars.iter()) {
            self.set_var(var, "");
            self.set_var(e_var, "");
        }
        self.set_var("INHERIT", "");
        self.set_var("INHERITED", "");
        if let Some(state) = self
            .shell
            .builtin_state_mut_of::<inherit::InheritCommand>("inherit")
        {
            state.inherited.clear();
        }

        if eapi >= Eapi::Six {
            self.run_string("shopt -s failglob").await?;
        } else {
            self.run_string("shopt -u failglob").await?;
        }

        // Export all PM-provided variables so external processes (make, ./configure,
        // portage ebuild-helpers like dodoc/doins) inherit them as environment variables.
        // Bash `export` on an unset/empty name is harmless — it just marks it for export.
        self.run_string(
            "export CATEGORY PN PV PR PVR P PF FILESDIR WORKDIR S T D EAPI EBUILD \
             HOME ROOT DISTDIR PORTAGE_BIN_PATH PATH EBUILD_PHASE EBUILD_PHASE_FUNC \
             MERGE_TYPE EPREFIX ED EROOT SYSROOT ESYSROOT BROOT USE \
             MAKEOPTS CFLAGS CXXFLAGS CPPFLAGS LDFLAGS CC CXX AR RANLIB NM STRIP",
        )
        .await
        .ok();

        // Source the ebuild — defines all phase functions and global variables.
        let params = self.shell.default_exec_params();
        self.shell
            .source_script(
                ebuild.path().as_std_path(),
                std::iter::empty::<&str>(),
                &params,
            )
            .await
            .map_err(|e| Error::Shell(format!("sourcing {}: {e}", ebuild.path())))?;

        // Combine eclass E_* contributions with ebuild-defined values (PMS 10.2).
        for (&var, &e_var) in accum_vars.iter().zip(e_vars.iter()) {
            let ebuild_val = self.get_var(var).unwrap_or_default();
            let eclass_val = self.get_var(e_var).unwrap_or_default();
            let combined = match (ebuild_val.is_empty(), eclass_val.is_empty()) {
                (true, true) => String::new(),
                (true, false) => eclass_val.trim().to_string(),
                (false, true) => ebuild_val,
                (false, false) => format!("{} {}", ebuild_val, eclass_val.trim()),
            };
            self.set_var(var, &combined);
            self.set_var(e_var, "");
        }

        // Compute $A from $SRC_URI for the active USE flag set.
        // $A is the space-separated list of distfile names needed by this ebuild.
        // It is a PM-provided variable (not computed in bash) and must be set
        // before any phase function runs (src_unpack reads it via ${A}).
        self.set_a_from_src_uri();
        self.run_string("export A").await.ok();

        // Wire up `default` and any missing EAPI default implementations
        // (e.g. src_compile → __eapi2_src_compile) for the current phase.
        // __ebuild_phase_funcs is a Rust builtin (not in funcs()), always run it.
        self.run_string(&format!("__ebuild_phase_funcs {eapi} {func_name}"))
            .await
            .ok();

        // Set the working directory for the phase.
        // src_unpack and pkg_nofetch run in $WORKDIR (archives are extracted there;
        // $S doesn't exist yet).  All other phases run in $S (the source tree).
        let cd_target = match func_name {
            "src_unpack" | "pkg_nofetch" => "\"${WORKDIR}\"",
            _ => "\"${S}\" 2>/dev/null || cd \"${WORKDIR}\" 2>/dev/null || true",
        };
        self.run_string(&format!("cd {cd_target}")).await.ok();

        // Run the phase function (may have been defined by the ebuild or by
        // __ebuild_phase_funcs as a fallback calling default()).
        let phase_defined = self.shell.funcs().get(func_name).is_some();
        if phase_defined {
            self.run_string(func_name).await?;
        } else {
            eprintln!("warning: {func_name} not defined, nothing to do");
        }

        Ok(())
    }

    /// Pre-parse every `.eclass` file in the shell's configured eclass directories
    /// into the shared AST cache.
    ///
    /// Calling this once before spawning workers guarantees 100% cache hits during
    /// ebuild processing, eliminating all per-worker parse work and any concurrent
    /// insert races.  Directories are searched in order; the first definition of
    /// each eclass name wins (same priority as `inherit`).
    /// Source an eclass by name.
    ///
    /// Searches the configured eclass directories in order.
    pub async fn source_eclass(&mut self, name: &str) -> Result<()> {
        let filename = format!("{name}.eclass");
        for dir in &self.eclass_dirs {
            let path: Utf8PathBuf = dir.join(&filename);
            if path.is_file() {
                let params = self.shell.default_exec_params();
                self.shell
                    .source_script(path.as_std_path(), std::iter::empty::<&str>(), &params)
                    .await
                    .map_err(|e| Error::Shell(format!("sourcing eclass {name}: {e}")))?;
                return Ok(());
            }
        }
        Err(Error::Shell(format!("eclass not found: {name}")))
    }

    /// Source a `make.defaults` file.
    ///
    /// Variable assignments (with `${VAR}` expansion) are evaluated in the
    /// shell environment.
    ///
    /// See [PMS 5.2.4](https://projects.gentoo.org/pms/9/pms.html#makedefaults).
    pub async fn source_make_defaults(&mut self, path: &Path) -> Result<()> {
        let params = self.shell.default_exec_params();
        self.shell
            .source_script(path, std::iter::empty::<&str>(), &params)
            .await
            .map_err(|e| Error::Shell(format!("sourcing make.defaults {}: {e}", path.display())))?;
        Ok(())
    }

    /// Resolve the path of a named eclass by searching the configured eclass directories.
    pub fn eclass_path(&self, name: &str) -> Option<Utf8PathBuf> {
        let filename = format!("{name}.eclass");
        self.eclass_dirs
            .iter()
            .map(|dir| dir.join(&filename))
            .find(|p| p.is_file())
    }

    /// Read a variable from the shell environment.
    pub fn get_var(&self, name: &str) -> Option<String> {
        self.shell.env_str(name).map(|cow| cow.into_owned())
    }

    /// Set a variable in the shell environment.
    fn set_var(&mut self, name: &str, value: &str) {
        let _ = self.shell.set_env_global(
            name,
            ShellVariable::new(ShellValue::String(value.to_string())),
        );
    }

    /// Run a bash script string directly in the shell without writing a temporary file.
    pub async fn run_string(&mut self, script: &str) -> Result<()> {
        let params = self.shell.default_exec_params();
        let source_info = SourceInfo::from("inline");
        self.shell
            .run_string(script, &source_info, &params)
            .await
            .map_err(|e| Error::Shell(format!("run_string: {e}")))?;
        Ok(())
    }

    /// Set the active USE flags for this shell session.
    ///
    /// These flags will be used by the `use()`, `usev()`, `usex()` functions
    /// when sourcing ebuilds and eclasses.
    ///
    /// # Example
    /// ```no_run
    /// use portage_repo::Repository;
    ///
    /// # async fn example() {
    /// let repo = Repository::open("/var/db/repos/gentoo").unwrap();
    /// let mut shell = repo.shell().await.unwrap();
    /// shell.set_use_flags(&["ssl", "gtk", "-doc"]).unwrap();
    /// # }
    /// ```
    pub fn set_use_flags(&mut self, flags: &[&str]) -> Result<()> {
        let mut new_flags = HashSet::new();

        for flag in flags {
            let flag_str = flag.trim();
            if flag_str.is_empty() {
                continue;
            }

            let (flag_name, enabled) = if let Some(stripped) = flag_str.strip_prefix('-') {
                (stripped.to_string(), false)
            } else if let Some(stripped) = flag_str.strip_prefix('+') {
                (stripped.to_string(), true)
            } else {
                (flag_str.to_string(), true)
            };

            if enabled {
                new_flags.insert(flag_name);
            } else {
                new_flags.remove(&flag_name);
            }
        }

        self.use_flags = new_flags;

        // Update the USE environment variable
        let use_flags = self.use_flags_string();
        if !use_flags.is_empty() {
            self.set_var("USE", &use_flags);
        } else {
            self.set_var("USE", "");
        }

        Ok(())
    }

    /// Get the current USE flags as a space-separated string.
    ///
    /// This can be used to set the `USE` environment variable in the shell.
    pub fn use_flags_string(&self) -> String {
        let mut flags: Vec<_> = self.use_flags.iter().cloned().collect();
        flags.sort();
        flags.join(" ")
    }

    /// Compute `$A` from `$SRC_URI` and inject it into the shell environment.
    ///
    /// `$A` is a PM-provided variable containing the space-separated list of
    /// distfile names required by the ebuild for the currently active USE flags.
    /// It is computed by the PM (not in bash) and must be set before any phase
    /// function runs, since `src_unpack` and `pkg_nofetch` iterate `${A}`.
    ///
    /// USE-conditional groups (`flag? ( ... )`) are evaluated against
    /// [`Self::use_flags`]; unconditional files are always included.
    fn set_a_from_src_uri(&mut self) {
        let src_uri = self.get_var("SRC_URI").unwrap_or_default();
        if src_uri.is_empty() {
            self.set_var("A", "");
            return;
        }
        let entries = match SrcUriEntry::parse(&src_uri) {
            Ok(e) => e,
            Err(_) => {
                self.set_var("A", "");
                return;
            }
        };
        let use_flags = self.use_flags.clone();
        let mut files: Vec<String> = Vec::new();
        collect_src_filenames(&entries, &use_flags, &mut files);
        self.set_var("A", &files.join(" "));
    }

    /// Extract metadata from shell variables into a `CacheEntry`-compatible string
    /// and parse it via portage-metadata.
    fn extract_metadata(&self) -> Result<EbuildMetadata> {
        // Collect (key, value) pairs directly from the shell environment,
        // using Cow<str> to avoid cloning when the value needs no normalization.
        let pairs: Vec<(&str, std::borrow::Cow<str>)> = METADATA_VARS
            .iter()
            .filter_map(|&var| {
                let value = self.shell.env_str(var)?;
                if value.is_empty() {
                    return None;
                }
                // Normalize embedded newlines/tabs to spaces (heredoc values).
                let normalized = if var == "DESCRIPTION" {
                    std::borrow::Cow::Owned(itertools::join(value.split_whitespace(), " "))
                } else if value.bytes().any(|b| matches!(b, b'\n' | b'\r' | b'\t')) {
                    std::borrow::Cow::Owned(itertools::join(value.split_whitespace(), " "))
                } else {
                    value
                };
                if normalized.is_empty() {
                    return None;
                }
                Some((var, normalized))
            })
            .collect();

        let entry = portage_metadata::CacheEntry::from_kv_pairs(
            pairs.iter().map(|(k, v)| (*k, v.as_ref())),
        )?;

        // Compute DEFINED_PHASES by inspecting which phase functions are
        // defined in the shell after sourcing (PMS 7.4).
        let mut defined_phases: Vec<Phase> = PHASE_FUNCTIONS
            .iter()
            .filter(|(name, _)| self.shell.funcs().get(name).is_some())
            .map(|(_, phase)| *phase)
            .collect();
        // Sort alphabetically by short name to match Portage's cache format.
        defined_phases.sort_by_key(|p| p.as_str());

        let mut metadata = entry.metadata;
        metadata.defined_phases = defined_phases;
        Ok(metadata)
    }
}

/// Recursively collect distfile names from a parsed `SRC_URI` tree.
/// Remove GNU make jobserver tokens from a MAKEFLAGS string.
///
/// `--jobserver-auth=R,W` (make ≥ 4.2) and `--jobserver-fds=R,W` (older
/// make) encode file-descriptor numbers that are only valid inside the make
/// process tree that created them.  Any other process that inherits MAKEFLAGS
/// and tries to open those fds will get EBADF or hit a completely unrelated
/// fd.  Strip them unconditionally at build-env initialisation time.
fn strip_jobserver_tokens(flags: &str) -> String {
    let cleaned: Vec<&str> = flags
        .split_whitespace()
        .filter(|tok| !tok.starts_with("--jobserver-auth=") && !tok.starts_with("--jobserver-fds="))
        .collect();
    cleaned.join(" ")
}

///
/// USE-conditional groups are evaluated against `use_flags`; unconditional
/// files are always appended.  The `->` arrow rename case is handled by
/// the `Renamed` variant (target filename is used, not the source URL).
fn collect_src_filenames(
    entries: &[SrcUriEntry],
    use_flags: &HashSet<String>,
    files: &mut Vec<String>,
) {
    for entry in entries {
        match entry {
            SrcUriEntry::Uri { filename, .. } => files.push(filename.clone()),
            SrcUriEntry::Renamed { target, .. } => files.push(target.clone()),
            SrcUriEntry::UseConditional {
                flag,
                negated,
                entries,
            } => {
                let flag_set = use_flags.contains(flag.as_str());
                // Include when: (not negated AND flag set) OR (negated AND flag not set).
                if flag_set != *negated {
                    collect_src_filenames(entries, use_flags, files);
                }
            }
            SrcUriEntry::Group(entries) => {
                collect_src_filenames(entries, use_flags, files);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Repository shell factory methods — live here so repo/ stays brush-free.
// ---------------------------------------------------------------------------

impl Repository {
    /// Create an [`EbuildShell`] configured for this repository.
    pub async fn shell(&self) -> Result<EbuildShell> {
        EbuildShell::new(self).await
    }

    /// Create an [`EbuildShell`] with master repository eclass directories.
    ///
    /// Master eclass directories are prepended (searched first).
    /// See [PMS 4.7](https://projects.gentoo.org/pms/9/pms.html#tree-layout).
    pub async fn shell_with_masters(&self, masters: &[&Repository]) -> Result<EbuildShell> {
        let mut shell = EbuildShell::new(self).await?;
        for master in masters.iter().rev() {
            let dir = master.path().join("eclass");
            if dir.is_dir() {
                shell.prepend_eclass_dir(dir);
            }
        }
        Ok(shell)
    }

    /// Like [`shell_with_masters`](Self::shell_with_masters) but shares an
    /// eclass AST cache across all created shells.
    pub async fn shell_with_masters_and_cache(
        &self,
        masters: &[&Repository],
        cache: Arc<papaya::HashMap<String, brush_parser::ast::Program>>,
    ) -> Result<EbuildShell> {
        let mut shell = EbuildShell::new_with_cache(self, cache).await?;
        for master in masters.iter().rev() {
            let dir = master.path().join("eclass");
            if dir.is_dir() {
                shell.prepend_eclass_dir(dir);
            }
        }
        Ok(shell)
    }

    /// Create an [`EbuildShell`] with a profile's USE configuration applied.
    ///
    /// `profile_rel_path` is relative to the repository's `profiles/` directory.
    /// `make_conf` is an optional `make.conf`-style script sourced after the
    /// profile chain but before `use.force`/`use.mask`.
    ///
    /// See [PMS 5.2](https://projects.gentoo.org/pms/9/pms.html#profiles).
    pub async fn shell_with_profile(
        &self,
        profile_rel_path: &str,
        make_conf: Option<&std::path::Path>,
    ) -> Result<EbuildShell> {
        use crate::repo::profile::ProfileStack;
        let path = self.path().join("profiles").join(profile_rel_path);
        let stack = ProfileStack::build(path.into())?;
        let mut shell = EbuildShell::new(self).await?;
        let confs: Vec<&std::path::Path> = make_conf.into_iter().collect();
        stack.configure_shell(&mut shell, &confs).await?;
        Ok(shell)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_use_flags() {
        let dir = tempdir().unwrap();
        let repo_path = dir.path().to_path_buf();

        // Create a minimal repository structure
        std::fs::create_dir_all(repo_path.join("metadata")).unwrap();
        std::fs::create_dir_all(repo_path.join("profiles")).unwrap();
        std::fs::create_dir_all(repo_path.join("eclass")).unwrap();

        // Write minimal layout.conf
        std::fs::write(
            repo_path.join("metadata").join("layout.conf"),
            "masters = \ncache-formats = md5-dict\n",
        )
        .unwrap();

        // Write repo_name
        std::fs::write(repo_path.join("profiles").join("repo_name"), "test-repo\n").unwrap();

        let repo = Repository::open(&repo_path).unwrap();
        let mut shell = repo.shell().await.unwrap();

        // Test setting USE flags
        shell.set_use_flags(&["ssl", "gtk", "-doc"]).unwrap();
        assert_eq!(shell.use_flags_string(), "gtk ssl");

        // Test that USE environment variable is set
        let use_env = shell.get_var("USE").unwrap_or_default();
        assert!(use_env.contains("ssl"));
        assert!(use_env.contains("gtk"));
        assert!(!use_env.contains("doc"));
    }
}
