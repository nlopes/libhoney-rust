use std::collections::HashMap;

use crate::errors::Result;
use crate::Value;

/// `FieldHolder` implements common functions that operate on the `fields` component of a
/// struct (usually Event or Builder). This avoids some duplication of code.
pub trait FieldHolder {
    /// add data to the current (event/builder) fields
    fn add(&mut self, data: HashMap<String, Value>);
    /// add_field adds a field to the current (event/builder) fields
    fn add_field(&mut self, name: &str, value: Value);
    /// add_func iterates over the results from func (until Err) and adds the results to
    /// the event/builder fields
    fn add_func<F>(&mut self, func: F)
    where
        F: Fn() -> Result<(String, Value)>;
}
