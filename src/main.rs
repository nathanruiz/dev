mod error;
mod cli;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Seek, SeekFrom};
use std::iter;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

use clap::{Parser, Subcommand};

use age::{Decryptor, Encryptor};
use age::armor::{ArmoredReader, ArmoredWriter};
use age::ssh::{Identity, Recipient};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use toml::{self, Value};

use error::*;
use cli::*;

#[derive(Deserialize, Serialize)]
struct Commands {
    start: Option<String>,
    shell: Option<String>,
    checks: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize, Serialize)]
struct Config {
    commands: Option<Commands>,
    keys: Option<BTreeMap<String, Vec<String>>>,
}

struct Repo {
    home: String,
    repo_path: PathBuf,
    config: Config,
}

impl Repo {
    pub fn new() -> Result<Self> {
        let repo_path = Self::get_repo_path()?;
        let config_path = repo_path.join(".dev/config.toml");
        let config = if config_path.is_file() {
            let content = std::fs::read_to_string(config_path).unwrap();
            toml::from_str(&content).unwrap()
        } else {
            Config {
                commands: None,
                keys: None,
            }
        };
        Ok(Self {
            home: std::env::var("HOME").unwrap(),
            repo_path,
            config,
        })
    }

    fn get_repo_path() -> Result<PathBuf> {
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .map_err(CommandError::SpawnError)
            .map_err(AppError::GitError)?;

        if !output.status.success() {
            return Err(AppError::GitError(CommandError::FailedError {
                status: output.status,
                stderr: Some(String::from_utf8_lossy(&output.stderr).to_string()),
            }));
        }

        let path = std::str::from_utf8(&output.stdout).unwrap();
        Ok(path.trim().into())
    }

    pub fn get_environment(&self, name: String) -> Environment<'_> {
        Environment {
            name,
            repo: self,
        }
    }
}

struct Environment<'a> {
    name: String,
    repo: &'a Repo,
}

impl Environment<'_> {
    fn path(&self) -> PathBuf {
        let name = format!(".dev/env.age.{}", self.name);
        self.repo.repo_path.join(name)
    }

    pub fn decrypt(&self) -> std::result::Result<NamedTempFile, AgeDecryptError> {
        let env_path = self.path();
        let mut output = match env_path.file_name() {
            Some(name) => NamedTempFile::with_suffix(name)?,
            None => NamedTempFile::with_suffix("dev.toml")?,
        };

        let private_key_path = format!("{}/.ssh/id_ed25519", self.repo.home);
        let private_key = File::open(private_key_path)?;
        let private_key = BufReader::new(private_key);
        let private_key = Identity::from_buffer(private_key, None)?;

        if std::fs::exists(&env_path)? {
            let input = File::open(env_path)?;
            let decryptor = Decryptor::new(ArmoredReader::new(input))?;
            let mut input = decryptor.decrypt(iter::once(&private_key as _))?;
            std::io::copy(&mut input, &mut output)?;
            output.seek(SeekFrom::Start(0))?;
        }

        Ok(output)
    }

    pub fn get_keys(&self) -> std::result::Result<Vec<Recipient>, AgeEncryptError> {
        if let Some(keys) = &self.repo.config.keys {
            let keys = keys.get(&self.name)
                .or_else(|| keys.get("default"));
            if let Some(keys) = keys {
                let mut result = Vec::with_capacity(keys.len());
                for key in keys {
                    result.push(Recipient::from_str(key)?);
                }
                return Ok(result);
            }
        }

        Ok(Vec::new())
    }

    pub fn encrypt(&self, mut input: &NamedTempFile) -> std::result::Result<(), AgeEncryptError> {
        let env_path = self.path();
        let keys = self.get_keys()?;
        let keys = keys.iter().map(|key| key as _);

        let encryptor = Encryptor::with_recipients(keys)?;
        let output = File::create(env_path)?;
        let output = ArmoredWriter::wrap_output(output, age::armor::Format::AsciiArmor)?;
        let mut output = encryptor.wrap_output(output)?;

        input.seek(SeekFrom::Start(0))?;
        std::io::copy(&mut input, &mut output)?;
        output.finish().and_then(|armor| armor.finish())?;

        Ok(())
    }

    fn calculate_checksum(&self, file: &NamedTempFile) -> Result<String> {
        let output = Command::new("sha256sum")
            .args(["--", file.path().to_str().unwrap()])
            .output()
            .map_err(CommandError::SpawnError)
            .map_err(AppError::ChecksumError)?;

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
            .map_err(CommandError::SpawnError)
            .map_err(AppError::EditorError)?;

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

    pub fn values(&self) -> Result<BTreeMap<String, Value>> {
        let file = self.decrypt()?;
        let content = std::fs::read_to_string(file).unwrap();
        toml::from_str(&content).map_err(AppError::ConfigParseError)
    }

    /// Run a given command with all defined environment variables, replacing the current process
    /// in the with the new one. On success, this method will never return.
    pub fn exec(&self, path: &str, args: Vec<&str>) -> Result<()> {
        let mut command = Command::new(path);
        for arg in &args {
            command.arg(arg);
        }

        for (key, value) in self.values()? {
            let value = match value {
                Value::String(value) => value,
                value => serde_json::to_string(&value).unwrap(),
            };
            command.env(key, value);
        }

        let err = command.exec();

        let mut all_args = vec![path];
        all_args.extend(args);
        let all_args = all_args.into_iter()
            .map(|s| s.into())
            .collect();

        Err(AppError::RunError(all_args, CommandError::SpawnError(err)))
    }
}

