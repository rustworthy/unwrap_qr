use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error("Error occurred: {0}")]
    Common(String),
}
