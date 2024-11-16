use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::os::unix::process::CommandExt;
use std::collections::HashMap;
use std::fmt;
use std::io;

use clap::{Parser, Subcommand};

use toml::{self, Value};
use tempfile::NamedTempFile;

#[derive(Debug)]
enum AppError {
    /// Failed to run a git command.
    GitError(CommandError),
    /// Failed to decrypt the config file.
    AgeDecryptError(CommandError),
    /// Failed to encrypt the config file.
    AgeEncryptError(CommandError),
    /// Failed to verify the checksum of the config file.
    ChecksumError(CommandError),
    /// Failed to modify the config file in an editor.
    EditorError(CommandError),
    /// Failed to parse the environment config file.
    ConfigParseError(toml::de::Error),
    /// Failed to run a command.
    RunError(Vec<String>, CommandError),
}

#[derive(Debug)]
enum CommandError {
    /// The command failed to spawn.
    SpawnError(io::Error),
    /// The command failed with an error message.
    FailedError {
        status: std::process::ExitStatus,
        stderr: Option<String>,
    },
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::GitError(cause) => write!(f, "Failed to run git: {}", cause),
            AppError::AgeDecryptError(cause) => write!(f, "Failed to run age decrypt: {}", cause),
            AppError::AgeEncryptError(cause) => write!(f, "Failed to run age encrypt: {}", cause),
            AppError::ChecksumError(cause) => write!(f, "Failed to run checksum: {}", cause),
            AppError::EditorError(cause) => write!(f, "Failed to run editor: {}", cause),
            AppError::ConfigParseError(cause) => write!(f, "Failed to parse config: {}", cause),
            AppError::RunError(command, cause) => write!(f, "Failed to run command '{}': {}", command.join(" "), cause),
        }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::SpawnError(e) => write!(f, "{}", e),
            CommandError::FailedError { status, stderr } => match stderr {
                Some(stderr) => write!(f, "Command failed with status code {}:\n{}", status, stderr),
                None => write!(f, "Command failed with status code {}", status),
            }
        }
    }
}

impl std::error::Error for AppError {}

type Result<T> = std::result::Result<T, AppError>;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(global = true, short, long, default_value = "local")]
    environment: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
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
}

