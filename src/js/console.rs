use boa_gc::{Finalize, Trace};
use boa_runtime::Logger;

#[derive(Trace, Finalize, Debug)]
pub struct TraceLogger;

impl Logger for TraceLogger {
    fn log(&self, msg: String, _state: &boa_runtime::ConsoleState, _context: &mut boa_engine::Context) -> boa_engine::JsResult<()> {
        tracing::debug!("{}", msg);
        Ok(())
    }

    fn info(&self, msg: String, _state: &boa_runtime::ConsoleState, _context: &mut boa_engine::Context) -> boa_engine::JsResult<()> {
        tracing::info!("{}", msg);
        Ok(())
    }

    fn warn(&self, msg: String, _state: &boa_runtime::ConsoleState, _context: &mut boa_engine::Context) -> boa_engine::JsResult<()> {
        tracing::warn!("{}", msg);
        Ok(())
    }

    fn error(&self, msg: String, _state: &boa_runtime::ConsoleState, _context: &mut boa_engine::Context) -> boa_engine::JsResult<()> {
        tracing::error!("{}", msg);
        Ok(())
    }
}