fn main() {
    let cli = Cli::parse();

    // You can check for the existence of subcommands, and if found use their
    // matches just as you would the top level cmd.
    if let Err(e) = cli.run() {
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

    pub struct TestSetup {
        _temp_dir: TempDir,
        pub repo: Repo,
    }

    impl TestSetup {
        pub fn new() -> Self {
            let temp_dir = TempDir::new().unwrap();
            let path: PathBuf = temp_dir.path().into();
            Command::new("git")
                .args(["-C", path.to_str().unwrap(), "init"])
                .output()
                .unwrap();

            // Create a .dev directory for secrets
            std::fs::create_dir(path.join(".dev")).unwrap();

            // Write ssh keys to fake home directory
            std::fs::create_dir(path.join(".ssh")).unwrap();
            std::fs::write(path.join(".ssh/id_ed25519.pub"), PUBLIC_KEY.trim()).unwrap();
            std::fs::write(path.join(".ssh/id_ed25519"), PRIVATE_KEY.trim()).unwrap();

            Self {
                _temp_dir: temp_dir,
                repo: Repo {
                    config: Config {
                        commands: None,
                        keys: Some(vec![PUBLIC_KEY.trim().into()]),
                    },
                    home: path.to_str().unwrap().into(),
                    repo_path: path,
                },
            }
        }

        pub fn env(&self) -> Environment {
            self.repo.get_environment("local".into())
        }
    }

    #[test]
    fn test_get_repo_path_success() {
        TestSetup::new();
        Repo::get_repo_path().unwrap();
    }

    #[test]
    fn test_encrypt_decrypt() {
        let setup = TestSetup::new();

        // Encrypt "test content"
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "test content").unwrap();
        setup.env().encrypt(&file).unwrap();

        // Decrypt the encrypted file
        let file = setup.env().decrypt().unwrap();
        let content = fs::read_to_string(file.path()).unwrap();

        // Decrypted content should be the same as the original content
        assert_eq!(content, "test content\n");

        // Encrypted file should not contain the original content
        let content = fs::read_to_string(setup.env().path()).unwrap();
        assert!(!content.contains("test content"));
    }

    #[test]
    fn test_calculate_checksum_success() {
        let setup = TestSetup::new();
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "test content").unwrap();

        let checksum = setup.env().calculate_checksum(&file).unwrap();
        assert_eq!(checksum, "a1fff0ffefb9eace7230c24e50731f0a91c62f9cefdfe77121c2f607125dffae");
    }

    #[test]
    fn test_run_editor_success() {
        let setup = TestSetup::new();
        let file = NamedTempFile::new().unwrap();

        env::set_var("EDITOR", "true");
        setup.env().run_editor(&file).unwrap();
    }

    #[test]
    fn test_run_editor_failure() {
        let setup = TestSetup::new();
        let file = NamedTempFile::new().unwrap();

        env::set_var("EDITOR", "false");
        let result = setup.env().run_editor(&file);

        assert!(result.is_err());
        if let Err(AppError::EditorError(CommandError::FailedError { status, .. })) = result {
            assert!(!status.success());
        } else {
            panic!("Expected EditorError with FailedError");
        }
    }
}
