use anstream::AutoStream;
use anyhow::Context;
use owo_colors::OwoColorize;
use std::borrow::Cow;
use std::io::stdout;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fmt::Display, fmt::Write, process::ExitCode};

pub use build_frontend::build_frontend;
pub use cache_clean::cache_clean;
pub use cache_dir::cache_dir;
pub use cache_prune::cache_prune;
pub use help::help;
pub use pip::check::pip_check;
pub use pip::compile::pip_compile;
pub use pip::freeze::pip_freeze;
pub use pip::install::pip_install;
pub use pip::list::pip_list;
pub use pip::show::pip_show;
pub use pip::sync::pip_sync;
pub use pip::tree::pip_tree;
pub use pip::uninstall::pip_uninstall;
pub use project::add::add;
pub use project::export::export;
pub use project::init::{init, InitKind, InitProjectKind};
pub use project::lock::lock;
pub use project::remove::remove;
pub use project::run::{run, RunCommand};
pub use project::sync::sync;
pub use project::tree::tree;
pub use publish::publish;
pub use python::dir::dir as python_dir;
pub use python::find::find as python_find;
pub use python::find::find_script as python_find_script;
pub use python::install::install as python_install;
pub use python::list::list as python_list;
pub use python::pin::pin as python_pin;
pub use python::uninstall::uninstall as python_uninstall;
#[cfg(feature = "self-update")]
pub use self_update::self_update;
pub use tool::dir::dir as tool_dir;
pub use tool::install::install as tool_install;
pub use tool::list::list as tool_list;
pub use tool::run::run as tool_run;
pub use tool::run::ToolRunCommand;
pub use tool::uninstall::uninstall as tool_uninstall;
pub use tool::update_shell::update_shell as tool_update_shell;
pub use tool::upgrade::upgrade as tool_upgrade;
use uv_cache::Cache;
use uv_configuration::Concurrency;
use uv_distribution_types::InstalledMetadata;
use uv_fs::{Simplified, CWD};
use uv_installer::compile_tree;
use uv_normalize::PackageName;
use uv_python::PythonEnvironment;
use uv_scripts::Pep723Script;
pub use venv::venv;
pub use version::version;

use crate::printer::Printer;

pub mod build_backend;
mod build_frontend;
mod cache_clean;
mod cache_dir;
mod cache_prune;
mod diagnostics;
mod help;
pub mod pip;
mod project;
mod publish;
mod python;
pub mod reporters;
mod run;
#[cfg(feature = "self-update")]
mod self_update;
mod tool;
mod venv;
mod version;

#[derive(Copy, Clone)]
pub enum ExitStatus {
    /// The command succeeded.
    Success,

    /// The command failed due to an error in the user input.
    Failure,

    /// The command failed with an unexpected error.
    Error,

    /// The command's exit status is propagated from an external command.
    External(u8),
}

impl From<ExitStatus> for ExitCode {
    fn from(status: ExitStatus) -> Self {
        match status {
            ExitStatus::Success => Self::from(0),
            ExitStatus::Failure => Self::from(1),
            ExitStatus::Error => Self::from(2),
            ExitStatus::External(code) => Self::from(code),
        }
    }
}

/// Format a duration as a human-readable string, Cargo-style.
pub(super) fn elapsed(duration: Duration) -> String {
    let secs = duration.as_secs();
    let ms = duration.subsec_millis();

    if secs >= 60 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else if secs > 0 {
        format!("{}.{:02}s", secs, duration.subsec_nanos() / 10_000_000)
    } else if ms > 0 {
        format!("{ms}ms")
    } else {
        format!("0.{:02}ms", duration.subsec_nanos() / 10_000)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub(super) enum ChangeEventKind {
    /// The package was removed from the environment.
    Removed,
    /// The package was added to the environment.
    Added,
    /// The package was reinstalled without changing versions.
    Reinstalled,
}

#[derive(Debug)]
pub(super) struct ChangeEvent<'a, T: InstalledMetadata> {
    dist: &'a T,
    kind: ChangeEventKind,
}

#[derive(Debug)]
pub(super) struct DryRunEvent<T: Display> {
    name: PackageName,
    version: T,
    kind: ChangeEventKind,
}

/// Compile all Python source files in site-packages to bytecode, to speed up the
/// initial run of any subsequent executions.
///
/// See the `--compile` option on `pip sync` and `pip install`.
pub(super) async fn compile_bytecode(
    venv: &PythonEnvironment,
    concurrency: &Concurrency,
    cache: &Cache,
    printer: Printer,
) -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    let mut files = 0;
    for site_packages in venv.site_packages() {
        let site_packages = CWD.join(site_packages);
        files += compile_tree(
            &site_packages,
            venv.python_executable(),
            concurrency,
            cache.root(),
        )
        .await
        .with_context(|| {
            format!(
                "Failed to bytecode-compile Python file in: {}",
                site_packages.user_display()
            )
        })?;
    }
    let s = if files == 1 { "" } else { "s" };
    writeln!(
        printer.stderr(),
        "{}",
        format!(
            "Bytecode compiled {} {}",
            format!("{files} file{s}").bold(),
            format!("in {}", elapsed(start.elapsed())).dimmed()
        )
        .dimmed()
    )?;
    Ok(())
}

