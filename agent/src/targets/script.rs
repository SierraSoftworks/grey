use boa_engine::{Module, Source, builtins::promise::PromiseState, job::JobExecutor};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, fmt::Display, rc::Rc, sync::atomic::AtomicBool};
use tracing::instrument;
use tracing_batteries::prelude::*;

use crate::{Sample, js::JobQueue, targets::Target};
use crate::js::{JsInto, SessionStorage};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScriptTarget {
    pub code: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Runtime cache backing the script's `sessionStorage` global. Clones share the
    /// same store, so state persists across the per-run clones taken by the probe
    /// runner and lasts until a config reload rebuilds this target.
    #[serde(skip)]
    session: SessionStorage,
}

/// Equality covers only the configured fields — the session store is runtime cache,
/// not configuration. The config reloader relies on this comparison to decide when a
/// probe has changed and needs its target (and therefore its cache) rebuilt.
impl PartialEq for ScriptTarget {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code && self.args == other.args
    }
}

impl Target for ScriptTarget {
    #[instrument("target.script", skip(self, _cancel), err(Debug), fields(script.exit_code = EmptyField))]
    async fn run(&self, _cancel: &AtomicBool) -> Result<Sample, Box<dyn std::error::Error>> {
        let code = self.code.clone();
        let args = self.args.clone();

        let executor = Rc::new(JobQueue::new());
        let context = &mut boa_engine::Context::builder()
            .job_executor(executor.clone())
            .build()?;

        crate::js::setup_runtime(context, self.session.clone(), args)?;

        let module = Module::parse(Source::from_bytes(&code), None, context)?;

        let promise = module.load_link_evaluate(context);

        executor.run_jobs_async(&RefCell::new(context)).await?;

        let output = context.eval(Source::from_bytes("output"))?;

        let mut sample = if let Some(output) = output.as_object() {
            output.js_into(context)?
        } else {
            Sample::default()
        };

        match promise.state() {
            PromiseState::Fulfilled(_) => {
                sample.set("script.exit_code", 0);
            }
            PromiseState::Rejected(err) => {
                return Err(err.to_string(context)?.to_std_string_lossy().into());
            }
            PromiseState::Pending => {
                return Err("Script awaits unresolvable promises.".into());
            }
        }

        Ok(sample)
    }
}

