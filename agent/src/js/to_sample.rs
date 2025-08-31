use boa_engine::{Context, JsError, JsObject, JsResult, JsValue, js_string};

use crate::{Sample, SampleValue};

pub trait JsInto<T> {
    fn js_into(&self, context: &mut Context) -> JsResult<T>;
}

impl JsInto<Sample> for JsObject {
    fn js_into(&self, context: &mut Context) -> JsResult<Sample> {
        let mut sample: Sample = Sample::default();

        let keys = self.own_property_keys(context)?;
        for key in keys {
            let value = self.get(key.clone(), context)?;
            sample.set(key.to_string(), value.js_into(context)?);
        }

        Ok(sample)
    }
}

impl JsInto<SampleValue> for JsValue {
    fn js_into(&self, context: &mut Context) -> JsResult<SampleValue> {
        if let Some(string) = self.as_string() {
            Ok(SampleValue::String(string.to_std_string_lossy()))
        } else if let Some(boolean) = self.as_boolean() {
            Ok(SampleValue::Bool(boolean))
        } else if let Some(number) = self.as_i32() {
            Ok(SampleValue::Int(number as i64))
        } else if let Some(number) = self.as_number() {
            Ok(SampleValue::Double(number))
        } else if let Some(object) = self.as_object() {
            object.js_into(context)
        } else {
            Ok(SampleValue::None)
        }
    }
}

impl JsInto<SampleValue> for JsObject {
    fn js_into(&self, context: &mut Context) -> JsResult<SampleValue> {
        if self.is_array() {
            let len = self.get(js_string!("length"), context)?.to_i32(context)?;
            let mut array = Vec::with_capacity(len as usize);

            for i in 0..len {
                let element = self.get(i, context)?;
                array.push(element.js_into(context)?);
            }

            Ok(SampleValue::List(array))
        } else {
            let json = context.global_object().get(js_string!("JSON"), context)?;

            let stringify = json
                .as_object()
                .ok_or(JsError::from_rust(std::io::Error::other(
                    "JSON is not defined in the global scope",
                )))?
                .get(js_string!("stringify"), context)?
                .as_function()
                .ok_or(JsError::from_rust(std::io::Error::other(
                    "JSON.stringify is not defined in the global scope",
                )))?;

            let value = stringify
                .call(&json, &[JsValue::from(self.clone())], context)?
                .to_string(context)?
                .to_std_string_lossy();
            Ok(SampleValue::String(value))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boa_engine::{Context, Source};

    #[test]
    fn test_js_primitives_into_sample_value() {
        assert_eq!(get_sample_value("null"), SampleValue::None);
        assert_eq!(get_sample_value("true"), SampleValue::Bool(true));
        assert_eq!(get_sample_value("false"), SampleValue::Bool(false));
        assert_eq!(get_sample_value("42"), SampleValue::Int(42));
        assert_eq!(get_sample_value("3.14"), SampleValue::Double(3.14));
        assert_eq!(get_sample_value(r#""hello""#), SampleValue::String("hello".into()));
    }

    #[test]
    fn test_js_array_into_sample_value() {
        assert_eq!(
            get_sample_value("[1, 2, 3]"),
            SampleValue::List(vec![
                SampleValue::Int(1),
                SampleValue::Int(2),
                SampleValue::Int(3)
            ])
        );
        assert_eq!(
            get_sample_value(r#"["a", "b", "c"]"#),
            SampleValue::List(vec![
                SampleValue::String("a".into()),
                SampleValue::String("b".into()),
                SampleValue::String("c".into())
            ])
        );
    }

    #[test]
    fn test_js_object_into_sample() {
        let sample = get_sample_value(r#"({ "key1": "value1", "key2": 42, "key3": true })"#);
        assert_eq!(sample, SampleValue::String(r#"{"key1":"value1","key2":42,"key3":true}"#.into()));
    }

    fn get_sample_value(script: &str) -> SampleValue {
        let mut context = Context::default();
        let js_value = context.eval(Source::from_bytes(script)).unwrap();

        js_value.js_into(&mut context).unwrap()
    }
}