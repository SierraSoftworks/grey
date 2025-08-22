use std::sync::Arc;

use deno_core::{
    error::ModuleLoaderError, resolve_url, ModuleLoader, ModuleResolutionError, ModuleType,
    ResolutionKind,
};

pub static MEMORY_SCRIPT_SPECIFIER: &str = "memory:probe.js";

pub struct MemoryModuleLoader {
    code: Arc<String>,
}

impl MemoryModuleLoader {
    pub fn new<S: Into<String>>(code: S) -> Self {
        Self {
            code: Arc::new(code.into()),
        }
    }
}

impl ModuleLoader for MemoryModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        kind: ResolutionKind,
    ) -> Result<deno_core::ModuleSpecifier, deno_core::error::ModuleLoaderError> {
        match kind {
            ResolutionKind::MainModule => {
                match resolve_url(MEMORY_SCRIPT_SPECIFIER) {
                    Ok(specifier) => Ok(specifier),
                    Err(ModuleResolutionError::InvalidUrl(e)) => Err(ModuleLoaderError::new("InvalidUrl", format!("{e}"))),
                    Err(ModuleResolutionError::InvalidBaseUrl(e)) => Err(ModuleLoaderError::new("InvalidBaseUrl", format!("{e}"))),
                    Err(ModuleResolutionError::ImportPrefixMissing { .. }) => Err(ModuleLoaderError::new("ImportPrefixMissing", format!("You have not provided a valid prefix for your module import (got specifier = {specifier}, referrer = {referrer}).")))
                }
            },
            _ => Err(ModuleLoaderError::new("ForeignModuleImport", "importing foreign modules is not supported in probe scripts"))
        }
    }

    fn load(
        &self,
        module_specifier: &deno_core::ModuleSpecifier,
        _maybe_referrer: Option<&deno_core::ModuleSpecifier>,
        _is_dyn_import: bool,
        requested_module_type: deno_core::RequestedModuleType,
    ) -> deno_core::ModuleLoadResponse {
        if matches!(requested_module_type, deno_core::RequestedModuleType::None)
            && module_specifier.as_str() == MEMORY_SCRIPT_SPECIFIER
        {
            deno_core::ModuleLoadResponse::Sync(Ok(deno_core::ModuleSource::new(
                ModuleType::JavaScript,
                deno_core::ModuleSourceCode::String(self.code.as_str().to_string().into()),
                module_specifier,
                None,
            )))
        } else {
            deno_core::ModuleLoadResponse::Sync(Err(ModuleLoaderError::new(
                "ForeignModuleImport",
                "importing foreign modules is not supported in probe scripts",
            )))
        }
    }
}

use ::deno_error::js_error_wrapper;
js_error_wrapper!(
    deno_core::ModuleResolutionError,
    JsModuleResolutionError,
    "ModuleResolutionError"
);
