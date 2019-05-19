use std::collections::HashMap;

use serde_json::Value;

/// FieldHolder implements common functions that operate on the `fields` component of a
/// struct (usually Event or Builder). This avoids some duplication of code.
pub trait FieldHolder {
    /// add data to the current (event/builder) fields
    fn add(&mut self, data: HashMap<String, Value>) {
        self.get_fields().extend(data);
    }

    /// add_field adds a field to the current (event/builder) fields
    fn add_field(&mut self, name: &str, value: Value) {
        self.get_fields().insert(name.to_string(), value.clone());
    }

    /// add_func iterates over the results from func (until Err) and adds the results to
    /// the event/builder fields
    fn add_func<F>(&mut self, func: F)
    where
        // TODO(nlopes): this shouldn't be std::io::Error
        F: Fn() -> Result<(String, Value), std::io::Error>,
    {
        while let Ok((name, value)) = func() {
            self.add_field(&name, value);
        }
    }

    /// get_fields provides an interface to the event/builder fields
    fn get_fields(&mut self) -> &mut HashMap<String, Value>;
}
