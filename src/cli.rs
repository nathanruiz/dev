use clap::{Parser, Args};

use toml::{self, Value};

use crate::error::*;
use crate::*;


#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[arg(global = true, short, long, default_value = "local")]
    pub environment: String,

    #[command(subcommand)]
    pub command: Commands,
}

trait Runnable {
    fn run(&self, environment: &str) -> Result<()>;
}

// dev ...
#[derive(Subcommand)]
pub enum Commands {
    Run(RunCommand),
    Start(StartCommand),
    Init(InitCommand),
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

impl Runnable for Commands {
    fn run(&self, environment: &str) -> Result<()> {
        match self {
            Self::Run(cmd) => cmd.run(environment),
            Self::Config { command } => command.run(environment),
            Self::Start(cmd) => cmd.run(environment),
            Self::Init(cmd) => cmd.run(environment),
        }
    }
}

// dev run <command> [args]
#[derive(Args)]
pub struct RunCommand {
    command: String,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    args: Vec<String>,
}

impl Runnable for RunCommand {
    fn run(&self, environment: &str) -> Result<()> {
        let args = self.args.iter().map(String::as_str).collect();
        run_command(environment, self.command.as_str(), args)
    }
}

// dev start
#[derive(Args)]
pub struct StartCommand;

impl Runnable for StartCommand {
    fn run(&self, environment: &str) -> Result<()> {
        run_command(environment, "nix", vec!["run", ".#dev.start"])
    }
}

// dev init
#[derive(Args)]
pub struct InitCommand;

impl Runnable for InitCommand {
    fn run(&self, _environment: &str) -> Result<()> {
        todo!()
    }
}

// dev config ...
#[derive(Subcommand)]
pub enum ConfigCommands {
    Export(ConfigExportCommand),
    Edit(ConfigEditCommand),
}

impl Runnable for ConfigCommands {
    fn run(&self, environment: &str) -> Result<()> {
        match self {
            Self::Export(cmd) => cmd.run(environment),
            Self::Edit(cmd) => cmd.run(environment),
        }
    }
}

// dev config export ...
#[derive(Args)]
struct ConfigExportCommand {
    #[arg(short, long, value_enum, default_value_t = ConfigExportFormat::Raw)]
    format: ConfigExportFormat,
}

impl Runnable for ConfigExportCommand {
    fn run(&self, environment: &str) -> Result<()> {
        let repo = Repo::new()?;
        let env = repo.get_environment(environment.into());
        match self.format {
            ConfigExportFormat::Raw => {
                let mut file = env.decrypt()?;
                std::io::copy(&mut file, &mut std::io::stdout()).unwrap();
            },
            ConfigExportFormat::Json => {
                let values = env.values()?;
                let json = serde_json::to_string_pretty(&values).unwrap();
                println!("{}", json);
            },
            ConfigExportFormat::Docker => {
                for (key, value) in env.values()? {
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

impl ConfigCommands {
    fn run(&self, environment: &str) -> Result<()> {
        let repo = Repo::new()?;
        let env = repo.get_environment(environment.into());
        env.edit()
    }
}

fn run_command(environment: &str, command: &str, args: Vec<&str>) -> Result<()> {
    let repo = Repo::new()?;
    let env = repo.get_environment(environment.into());
    env.exec(command, args)
}


#[derive(clap::ValueEnum, Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum ConfigExportFormat {
    #[default]
    Raw,
    Json,
    Docker,
}
