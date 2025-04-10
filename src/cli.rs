use std::io::Write;
use std::fs::File;

use clap::{Parser, Args};
use inquire::{self, Confirm, Text};
use inquire::error::InquireResult;
use inquire::ui::RenderConfig;
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
    fn run(self, repo: &Repo, environment: &Environment<'_>) -> Result<()>;
}

// dev ...
#[derive(Subcommand)]
enum Commands {
    /// Run a command inside a specified environment.
    Run(RunCommand),
    /// Run the main service(s) for this project.
    Start(StartCommand),
    /// Run all CI checks enabled for this project.
    Check(CheckCommand),
    /// Initial dev tool files in a git repo.
    Init(InitCommand),
    /// Interact with environment variables in an environment.
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

impl Runnable for &Commands {
    fn run(self, repo: &Repo, environment: &Environment<'_>) -> Result<()> {
        match self {
            Commands::Run(cmd) => cmd.run(repo, environment),
            Commands::Config { command } => command.run(repo, environment),
            Commands::Start(cmd) => cmd.run(repo, environment),
            Commands::Check(cmd) => cmd.run(repo, environment),
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
    fn run(self, repo: &Repo, environment: &Environment<'_>) -> Result<()> {
        let mut args: Vec<&str> = self.args.iter()
            .map(String::as_str)
            .collect();
        if let Some(commands) = &repo.config.commands {
            if let Some(shell) = &commands.shell {
                args.insert(0, self.command.as_str());
                args.insert(0, "--");
                args.insert(0, shell);
                args.insert(0, "-ce");
                return environment.exec("bash", args);
            }
        }
        environment.exec(self.command.as_str(), args)
    }
}

// dev start
#[derive(Args)]
struct StartCommand;

impl Runnable for &StartCommand {
    fn run(self, repo: &Repo, environment: &Environment<'_>) -> Result<()> {
        if let Some(commands) = &repo.config.commands {
            if let Some(start) = &commands.start {
                return environment.exec("bash", vec!["-ce", &start]);
            }
        }
        Err(AppError::ConfigMissing("commands.start".into()))
    }
}

// dev check
#[derive(Args)]
struct CheckCommand;

impl Runnable for &CheckCommand {
    fn run(self, repo: &Repo, _environment: &Environment<'_>) -> Result<()> {
        if let Some(commands) = &repo.config.commands {
            if let Some(checks) = &commands.checks {
                for (name, check) in checks {
                    eprintln!("Running {} check...", name);
                    let mut command = Command::new("bash");
                    command.arg("-ce");
                    command.arg(check);

                    let result = match command.status() {
                        Ok(status) if status.success() => Ok(()),
                        Ok(status) => Err(CommandError::FailedError {
                            status,
                            stderr: None,
                        }),
                        Err(err) => Err(CommandError::SpawnError(err)),
                    };
                    let command = vec!["bash".into(), "-ce".into(), check.into()];
                    result.map_err(|err| AppError::RunError(command, err))?;
                }
                eprintln!("All checks passed!");
                return Ok(());
            }
        }
        Err(AppError::ConfigMissing("commands.checks".into()))
    }
}

// dev init
#[derive(Args)]
struct InitCommand;

impl InitCommand {
    fn ensure_dir(&self, path: PathBuf) {
        if let Err(e) = std::fs::create_dir(path) {
            match e.kind() {
                std::io::ErrorKind::AlreadyExists => {},
                _ => panic!("{:?}", e),
            }
        };
    }

    fn prompt_for_ssh_keys(&self, repo: &Repo) -> InquireResult<()> {
        let keys_path = repo.repo_path.join(".dev/developers");
        let mut output = File::create(keys_path).unwrap();
        println!("This tool uses SSH keys to encrypt environment variables.");
        let mut more = Confirm::new("Do you want to add any SSH keys?")
            .with_default(true)
            .prompt()?;

        while more {
            let key = Text::new("Enter your SSH public key:").prompt()?;
            writeln!(output, "{}", key).unwrap();
            more = Confirm::new("Do you want to add more SSH keys?")
                .with_default(true)
                .prompt()?;
        }

        Ok(())
    }
}

impl Runnable for &InitCommand {
    fn run(self, repo: &Repo, _environment: &Environment<'_>) -> Result<()> {
        let render_config = RenderConfig::default();
        inquire::set_global_render_config(render_config);

        println!("Welcome to the dev setup process");
        println!();

        // Create the .dev directory
        let dev_dir = repo.repo_path.join(".dev");
        self.ensure_dir(dev_dir);

        // Create the .dev/developers file
        self.prompt_for_ssh_keys(repo).unwrap();

        Ok(())
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
    fn run(self, repo: &Repo, environment: &Environment<'_>) -> Result<()> {
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
    fn run(self, _repo: &Repo, environment: &Environment<'_>) -> Result<()> {
        match self.format {
            ConfigExportFormat::Raw => {
                ConfigExportCommand::format_raw(environment, &mut std::io::stdout())
            },
            ConfigExportFormat::Json => {
                ConfigExportCommand::format_json(environment, &mut std::io::stdout())
            },
            ConfigExportFormat::Docker => {
                ConfigExportCommand::format_docker(environment, &mut std::io::stdout())
            },
        }
    }
}

impl ConfigExportCommand {
    fn format_raw<W: Write>(environment: &Environment<'_>, out: &mut W) -> Result<()> {
        let mut file = environment.decrypt().map_err(AppError::AgeDecryptError)?;
        std::io::copy(&mut file, out).unwrap();
        Ok(())
    }

    fn format_json<W: Write>(environment: &Environment<'_>, out: &mut W) -> Result<()> {
        let values = environment.values()?;
        serde_json::to_writer_pretty(out, &values).unwrap();
        Ok(())
    }

    fn format_docker<W: Write>(environment: &Environment<'_>, out: &mut W) -> Result<()> {
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
            writeln!(out, "{}={}", key, value).unwrap();
        }
        Ok(())
    }
}

// dev config edit ...
#[derive(Args)]
struct ConfigEditCommand;

impl Runnable for &ConfigEditCommand {
    fn run(self, _repo: &Repo, environment: &Environment<'_>) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::TestSetup;

    fn set_envs(setup: &mut TestSetup) {
        let env = setup.env();
        let mut file = env.decrypt().unwrap();
        writeln!(file, "ABC=123").unwrap();
        writeln!(file, "{}", "TEST = { b = 2, a = 1 }").unwrap();
        file.flush().unwrap();
        env.encrypt(&file).unwrap();
    }

    #[test]
    fn test_config_export_raw_format() {
        let mut setup = TestSetup::new();
        set_envs(&mut setup);
        let mut output = Vec::new();

        ConfigExportCommand::format_raw(&setup.env(), &mut output).unwrap();

        assert_eq!(&output, b"ABC=123\nTEST = { b = 2, a = 1 }\n");
    }

    #[test]
    fn test_config_export_json_format() {
        let mut setup = TestSetup::new();
        set_envs(&mut setup);
        let mut output = Vec::new();

        ConfigExportCommand::format_json(&setup.env(), &mut output).unwrap();

        assert_eq!(&output, br#"{
  "ABC": 123,
  "TEST": {
    "a": 1,
    "b": 2
  }
}"#)
    }

    #[test]
    fn test_config_export_docker_format() {
        let mut setup = TestSetup::new();
        set_envs(&mut setup);
        let mut output = Vec::new();

        ConfigExportCommand::format_docker(&setup.env(), &mut output).unwrap();

        assert_eq!(&output, b"ABC=123\nTEST={\"a\":1,\"b\":2}\n");
    }
}