impl Commands {
    fn run(&self, environment: &str) -> Result<()> {
        match self {
            Self::Run { command, args } => {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                self.run_command(environment, command.as_str(), &args)?;
            },
            Self::Start { } => {
                self.run_command(environment, "nix", &["run", ".#dev.start"])?;
            },
            Self::Config { command } => {
                command.run(environment)?;
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
enum ConfigCommands {
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
enum ConfigExportFormat {
    #[default]
    Raw,
    Json,
    Docker,
}

struct EnvironmentConfig {
    env_path: PathBuf,
    home: String,
    repo_path: PathBuf,
}

impl EnvironmentConfig {
    pub fn from_env(environment: &str) -> Result<Self> {
        let repo_path = Self::get_repo_path()?;
        Ok(Self {
            env_path: Self::path_from_env(environment, &repo_path),
            home: std::env::var("HOME").unwrap(),
            repo_path,
        })
    }

    fn get_repo_path() -> Result<PathBuf> {
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .map_err(|e| AppError::GitError(CommandError::SpawnError(e)))?;

        if !output.status.success() {
            return Err(AppError::GitError(CommandError::FailedError {
                status: output.status,
                stderr: Some(String::from_utf8_lossy(&output.stderr).to_string()),
            }));
        }

        let path = std::str::from_utf8(&output.stdout).unwrap();
        Ok(path.trim().into())
    }

    fn path_from_env(environment: &str, repo_path: &Path) -> PathBuf {
        let name = format!(".dev/env.age.{}", environment);
        repo_path.join(name)
    }

    fn keys_path(&self) -> PathBuf {
        self.repo_path.join(".dev/developers")
    }

    pub fn decrypt(&self) -> Result<NamedTempFile> {
        let name = self.env_path.file_name().unwrap();
        let file = NamedTempFile::with_suffix(name).unwrap();

        if std::fs::exists(&self.env_path).unwrap() {
            let output = Command::new("age")
                .args(["-d"])
                .args(["-i", &format!("{}/.ssh/id_ed25519", self.home)])
                .args(["-o", file.path().to_str().unwrap()])
                .args(["--", self.env_path.to_str().unwrap()])
                .output()
                .map_err(|e| AppError::AgeDecryptError(CommandError::SpawnError(e)))?;

            if !output.status.success() {
                return Err(AppError::AgeDecryptError(CommandError::FailedError {
                    status: output.status,
                    stderr: Some(String::from_utf8_lossy(&output.stderr).to_string()),
                }));
            }
        }

        Ok(file)
    }

    pub fn encrypt(&self, file: &NamedTempFile) -> Result<()> {
        let output = Command::new("age")
            .args(["-e", "-a"])
            .args(["-R", self.keys_path().to_str().unwrap()])
            .args(["-o", self.env_path.to_str().unwrap()])
            .args(["--", file.path().to_str().unwrap()])
            .output()
            .map_err(|e| AppError::AgeEncryptError(CommandError::SpawnError(e)))?;

        if !output.status.success() {
            return Err(AppError::AgeEncryptError(CommandError::FailedError {
                status: output.status,
                stderr: Some(String::from_utf8_lossy(&output.stderr).to_string()),
            }));
        }

        Ok(())
    }

    fn calculate_checksum(&self, file: &NamedTempFile) -> Result<String> {
        let output = Command::new("sha256sum")
            .args(["--", file.path().to_str().unwrap()])
            .output()
            .map_err(|e| AppError::ChecksumError(CommandError::SpawnError(e)))?;

        if !output.status.success() {
            return Err(AppError::ChecksumError(CommandError::FailedError {
                status: output.status,
                stderr: Some(String::from_utf8_lossy(&output.stderr).to_string()),
            }));
        }

        let path = std::str::from_utf8(&output.stdout).unwrap();
        let (hash, _) = path.split_once(" ").unwrap();
        Ok(hash.into())
    }

    fn run_editor(&self, file: &NamedTempFile) -> Result<()> {
        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
        let path = file.path()
            .to_str()
            .unwrap()
            .replace("'", "'\\''");

        let status = Command::new("bash")
            .args(["-c", &format!("{} -- '{}'", editor, path)])
            .status()
            .map_err(|e| AppError::EditorError(CommandError::SpawnError(e)))?;

        if !status.success() {
            return Err(AppError::EditorError(CommandError::FailedError {
                status,
                stderr: None,
            }));
        }

        Ok(())
    }

    pub fn edit(&self) -> Result<()> {
        let file = self.decrypt()?;

        let old_hash = self.calculate_checksum(&file)?;

        self.run_editor(&file)?;

        let new_hash = self.calculate_checksum(&file)?;

        // Only encrypt the file if the content has changed from the original,
        // since re-encrypting the same file will result in a different
        // encrypted result, which can be avoided.
        if old_hash != new_hash {
            self.encrypt(&file)?;
        }

        Ok(())
    }

    pub fn values(&self) -> Result<HashMap<String, Value>> {
        let file = self.decrypt()?;
        let content = std::fs::read_to_string(file).unwrap();
        toml::from_str(&content).map_err(AppError::ConfigParseError)
    }
}

fn main() {
    let cli = Cli::parse();

    // You can check for the existence of subcommands, and if found use their
    // matches just as you would the top level cmd.
    if let Err(e) = cli.command.run(cli.environment.as_str()) {
        let arg0 = std::env::args().next().unwrap();
        eprintln!("{}: {}", arg0, e);
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::io::Write;
    use tempfile::{TempDir, NamedTempFile};

    const PUBLIC_KEY: &str = "
ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMKcaO+SsZg1StalnVVX+nei1oqLT/ShJTleGpucGUt5 testkey
    ";
    const PRIVATE_KEY: &str = "
-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACDCnGjvkrGYNUrWpZ1VV/p3otaKi0/0oSU5XhqbnBlLeQAAAJCori2BqK4t
gQAAAAtzc2gtZWQyNTUxOQAAACDCnGjvkrGYNUrWpZ1VV/p3otaKi0/0oSU5XhqbnBlLeQ
AAAED75GvIoqmYJAe9EVTIJ1RyG6jQwxp4IaKtOuhyKmQ1lcKcaO+SsZg1StalnVVX+nei
1oqLT/ShJTleGpucGUt5AAAAB3Rlc3RrZXkBAgMEBQY=
-----END OPENSSH PRIVATE KEY-----
    ";

    struct TestSetup {
        _temp_dir: TempDir,
        env_config: EnvironmentConfig,
    }

    impl TestSetup {
        fn new() -> Self {
            let temp_dir = TempDir::new().unwrap();
            let path: PathBuf = temp_dir.path().into();
            Command::new("git")
                .args(["-C", path.to_str().unwrap(), "init"])
                .output()
                .unwrap();

            // Create a dummy developers file
            std::fs::create_dir(path.join(".dev")).unwrap();
            std::fs::write(path.join(".dev/developers"), PUBLIC_KEY.trim()).unwrap();

            // Write ssh keys to fake home directory
            std::fs::create_dir(path.join(".ssh")).unwrap();
            std::fs::write(path.join(".ssh/id_ed25519.pub"), PUBLIC_KEY.trim()).unwrap();
            std::fs::write(path.join(".ssh/id_ed25519"), PRIVATE_KEY.trim()).unwrap();

            let env_path = path.join(".dev/env.age.local");

            Self {
                _temp_dir: temp_dir,
                env_config: EnvironmentConfig {
                    env_path,
                    home: path.to_str().unwrap().into(),
                    repo_path: path.into(),
                },
            }
        }
    }

    #[test]
    fn test_get_repo_path_success() {
        TestSetup::new();
        EnvironmentConfig::get_repo_path().unwrap();
    }

    #[test]
    fn test_encrypt_decrypt() {
        let setup = TestSetup::new();

        // Encrypt "test content"
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "test content").unwrap();
        setup.env_config.encrypt(&file).unwrap();

        // Decrypt the encrypted file
        let file = setup.env_config.decrypt().unwrap();
        let content = fs::read_to_string(file.path()).unwrap();

        // Decrypted content should be the same as the original content
        assert_eq!(content, "test content\n");

        // Encrypted file should not contain the original content
        let content = fs::read_to_string(&setup.env_config.env_path).unwrap();
        assert!(!content.contains("test content"));
    }

    #[test]
    fn test_calculate_checksum_success() {
        let setup = TestSetup::new();
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "test content").unwrap();

        let checksum = setup.env_config.calculate_checksum(&file).unwrap();
        assert_eq!(checksum, "a1fff0ffefb9eace7230c24e50731f0a91c62f9cefdfe77121c2f607125dffae");
    }

    #[test]
    fn test_run_editor_success() {
        let setup = TestSetup::new();
        let file = NamedTempFile::new().unwrap();

        env::set_var("EDITOR", "true");
        setup.env_config.run_editor(&file).unwrap();
    }

    #[test]
    fn test_run_editor_failure() {
        let setup = TestSetup::new();
        let file = NamedTempFile::new().unwrap();

        env::set_var("EDITOR", "false");
        let result = setup.env_config.run_editor(&file);

        assert!(result.is_err());
        if let Err(AppError::EditorError(CommandError::FailedError { status, .. })) = result {
            assert!(!status.success());
        } else {
            panic!("Expected EditorError with FailedError");
        }
    }
}
