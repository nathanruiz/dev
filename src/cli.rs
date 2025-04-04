use std::io::BufRead;
use std::io::Write;
use std::fs::File;

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
    fn prompt(&self, message: &str) -> String {
        print!("{}", message);
        std::io::stdout().flush().unwrap();

        let stdin = std::io::stdin();
        let mut line = String::new();
        stdin.lock().read_line(&mut line).unwrap();
        line.trim().into()
    }

    fn multi_line_prompt(&self) -> Vec<String> {
        let mut lines = Vec::new();
        loop {
            let line = self.prompt("> ");

            if line.is_empty() {
                return lines;
            }

            lines.push(line);
        }
    }

    fn write_lines(&self, output: PathBuf, lines: &[String]) {
        let mut output = File::create(output).unwrap();
        for line in lines {
            writeln!(output, "{}", line).unwrap();
        }
    }

    fn ensure_dir(&self, path: PathBuf) {
        if let Err(e) = std::fs::create_dir(path) {
            match e.kind() {
                std::io::ErrorKind::AlreadyExists => {},
                _ => panic!("{:?}", e),
            }
        };
    }
}

impl Runnable for &InitCommand {
    fn run(self, repo: &Repo, _environment: &Environment<'_>) -> Result<()> {
        println!("Welcome to the dev setup process");
        println!();

        // Create the .dev directory
        let dev_dir = repo.repo_path.join(".dev");
        self.ensure_dir(dev_dir);

        // Create the .dev/developers file
        println!("Enter the ssh keys of all developers that need access to your env files, each");
        println!("on their own lines. Once you are done, enter one blank line:");
        let keys = self.multi_line_prompt();
        let keys_path = repo.repo_path.join(".dev/developers");
        self.write_lines(keys_path, &keys);

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
