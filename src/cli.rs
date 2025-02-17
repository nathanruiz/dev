use clap::{Parser, Args};

use toml::{self, Value};

use crate::error::*;
use crate::*;


/// The missing tool for 12 factor development environments.
#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[arg(global = true, short, long, default_value = "local")]
    environment: String,

    #[command(subcommand)]
    command: Commands,
}

impl Cli {
    pub fn run(&self) -> Result<()> {
        let repo = Repo::new()?;
        let environment = repo.get_environment(self.environment.clone());
        (&self.command).run(&repo, &environment)
    }
}

trait Runnable {
    fn run<'a>(self, repo: &Repo, environment: &Environment<'a>) -> Result<()>;
}

// dev ...
#[derive(Subcommand)]
enum Commands {
    /// Run a command inside a specified environment.
    Run(RunCommand),
    /// Run the main service(s) for this project.
    Start(StartCommand),
    /// Initial dev tool files in a git repo.
    Init(InitCommand),
    /// Interact with environment variables in an environment.
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

impl Runnable for &Commands {
    fn run<'a>(self, repo: &Repo, environment: &Environment<'a>) -> Result<()> {
        match self {
            Commands::Run(cmd) => cmd.run(repo, environment),
            Commands::Config { command } => command.run(repo, environment),
            Commands::Start(cmd) => cmd.run(repo, environment),
            Commands::Init(cmd) => cmd.run(repo, environment),
        }
    }
}

// dev run <command> [args]
#[derive(Args)]
struct RunCommand {
    /// The path of the command to execute.
    command: String,
    /// Any arguments to be passed into the command.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

impl Runnable for &RunCommand {
    fn run<'a>(self, _repo: &Repo, environment: &Environment<'a>) -> Result<()> {
        let args = self.args.iter()
            .map(String::as_str)
            .collect();
        environment.exec(self.command.as_str(), args)
    }
}

// dev start
#[derive(Args)]
struct StartCommand;

impl Runnable for &StartCommand {
    fn run<'a>(self, _repo: &Repo, environment: &Environment<'a>) -> Result<()> {
        environment.exec("nix", vec!["run", ".#dev.start"])
    }
}

// dev init
#[derive(Args)]
struct InitCommand;

impl Runnable for &InitCommand {
    fn run<'a>(self, _repo: &Repo, _environment: &Environment<'a>) -> Result<()> {
        todo!()
    }
}

// dev config ...
#[derive(Subcommand)]
enum ConfigCommands {
    /// Export encrypted environment variables for use by other tools.
    Export(ConfigExportCommand),
    /// Decrypt and open the environment variable file in your default editor.
    Edit(ConfigEditCommand),
}

impl Runnable for &ConfigCommands {
    fn run<'a>(self, repo: &Repo, environment: &Environment<'a>) -> Result<()> {
        match self {
            ConfigCommands::Export(cmd) => cmd.run(repo, environment),
            ConfigCommands::Edit(cmd) => cmd.run(repo, environment),
        }
    }
}

// dev config export ...
#[derive(Args)]
struct ConfigExportCommand {
    #[arg(short, long, value_enum, default_value_t = ConfigExportFormat::Raw)]
    format: ConfigExportFormat,
}

impl Runnable for &ConfigExportCommand {
    fn run<'a>(self, _repo: &Repo, environment: &Environment<'a>) -> Result<()> {
        match self.format {
            ConfigExportFormat::Raw => {
                let mut file = environment.decrypt()?;
                std::io::copy(&mut file, &mut std::io::stdout()).unwrap();
            },
            ConfigExportFormat::Json => {
                let values = environment.values()?;
                let json = serde_json::to_string_pretty(&values).unwrap();
                println!("{}", json);
            },
            ConfigExportFormat::Docker => {
                for (key, value) in environment.values()? {
                    let value = match value {
                        Value::String(value) => value,
                        value => serde_json::to_string(&value).unwrap(),
                    };
                    // Docker env files don't support newlines in environment
                    // variable values. We replace them with spaces to attempt
                    // to allow it to still work if the use case doesn't require
                    // the newlines.
                    let value = value.replace("\n", " ");
                    println!("{}={}", key, value);
                }
            },
        };
        Ok(())
    }
}

// dev config edit ...
#[derive(Args)]
struct ConfigEditCommand;

impl Runnable for &ConfigEditCommand {
    fn run<'a>(self, _repo: &Repo, environment: &Environment<'a>) -> Result<()> {
        environment.edit()
    }
}

#[derive(clap::ValueEnum, Copy, Clone, Debug, Default, PartialEq, Eq)]
enum ConfigExportFormat {
    #[default]
    Raw,
    Json,
    Docker,
}
