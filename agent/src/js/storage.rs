use std::collections::BTreeMap;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use boa_engine::{
    Context, IntoJsFunctionCopied, JsData, JsResult, JsValue, interop::ContextData, js_string,
    object::ObjectInitializer, property::Attribute,
};
use boa_gc::{Finalize, Trace};

/// A `sessionStorage`-like key-value store exposed to script probes, allowing
/// them to cache state (such as access tokens) across probe invocations.
///
/// Cloned instances share the same underlying store, while separately
/// constructed instances are fully isolated. Each probe's `ScriptTarget` owns
/// one of these, so state survives across scheduled runs (which clone the
/// target) and lasts until the probe's configuration is rebuilt by a config
/// reload or the process restarts.
#[derive(Clone, Debug, Default, Trace, Finalize, JsData)]
pub(crate) struct SessionStorage {
    #[unsafe_ignore_trace]
    store: Arc<RwLock<BTreeMap<String, String>>>,
}

impl SessionStorage {

    /// Exposes this storage instance as the `sessionStorage` global within the
    /// provided JavaScript context, mirroring the Web Storage API's method
    /// surface (`getItem`, `setItem`, `removeItem`, `clear`, `key`, `length`).
    pub fn register(self, context: &mut Context) -> JsResult<()> {
        context.insert_data(self);

        let get_item_ = get_item.into_js_function_copied(context);
        let set_item_ = set_item.into_js_function_copied(context);
        let remove_item_ = remove_item.into_js_function_copied(context);
        let clear_ = clear.into_js_function_copied(context);
        let key_ = key.into_js_function_copied(context);
        let length_ = length
            .into_js_function_copied(context)
            .to_js_function(context.realm());

        let storage = ObjectInitializer::new(context)
            .function(get_item_, js_string!("getItem"), 1)
            .function(set_item_, js_string!("setItem"), 2)
            .function(remove_item_, js_string!("removeItem"), 1)
            .function(clear_, js_string!("clear"), 0)
            .function(key_, js_string!("key"), 1)
            .accessor(
                js_string!("length"),
                Some(length_),
                None,
                Attribute::ENUMERABLE,
            )
            .build();

        context.register_global_property(
            js_string!("sessionStorage"),
            storage,
            Attribute::READONLY | Attribute::ENUMERABLE,
        )?;

        Ok(())
    }

    fn read(&self) -> RwLockReadGuard<'_, BTreeMap<String, String>> {
        self.store.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, BTreeMap<String, String>> {
        self.store.write().unwrap_or_else(PoisonError::into_inner)
    }
}

fn get_item(
    ContextData(storage): ContextData<SessionStorage>,
    key: JsValue,
    context: &mut Context,
) -> JsResult<JsValue> {
    let key = key.to_string(context)?.to_std_string_lossy();
    Ok(storage
        .read()
        .get(&key)
        .map_or_else(JsValue::null, |value| js_string!(value.as_str()).into()))
}

fn set_item(
    ContextData(storage): ContextData<SessionStorage>,
    key: JsValue,
    value: JsValue,
    context: &mut Context,
) -> JsResult<JsValue> {
    let key = key.to_string(context)?.to_std_string_lossy();
    let value = value.to_string(context)?.to_std_string_lossy();
    storage.write().insert(key, value);
    Ok(JsValue::undefined())
}

fn remove_item(
    ContextData(storage): ContextData<SessionStorage>,
    key: JsValue,
    context: &mut Context,
) -> JsResult<JsValue> {
    let key = key.to_string(context)?.to_std_string_lossy();
    storage.write().remove(&key);
    Ok(JsValue::undefined())
}

fn clear(ContextData(storage): ContextData<SessionStorage>) -> JsResult<JsValue> {
    storage.write().clear();
    Ok(JsValue::undefined())
}

fn key(
    ContextData(storage): ContextData<SessionStorage>,
    index: JsValue,
    context: &mut Context,
) -> JsResult<JsValue> {
    let index = index.to_u32(context)? as usize;
    Ok(storage
        .read()
        .keys()
        .nth(index)
        .map_or_else(JsValue::null, |key| js_string!(key.as_str()).into()))
}

fn length(ContextData(storage): ContextData<SessionStorage>) -> JsResult<JsValue> {
    Ok(JsValue::from(storage.read().len() as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use boa_engine::Source;

    fn context_with_storage(storage: SessionStorage) -> Context {
        let mut context = Context::default();
        storage.register(&mut context).unwrap();
        context
    }

    fn eval(context: &mut Context, code: &str) -> JsValue {
        context.eval(Source::from_bytes(code)).unwrap()
    }

    #[test]
    fn test_set_and_get_item() {
        let storage = SessionStorage::default();
        let mut context = context_with_storage(storage.clone());

        let result = eval(
            &mut context,
            r#"
            sessionStorage.setItem("token", "abc123");
            sessionStorage.getItem("token")
            "#,
        );
        assert_eq!(result, js_string!("abc123").into());
        assert_eq!(
            storage.read().get("token").map(String::as_str),
            Some("abc123")
        );
    }

    #[test]
    fn test_get_missing_item_returns_null() {
        let mut context = context_with_storage(SessionStorage::default());

        let result = eval(&mut context, r#"sessionStorage.getItem("missing")"#);
        assert_eq!(result, JsValue::null());
    }

    #[test]
    fn test_values_are_coerced_to_strings() {
        let mut context = context_with_storage(SessionStorage::default());

        let result = eval(
            &mut context,
            r#"
            sessionStorage.setItem("number", 42);
            typeof sessionStorage.getItem("number") + ":" + sessionStorage.getItem("number")
            "#,
        );
        assert_eq!(result, js_string!("string:42").into());
    }

    #[test]
    fn test_remove_item() {
        let mut context = context_with_storage(SessionStorage::default());

        let result = eval(
            &mut context,
            r#"
            sessionStorage.setItem("a", "1");
            sessionStorage.removeItem("a");
            sessionStorage.getItem("a")
            "#,
        );
        assert_eq!(result, JsValue::null());
    }

    #[test]
    fn test_clear() {
        let storage = SessionStorage::default();
        let mut context = context_with_storage(storage.clone());

        let result = eval(
            &mut context,
            r#"
            sessionStorage.setItem("a", "1");
            sessionStorage.setItem("b", "2");
            sessionStorage.clear();
            sessionStorage.length
            "#,
        );
        assert_eq!(result, JsValue::from(0));
        assert!(storage.read().is_empty());
    }

    #[test]
    fn test_key_and_length() {
        let mut context = context_with_storage(SessionStorage::default());

        let result = eval(
            &mut context,
            r#"
            sessionStorage.setItem("b", "2");
            sessionStorage.setItem("a", "1");
            [sessionStorage.length, sessionStorage.key(0), sessionStorage.key(1), sessionStorage.key(2)].join(",")
            "#,
        );
        assert_eq!(result, js_string!("2,a,b,").into());
    }

    #[test]
    fn test_persists_across_contexts() {
        let storage = SessionStorage::default();

        {
            let mut context = context_with_storage(storage.clone());
            eval(
                &mut context,
                r#"sessionStorage.setItem("token", "cached")"#,
            );
        }

        let mut context = context_with_storage(storage);
        let result = eval(&mut context, r#"sessionStorage.getItem("token")"#);
        assert_eq!(result, js_string!("cached").into());
    }
}
