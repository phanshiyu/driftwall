//! driftwall command line entry point.
//!
//! This is the only place that prints, and the only place that turns errors into exit codes.

mod args;
mod commands;
mod diff;
mod error_chain;
mod git;
mod locate;
mod outcome;
mod output;
mod render;
mod views;

use std::process::ExitCode;

use clap::Parser;

use args::Cli;
use outcome::Outcome;

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(usage) => {
            let text = usage.render().to_string();
            // why: --help and --version are not failures; real usage errors are tool errors (2).
            return if usage.use_stderr() {
                output::stderr(&text);
                ExitCode::from(Outcome::ToolError.code())
            } else {
                finish(output::stdout(&text), Outcome::Clean)
            };
        }
    };
    match commands::execute(&cli.command) {
        Ok(run) => finish(output::stdout(&run.stdout), run.outcome),
        Err(error) => fail(&error),
    }
}

/// Maps the result of the final stdout write onto an exit code.
fn finish(written: std::io::Result<()>, outcome: Outcome) -> ExitCode {
    match written {
        Ok(()) => ExitCode::from(outcome.code()),
        Err(error) => fail(&anyhow::Error::new(error).context("cannot write to stdout")),
    }
}

/// Prints `error` with its whole source chain to stderr and returns the tool-error exit code.
fn fail(error: &anyhow::Error) -> ExitCode {
    let messages = error.chain().map(ToString::to_string);
    output::stderr(&format!(
        "driftwall: error: {}\n",
        error_chain::join(messages)
    ));
    ExitCode::from(Outcome::ToolError.code())
}
