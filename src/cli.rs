use clap::Parser;

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

#[derive(Subcommand)]
pub enum Commands {
    Run {
        command: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
        args: Vec<String>,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    Start { },
    Init { },
}

impl Commands {
    pub fn run(&self, environment: &str) -> Result<()> {
        match self {
            Self::Run { command, args } => {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                self.run_command(environment, command.as_str(), &args)?;
            },
            Self::Config { command } => {
                command.run(environment)?;
            },
            Self::Start { } => {
                self.run_command(environment, "nix", &["run", ".#dev.start"])?;
            },
            Self::Init { } => {
                //File::create_directory().unwrap();
                //File::create();
                // Create
            },
        };
        Ok(())
    }

    fn run_command(&self, environment: &str, command: &str, args: &[&str]) -> Result<()> {
        let mut process = Command::new(command);
        for arg in args {
            process.arg(arg);
        }

        let config = EnvironmentConfig::from_env(environment)?;
        for (key, value) in config.values()? {
            match value {
                Value::String(value) => process.env(key, value),
                value => process.env(key, value.to_string()),
            };
        }

        let err = process.exec();

        let mut command = vec![command];
        command.extend(args);
        let command = command.into_iter()
            .map(|s| s.into())
            .collect();

        Err(AppError::RunError(command, CommandError::SpawnError(err)))
    }
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    Export {
        #[arg(short, long, value_enum, default_value_t = ConfigExportFormat::Raw)]
        format: ConfigExportFormat,
    },
    Edit {},
}

impl ConfigCommands {
    fn run(&self, environment: &str) -> Result<()> {
        match self {
            Self::Export { format } => {
                self.export_command(environment, *format)?;
            },
            Self::Edit { } => {
                self.edit_command(environment)?;
            },
        };
        Ok(())
    }

    fn export_command(&self, environment: &str, format: ConfigExportFormat) -> Result<()> {
        let env = EnvironmentConfig::from_env(environment)?;
        match format {
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

    fn edit_command(&self, environment: &str) -> Result<()> {
        let env = EnvironmentConfig::from_env(environment)?;
        env.edit()?;
        Ok(())
    }
}

#[derive(clap::ValueEnum, Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum ConfigExportFormat {
    #[default]
    Raw,
    Json,
    Docker,
}
