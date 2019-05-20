use std::collections::HashMap;

use serde_json::Value;

use crate::event::Event;
use crate::fields::FieldHolder;
use crate::ClientOptions;

/// Shorthand type for the function to be passed to the add_dynamic_field calls
pub type DynamicFieldFunc = fn() -> Value;

impl FieldHolder for Builder {
    fn get_fields(&mut self) -> &mut HashMap<String, Value> {
        &mut self.fields
    }
}

/// Builder is used to create templates for new events, specifying default fields and
/// override settings.
#[derive(Debug, Clone)]
pub struct Builder {
    options: ClientOptions,
    pub(crate) fields: HashMap<String, Value>,
    dynamic_fields: Vec<(String, DynamicFieldFunc)>,
}

impl Builder {
    /// Creates a new event Builder with emtpy Static or Dynamic fields.
    pub fn new(options: ClientOptions) -> Self {
        Builder {
            options,
            fields: HashMap::new(),
            dynamic_fields: Vec::new(),
        }
    }

    /// add_dynamic_field adds a dynamic field to the builder. Any events created from
    /// this builder will get this metric added.
    pub fn add_dynamic_field(&mut self, name: &str, func: DynamicFieldFunc) {
        self.dynamic_fields.push((name.to_string(), func));
    }

    /// new_event creates a new Event prepopulated with fields, dynamic field values, and
    /// configuration inherited from the builder.
    pub fn new_event<T: Clone + Send>(&self) -> Event<T> {
        let mut e = Event::new(&self.options);
        e.fields = self.fields.clone();
        for (name, func) in &self.dynamic_fields {
            e.add_field(&name, func())
        }
        e
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_add() {
        let mut builder = Builder::new(Default::default());
        let mut d: HashMap<String, Value> = HashMap::new();
        d.insert("key".to_string(), Value::String("value".to_string()));
        builder.add(d);

        assert!(builder.fields.contains_key("key"));
        assert_eq!(builder.fields["key"], Value::String("value".to_string()));
    }

    #[test]
    fn test_builder_add_conflict() {
        let mut builder = Builder::new(Default::default());
        let mut data1: HashMap<String, Value> = HashMap::new();
        data1.insert("key".to_string(), Value::String("value".to_string()));
        builder.add(data1);
        let mut data2: HashMap<String, Value> = HashMap::new();
        data2.insert("key".to_string(), serde_json::json!(["1", "2"]));
        builder.add(data2);

        assert_eq!(builder.fields["key"], serde_json::json!(["1", "2"]));
    }
}
