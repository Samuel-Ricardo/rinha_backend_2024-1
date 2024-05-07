use std::{error, fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Serialization(Box<dyn error::Error + Send + Sync>),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Serialization(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<bitcode::Error> for Error {
    fn from(err: bitcode::Error) -> Self {
        Self::Serialization(Box::new(err))
    }
}
