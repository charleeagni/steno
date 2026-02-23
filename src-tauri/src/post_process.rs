use crate::runtime::RuntimeError;

pub trait PostProcessor: Send + Sync {
    fn process(&self, text: &str) -> Result<String, RuntimeError>;
}

#[derive(Default)]
pub struct NoopPostProcessor;

impl PostProcessor for NoopPostProcessor {
    fn process(&self, text: &str) -> Result<String, RuntimeError> {
        Ok(text.to_string())
    }
}
