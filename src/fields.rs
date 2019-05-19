use std::collections::HashMap;

use serde_json::Value;

/// FieldHolder implements common functions that operate on the `fields` component of a
/// struct (usually Event or Builder). This avoids some duplication of code.
pub trait FieldHolder {
    fn add(&mut self, data: HashMap<String, Value>) {
        self.get_fields().extend(data);
    }

    fn add_field(&mut self, name: &str, value: Value) {
        self.get_fields().insert(name.to_string(), value.clone());
    }

    fn add_func<F>(&mut self, func: F)
    where
        // TODO(nlopes): this shouldn't be std::io::Error
        F: Fn() -> Result<(String, Value), std::io::Error>,
    {
        while let Ok((name, value)) = func() {
            self.add_field(&name, value);
        }
    }

    fn get_fields(&mut self) -> &mut HashMap<String, Value>;
}
