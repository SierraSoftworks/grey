use std::{
    fmt::Display, sync::Arc, cell::RefCell,
};

use opentelemetry::trace::SpanKind;
use serde::{Deserialize, Serialize};

use crate::{Sample, deno};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScriptTarget {
    pub code: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl ScriptTarget {
    #[instrument(
        "target.script",
        skip(self), err(Raw), fields(
        otel.kind=?SpanKind::Client,
    ))]
    pub async fn run(&self) -> Result<Sample, Box<dyn std::error::Error>> {
        let sample = Arc::new(RefCell::new(Sample::default()));

        let mut worker = deno::Worker::new_for_code(&self.code, deno::WorkerContext {
            args: self.args.clone(),
            output: sample.clone(),
        })?;

        let exit_code = worker.run().await?;

        let sample = sample.take().with("script.exit_code", exit_code);

        Ok(sample)
    }
}

impl Display for ScriptTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "script.js")
    }
}


#[cfg(test)]
mod tests {
    use crate::sample::SampleValue;

    use super::*;

    #[tokio::test]
    async fn test_script_ok() {
        let target = ScriptTarget {
            code: "console.log('Hello, world!');".to_string(),
            ..Default::default()
        };

        target.run().await.expect("no error to be raised");
    }

    #[tokio::test]
    async fn test_script_invalid_syntax() {
        let target = ScriptTarget {
            code: "console.log('Hello, world!".to_string(),
            ..Default::default()
        };

        target.run().await.expect_err("an error should be raised");
    }

    #[tokio::test]
    async fn test_script_with_outputs() {
        let target = ScriptTarget {
            code: r#"setOutput('abc', '123'); setOutput('x', 'y')"#.into(),
            ..Default::default()
        };

        let sample = target.run().await.expect("no error to be raised");
        assert_eq!(sample.get("abc"), &SampleValue::from("123"));
    }

    #[tokio::test]
    async fn test_script_fetch() {
        let target = ScriptTarget {
            code: r#"
            const result = await fetch("https://bender.sierrasoftworks.com/api/v1/quote", {
                headers: getTraceHeaders()
            });
            setOutput('http.status_code', result.status);
            "#.into(),
            ..Default::default()
        };

        let sample = target.run().await.expect("no error to be raised");
        assert_eq!(sample.get("http.status_code"), &SampleValue::from(200));
    }
}