/// Formats a number of bytes into a human readable SI-prefixed size.
///
/// Returns a tuple of `(quantity, units)`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
pub(super) fn human_readable_bytes(bytes: u64) -> (f32, &'static str) {
    static UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let bytes = bytes as f32;
    let i = ((bytes.log2() / 10.0) as usize).min(UNITS.len() - 1);
    (bytes / 1024_f32.powi(i as i32), UNITS[i])
}

/// A multicasting writer that writes to both the standard output and an output file, if present.
#[allow(clippy::disallowed_types)]
struct OutputWriter<'a> {
    stdout: Option<AutoStream<std::io::Stdout>>,
    output_file: Option<&'a Path>,
    buffer: Vec<u8>,
}

#[allow(clippy::disallowed_types)]
impl<'a> OutputWriter<'a> {
    /// Create a new output writer.
    fn new(include_stdout: bool, output_file: Option<&'a Path>) -> Self {
        let stdout = include_stdout.then(|| AutoStream::<std::io::Stdout>::auto(stdout()));
        Self {
            stdout,
            output_file,
            buffer: Vec::new(),
        }
    }

    /// Write the given arguments to both standard output and the output buffer, if present.
    fn write_fmt(&mut self, args: std::fmt::Arguments<'_>) -> std::io::Result<()> {
        use std::io::Write;

        // Write to the buffer.
        if self.output_file.is_some() {
            self.buffer.write_fmt(args)?;
        }

        // Write to standard output.
        if let Some(stdout) = &mut self.stdout {
            write!(stdout, "{args}")?;
        }

        Ok(())
    }

    /// Commit the buffer to the output file.
    async fn commit(self) -> std::io::Result<()> {
        if let Some(output_file) = self.output_file {
            if let Some(parent_dir) = output_file.parent() {
                fs_err::create_dir_all(parent_dir)?;
            }

            // If the output file is an existing symlink, write to the destination instead.
            let output_file = fs_err::read_link(output_file)
                .map(Cow::Owned)
                .unwrap_or(Cow::Borrowed(output_file));
            let stream = anstream::adapter::strip_bytes(&self.buffer).into_vec();
            uv_fs::write_atomic(output_file, &stream).await?;
        }
        Ok(())
    }
}

/// Given a list of names, return a conjunction of the names (e.g., "Alice, Bob, and Charlie").
pub(super) fn conjunction(names: Vec<String>) -> String {
    let mut names = names.into_iter();
    let first = names.next();
    let last = names.next_back();
    match (first, last) {
        (Some(first), Some(last)) => {
            let mut result = first;
            let mut comma = false;
            for name in names {
                result.push_str(", ");
                result.push_str(&name);
                comma = true;
            }
            if comma {
                result.push_str(", and ");
            } else {
                result.push_str(" and ");
            }
            result.push_str(&last);
            result
        }
        (Some(first), None) => first,
        _ => String::new(),
    }
}

/// Capitalize the first letter of a string.
pub(super) fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// A Python file that may or may not include an existing PEP 723 script tag.
#[derive(Debug)]
pub enum ScriptPath {
    /// The Python file already includes a PEP 723 script tag.
    Script(Pep723Script),
    /// The Python file does not include a PEP 723 script tag.
    Path(PathBuf),
}
