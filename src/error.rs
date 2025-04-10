use std::{fmt, io};


#[derive(Debug)]
pub enum AppError {
    /// Failed to run a git command.
    GitError(CommandError),
    /// Failed to decrypt the config file.
    AgeDecryptError(AgeDecryptError),
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
    /// Value was missing from config file.
    ConfigMissing(String),
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
            AppError::ConfigMissing(setting) => write!(f, "Missing required config value '{}'", setting),
        }
    }
}

#[derive(Debug)]
pub enum AgeDecryptError {
    Io(std::io::Error),
    Decrypt(age::DecryptError),
}

impl fmt::Display for AgeDecryptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "Failed to decrypt environment variables: {}", e),
            Self::Decrypt(e) => write!(f, "Failed to decrypt environment variables: {}", e),
        }
    }
}

impl From<std::io::Error> for AgeDecryptError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<age::DecryptError> for AgeDecryptError {
    fn from(err: age::DecryptError) -> Self {
        Self::Decrypt(err)
    }
}

#[derive(Debug)]
pub enum CommandError {
    /// The command failed to spawn.
    SpawnError(io::Error),
    /// The command failed with an error message.
    FailedError {
        status: std::process::ExitStatus,
        stderr: Option<String>,
    },
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

pub type Result<T> = std::result::Result<T, AppError>;
