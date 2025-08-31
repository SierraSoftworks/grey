use boa_gc::{Finalize, Trace};
use boa_runtime::Logger;

#[derive(Trace, Finalize, Debug)]
pub struct TraceLogger;

impl Logger for TraceLogger {
    fn log(
        &self,
        msg: String,
        _state: &boa_runtime::ConsoleState,
        _context: &mut boa_engine::Context,
    ) -> boa_engine::JsResult<()> {
        tracing::debug!("{}", msg);
        Ok(())
    }

    fn info(
        &self,
        msg: String,
        _state: &boa_runtime::ConsoleState,
        _context: &mut boa_engine::Context,
    ) -> boa_engine::JsResult<()> {
        tracing::info!("{}", msg);
        Ok(())
    }

    fn warn(
        &self,
        msg: String,
        _state: &boa_runtime::ConsoleState,
        _context: &mut boa_engine::Context,
    ) -> boa_engine::JsResult<()> {
        tracing::warn!("{}", msg);
        Ok(())
    }

    fn error(
        &self,
        msg: String,
        _state: &boa_runtime::ConsoleState,
        _context: &mut boa_engine::Context,
    ) -> boa_engine::JsResult<()> {
        tracing::error!("{}", msg);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boa_engine::{Context, Source};

    #[test]
    fn test_console_logging() {
        let mut context = Context::default();
        boa_runtime::register((boa_runtime::extensions::ConsoleExtension(TraceLogger),), None, &mut context).unwrap();
        
        context
            .eval(Source::from_bytes(
                r#"
            console.log("This is a log message");
            console.info("This is an info message");
            console.warn("This is a warning message");
            console.error("This is an error message");
        "#,
            ))
            .unwrap();
    }
}