impl Display for ScriptTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "probe.script({})",
            self.args
                .iter()
                .map(|a| serde_json::to_string(a).unwrap_or_else(|_| format!("\"{a}\"")))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::sample::SampleValue;

    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_script_ok() {
        let target = ScriptTarget {
            code: "console.log('Hello, world!');".to_string(),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);

        target.run(&cancel).await.expect("no error to be raised");
    }

    #[tokio::test]
    async fn test_script_invalid_syntax() {
        let target = ScriptTarget {
            code: "console.log('Hello, world!".to_string(),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);

        target
            .run(&cancel)
            .await
            .expect_err("an error should be raised");
    }

    #[tokio::test]
    async fn test_script_with_outputs() {
        let target = ScriptTarget {
            code: r#"output['abc'] = '123'; output['x'] = 'y';"#.into(),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);

        let sample = target.run(&cancel).await.expect("no error to be raised");
        assert_eq!(sample.get("abc"), &SampleValue::from("123"));
    }

    #[tokio::test]
    async fn test_script_args() {
        let target = ScriptTarget {
            code: r#"output.a = arguments[0]; output.b = arguments[1]"#.into(),
            args: vec!["arg1".into(), "arg2".into()],
            ..Default::default()
        };

        let cancel = AtomicBool::new(false);
        let sample = target.run(&cancel).await.expect("no error to be raised");
        assert_eq!(sample.get("a"), &SampleValue::from("arg1"));
        assert_eq!(sample.get("b"), &SampleValue::from("arg2"));
    }

    #[tokio::test]
    async fn test_script_set_timeout() {
        let target = ScriptTarget {
            code: r#"await new Promise((resolve) => setTimeout(resolve, 100));"#.into(),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);

        let result = target.run(&cancel).await.unwrap();
        assert_eq!(result.get("script.exit_code"), &SampleValue::from(0));
    }

    #[tokio::test]
    async fn test_script_fetch() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"value": "test"})),
            )
            .mount(&mock_server)
            .await;

        let target = ScriptTarget {
            code: format!(
                r#"
            const result = await fetch("{}/test", {{ headers: getTraceHeaders() }});
            output['status'] = result.status;
            output['value'] = (await result.json()).value;
            "#,
                mock_server.uri()
            ),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);

        let sample = target.run(&cancel).await.expect("no error to be raised");
        assert_eq!(sample.get("status"), &SampleValue::from(200));
        assert_eq!(sample.get("value"), &SampleValue::from("test"));
    }

    #[tokio::test]
    async fn test_script_ignores_invalid_blockers() {
        // validates that tokio's timeout wrapper causes a script to halt execution
        let target = ScriptTarget {
            code: r#"await new Promise((resolve) => { /* never resolves */ });"#.into(),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);

        target
            .run(&cancel)
            .await
            .expect_err("Should raise an error explaining that the script cannot complete");
    }

    #[tokio::test]
    async fn test_script_timeout() {
        // validates that tokio's timeout wrapper causes a script to halt execution
        let target = ScriptTarget {
            code: r#"await new Promise((resolve) => { setTimeout(resolve, 1000); });"#.into(),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);

        match tokio::time::timeout(Duration::from_millis(100), target.run(&cancel)).await {
            Ok(result) => panic!("Expected timeout, but got {:?}", result),
            Err(_) => {}
        }
    }

    #[tokio::test]
    async fn test_script_session_storage() {
        let target = ScriptTarget {
            code: r#"
            const runs = parseInt(sessionStorage.getItem("runs") ?? "0", 10) + 1;
            sessionStorage.setItem("runs", runs);
            output.runs = sessionStorage.getItem("runs");
            "#
            .into(),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);

        // state persists across invocations, including through the per-run clones
        // taken by the probe runner
        let sample = target.run(&cancel).await.expect("no error to be raised");
        assert_eq!(sample.get("runs"), &SampleValue::from("1"));

        let sample = target
            .clone()
            .run(&cancel)
            .await
            .expect("no error to be raised");
        assert_eq!(sample.get("runs"), &SampleValue::from("2"));

        // ...but a separately constructed target (e.g. another probe using the same
        // script, or this probe after a config reload) starts from an empty store
        let fresh = ScriptTarget {
            code: target.code.clone(),
            ..Default::default()
        };
        let sample = fresh.run(&cancel).await.expect("no error to be raised");
        assert_eq!(sample.get("runs"), &SampleValue::from("1"));
    }

    #[tokio::test]
    async fn test_script_target_equality_ignores_session_state() {
        let populated = ScriptTarget {
            code: "sessionStorage.setItem('k', 'v')".into(),
            ..Default::default()
        };
        let empty = ScriptTarget {
            code: populated.code.clone(),
            ..Default::default()
        };

        let cancel = AtomicBool::new(false);
        populated.run(&cancel).await.expect("no error to be raised");

        // the config reloader compares probes to decide what to rebuild; cached
        // session state must not make otherwise-identical configs look different
        assert_eq!(populated, empty);
        assert_ne!(
            populated,
            ScriptTarget {
                code: "other".into(),
                ..Default::default()
            }
        );
        assert_ne!(
            populated,
            ScriptTarget {
                code: populated.code.clone(),
                args: vec!["arg".into()],
                ..Default::default()
            }
        );
    }

    #[tokio::test]
    async fn test_script_json() {
        let target = ScriptTarget {
            code: r#"
            output.json = JSON.stringify({ "x": 1 })
            "#
            .into(),
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);

        let sample = target.run(&cancel).await.expect("no error to be raised");
        assert_eq!(sample.get("json"), &SampleValue::from(r#"{"x":1}"#));
    }
}
