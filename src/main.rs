use anyhow::{bail, Context};
use clap::{error::ErrorKind, CommandFactory, Parser as _};
use std::{
    env::{
        self,
        VarError::{NotPresent, NotUnicode},
    },
    ffi::OsString,
    process::{Command, Output, Stdio},
};

#[derive(Debug, clap::Parser)]
#[command(about)]
struct Args {
    /// Run COMMAND in a shell, specified by the SHELL environment variable.
    ///
    /// There must be only one argument.
    #[arg(short, long)]
    shell: bool,
    /// Don't prompt for confirmation before committing.
    #[arg(short, long, alias = "no-confirm")]
    yes: bool,
    /// The command and its arguments.
    ///
    /// The commit message will be `run: [COMMAND]...`.
    #[arg(num_args(1..))]
    command: Vec<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let clean = errexit(run(git()
        .args(["status", "--porcelain"])
        .stdout(Stdio::piped()))?)?
    .stdout
    .is_empty();

    if !clean {
        bail!("git-run performs a `git add .`, but there are dirty or untracked files before running the command.")
    }

    let message = match (args.shell, args.command.as_slice()) {
        (true, [arg]) => {
            let shell = match env::var("SHELL") {
                Ok(s) => OsString::from(s),
                Err(NotUnicode(s)) => s,
                Err(NotPresent) => {
                    bail!("--shell was specified, but the environment variable SHELL is not set")
                }
            };
            errexit(run(visible(
                Command::new(shell).arg("-i").arg("-c").arg(arg),
            ))?)?;
            format!("run: {arg}")
        }
        (true, _) => Args::command()
            .error(
                ErrorKind::ArgumentConflict,
                "when --shell is supplied, COMMAND must be a single string",
            )
            .exit(),
        (false, [first, rest @ ..]) => {
            errexit(run(visible(Command::new(first).args(rest)))?)?;
            format!("run: {}", itertools::join(args.command, " "))
        }
        (false, _) => unreachable!("#[arg(num_args(1..)] prevents us getting here"),
    };

    errexit(run(visible(git().args(["add", "."])))?)?;
    errexit(run(visible(git().args([
        "-c",
        "color.status=always",
        "status",
    ])))?)?;

    let permission = args.yes
        || dialoguer::Confirm::new()
            .default(true)
            .with_prompt(format!("commit with message `{message}`"))
            .interact()
            .unwrap_or(false);

    match permission {
        true => errexit(run(visible(git().args([
            "commit",
            "--message",
            message.as_str(),
        ])))?)?,
        false => bail!("cancelled"),
    };

    Ok(())
}

fn run(command: &mut Command) -> anyhow::Result<(&mut Command, Output)> {
    let (program, args) = get_program_and_args(command);
    let exit_status = command
        .output()
        .with_context(|| format!("couldn't run {program:?} with arguments {args:?}"))?;
    Ok((command, exit_status))
}

fn get_program_and_args(command: &Command) -> (OsString, Vec<OsString>) {
    (
        command.get_program().into(),
        command.get_args().map(Into::into).collect(),
    )
}

fn errexit((command, output): (&mut Command, Output)) -> anyhow::Result<Output> {
    let (program, args) = get_program_and_args(command);
    match output.status.code() {
        Some(0) => Ok(output),
        Some(nonzero) => {
            bail!("program {program:?} with arguments {args:?} failed with status {nonzero}")
        }
        None => bail!("program {program:?} with arguments {args:?} failed with no status"),
    }
}

fn git() -> Command {
    Command::new("git")
}

fn visible(command: &mut Command) -> &mut Command {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
}
