use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Ok,
    Usage,
    Connection,
    Rpc,
    Timeout,
}

impl ExitCode {
    pub fn as_i32(self) -> i32 {
        match self {
            Self::Ok => 0,
            Self::Usage => 1,
            Self::Connection => 2,
            Self::Rpc => 3,
            Self::Timeout => 4,
        }
    }
}

#[derive(Debug)]
pub enum CliError {
    Usage(String),
    Connection(String),
    Rpc(String),
    Timeout(String),
}

impl CliError {
    pub fn connection(error: impl ToString) -> Self {
        Self::Connection(error.to_string())
    }

    pub fn rpc(error: impl ToString) -> Self {
        Self::Rpc(error.to_string())
    }

    pub fn from_request(error: anyhow::Error) -> Self {
        let message = format!("{error:#}");
        if message.contains("timed out") {
            Self::Timeout(message)
        } else if message.contains("connect ") {
            Self::Connection(message)
        } else {
            Self::Rpc(message)
        }
    }

    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::Usage(_) => ExitCode::Usage,
            Self::Connection(_) => ExitCode::Connection,
            Self::Rpc(_) => ExitCode::Rpc,
            Self::Timeout(_) => ExitCode::Timeout,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message)
            | Self::Connection(message)
            | Self::Rpc(message)
            | Self::Timeout(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for CliError {}
