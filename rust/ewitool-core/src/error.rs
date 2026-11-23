use std::fmt;
use std::io;

#[derive(Debug)]
pub enum Error {
    /// A patch or patch file had the wrong number of bytes.
    WrongLength {
        expected: usize,
        actual: usize,
    },
    /// A hex string was not valid hex.
    BadHex,
    /// A patch name was rejected; the message says why.
    InvalidName(String),
    /// A legacy import file's extension is not .bnk or .sqs.
    UnknownLegacyType,
    /// A legacy import file had no BODY marker.
    BodyNotFound,
    /// A legacy import file's first patch was missing or malformed.
    FirstPatchMissing,
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::WrongLength { expected, actual } => {
                write!(
                    f,
                    "wrong length: {actual} bytes (patches are {expected} bytes each)"
                )
            }
            Error::BadHex => write!(f, "invalid hex string"),
            Error::InvalidName(why) => write!(f, "invalid patch name: {why}"),
            Error::UnknownLegacyType => write!(f, "legacy files must end in .bnk or .sqs"),
            Error::BodyNotFound => write!(f, "could not find BODY in soundbank file"),
            Error::FirstPatchMissing => {
                write!(f, "could not find first patch in soundbank file")
            }
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}
