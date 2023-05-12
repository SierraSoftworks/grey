use std::rc::Rc;

use deno_core::{ModuleLoader, ResolutionKind, resolve_url, ModuleType};

static MEMORY_SCRIPT_SPECIFIER: &str = "memory:main_module.js";

pub struct MemoryModuleLoader {
    code: String,
    loader: Option<Rc<dyn ModuleLoader>>
}

impl MemoryModuleLoader {
    pub fn new<S: Into<String>>(code: S) -> Self {
        Self {
            code: code.into(),
            loader: None
        }
    }

    #[allow(unused)]
    pub fn with_fallback(self, loader: Rc<dyn ModuleLoader>) -> Self {
        Self {
            code: self.code,
            loader: Some(loader)
        }
    }
}

impl ModuleLoader for MemoryModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        kind: deno_core::ResolutionKind,
    ) -> Result<deno_core::ModuleSpecifier, deno_core::anyhow::Error> {
        match kind {
            ResolutionKind::MainModule => {
                Ok(resolve_url(MEMORY_SCRIPT_SPECIFIER)?)
            },
            _ => match self.loader.clone() {
                Some(loader) => loader.resolve(specifier, referrer, kind),
                _ => deno_core::anyhow::bail!("importing foreign modules is not supported in probe scripts")
            }
        }
    }

    fn load(
        &self,
        module_specifier: &deno_core::ModuleSpecifier,
        maybe_referrer: Option<&deno_core::ModuleSpecifier>,
        is_dyn_import: bool,
    ) -> std::pin::Pin<Box<deno_core::ModuleSourceFuture>> {
        if module_specifier.as_str() == MEMORY_SCRIPT_SPECIFIER {
            let module_specifier = module_specifier.to_owned();
            let code = format!("{}", self.code).into();
            Box::pin(async move {
                if is_dyn_import {
                    deno_core::anyhow::bail!("importing the main module again is not supported")
                } else {
                    Ok(deno_core::ModuleSource::new(
                        ModuleType::JavaScript,
                        code,
                        &module_specifier,
                    ))
                }
            })
        } else if let Some(loader) = self.loader.clone() {
            loader.load(module_specifier, maybe_referrer, is_dyn_import)
        } else {
            Box::pin(async {
                deno_core::anyhow::bail!("loading external modules is not supported in this environment")
            })
        }
    }
}