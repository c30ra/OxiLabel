//! Attributes: extra named properties a label defines (e.g. "occluded"), whose values each
//! annotation or tag with that label carries.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Label;

/// An attribute a label defines: its name and type, with the value new annotations get.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttributeDefinition {
    pub name: String,
    #[serde(flatten)]
    pub kind: AttributeKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AttributeKind {
    Checkbox { default: bool },
    /// One of `options`.
    Select { options: Vec<String>, default: String },
    /// A number, optionally limited to `min`..=`max`.
    Number { min: Option<f64>, max: Option<f64>, default: f64 },
    Text { default: String },
}

/// An attribute's value: a checkbox's state, a number, or a select's or text's string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttributeValue {
    Bool(bool),
    Number(f64),
    Text(String),
}

/// An annotation's attribute values by name.
pub type Attributes = BTreeMap<String, AttributeValue>;

impl AttributeKind {
    /// Every type, by the name its JSON `type` tag uses.
    pub const TYPES: [&'static str; 4] = ["checkbox", "select", "number", "text"];

    pub fn type_name(&self) -> &'static str {
        match self {
            AttributeKind::Checkbox { .. } => "checkbox",
            AttributeKind::Select { .. } => "select",
            AttributeKind::Number { .. } => "number",
            AttributeKind::Text { .. } => "text",
        }
    }

    /// A new attribute of the type named `type_name` (see `TYPES`), unset.
    pub fn blank(type_name: &str) -> Option<AttributeKind> {
        Some(match type_name {
            "checkbox" => AttributeKind::Checkbox { default: false },
            "select" => AttributeKind::Select { options: Vec::new(), default: String::new() },
            "number" => AttributeKind::Number { min: None, max: None, default: 0.0 },
            "text" => AttributeKind::Text { default: String::new() },
            _ => return None,
        })
    }
}

impl AttributeDefinition {
    pub fn default_value(&self) -> AttributeValue {
        match &self.kind {
            AttributeKind::Checkbox { default } => AttributeValue::Bool(*default),
            AttributeKind::Select { default, .. } | AttributeKind::Text { default } => AttributeValue::Text(default.clone()),
            AttributeKind::Number { default, .. } => AttributeValue::Number(*default),
        }
    }

    /// Whether `value` suits this attribute: the right type, one of a select's options, a
    /// number within its range.
    pub fn check(&self, value: &AttributeValue) -> Result<(), String> {
        let name = &self.name;
        match (&self.kind, value) {
            (AttributeKind::Checkbox { .. }, AttributeValue::Bool(_)) | (AttributeKind::Text { .. }, AttributeValue::Text(_)) => Ok(()),
            (AttributeKind::Select { options, .. }, AttributeValue::Text(text)) if options.contains(text) => Ok(()),
            (AttributeKind::Select { options, .. }, _) => Err(format!("\"{name}\" must be one of {}", options.join(", "))),
            (AttributeKind::Number { min, max, .. }, AttributeValue::Number(number)) => {
                if number.is_finite() && min.is_none_or(|min| *number >= min) && max.is_none_or(|max| *number <= max) {
                    Ok(())
                } else {
                    Err(format!("\"{name}\" is out of range"))
                }
            }
            (AttributeKind::Checkbox { .. }, _) => Err(format!("\"{name}\" must be true or false")),
            (AttributeKind::Number { .. }, _) => Err(format!("\"{name}\" must be a number")),
            (AttributeKind::Text { .. }, _) => Err(format!("\"{name}\" must be text")),
        }
    }

    /// Checks a definition being saved: a name, options for a select, a default that fits.
    fn validated(mut self) -> Result<Self, String> {
        self.name = self.name.trim().to_owned();
        if self.name.is_empty() {
            return Err("attribute names must not be blank".into());
        }
        if let AttributeKind::Select { options, .. } = &mut self.kind {
            options.iter_mut().for_each(|option| *option = option.trim().to_owned());
            options.retain(|option| !option.is_empty());
            if options.is_empty() {
                return Err(format!("\"{}\" needs at least one option", self.name));
            }
        }
        if let AttributeKind::Number { min: Some(min), max: Some(max), .. } = self.kind
            && min > max
        {
            return Err(format!("\"{}\" has its minimum above its maximum", self.name));
        }
        self.check(&self.default_value()).map_err(|reason| format!("the default of {reason}"))?;
        Ok(self)
    }
}

/// Checks a label's attribute definitions; names must be unique.
pub(crate) fn validated(definitions: Vec<AttributeDefinition>) -> Result<Vec<AttributeDefinition>, String> {
    let definitions = definitions.into_iter().map(AttributeDefinition::validated).collect::<Result<Vec<_>, _>>()?;
    for (i, definition) in definitions.iter().enumerate() {
        if definitions[..i].iter().any(|earlier| earlier.name == definition.name) {
            return Err(format!("there are two attributes named \"{}\"", definition.name));
        }
    }
    Ok(definitions)
}

impl Label {
    /// Whether every value is one of this label's attributes and suits it. Missing values count
    /// as the default.
    pub fn check_attributes(&self, values: &Attributes) -> Result<(), String> {
        for (name, value) in values {
            let definition = self.attributes.iter().find(|definition| &definition.name == name);
            let definition = definition.ok_or_else(|| format!("\"{}\" has no attribute \"{name}\"", self.name))?;
            definition.check(value)?;
        }
        Ok(())
    }

    /// `values` made to fit this label: values that suit its attributes are kept, the others
    /// dropped, and missing ones set to their defaults (e.g. after relabelling).
    pub fn conform_attributes(&self, values: &Attributes) -> Attributes {
        self.attributes
            .iter()
            .map(|definition| {
                let value = values.get(&definition.name).filter(|value| definition.check(value).is_ok());
                (definition.name.clone(), value.cloned().unwrap_or_else(|| definition.default_value()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Label, LabelDefinition, ShapeType};
    use serde_json::json;

    fn severity() -> AttributeDefinition {
        AttributeDefinition {
            name: "severity".into(),
            kind: AttributeKind::Select { options: vec!["low".into(), "high".into()], default: "low".into() },
        }
    }

    fn label(attributes: Vec<AttributeDefinition>) -> Label {
        let definition = LabelDefinition {
            name: "crack".into(),
            color: "#e74c3c".into(),
            hotkey: None,
            shape_types: vec![ShapeType::Polygon],
            ignore: false,
            attributes,
            footprint: crate::Footprint::default(),
            show_footprint: true,
            constraints: Default::default(),
        };
        Label::new(1, definition)
    }

    fn values(pairs: serde_json::Value) -> Attributes {
        serde_json::from_value(pairs).unwrap()
    }

    #[test]
    fn definitions_serialize_with_a_type_tag_and_values_as_plain_json() {
        let occluded = AttributeDefinition { name: "occluded".into(), kind: AttributeKind::Checkbox { default: false } };
        assert_eq!(serde_json::to_value(&occluded).unwrap(), json!({"name": "occluded", "type": "checkbox", "default": false}));
        let width = AttributeDefinition { name: "width".into(), kind: AttributeKind::Number { min: Some(0.0), max: None, default: 1.5 } };
        assert_eq!(serde_json::to_value(&width).unwrap(), json!({"name": "width", "type": "number", "min": 0.0, "max": null, "default": 1.5}));
        assert_eq!(values(json!({"a": true, "b": 2.5, "c": "x"})).len(), 3);
    }

    #[test]
    fn definitions_are_checked_when_a_label_is_saved() {
        let check = |attributes| LabelDefinition { attributes, ..label(vec![]).definition() }.validated(&[]).map(|_| ());
        assert_eq!(check(vec![severity()]), Ok(()));
        assert!(check(vec![severity(), severity()]).is_err(), "names are unique");
        let blank = AttributeDefinition { name: " ".into(), kind: AttributeKind::Text { default: String::new() } };
        assert!(check(vec![blank]).is_err());
        let stray = AttributeDefinition { name: "s".into(), kind: AttributeKind::Select { options: vec!["a".into()], default: "b".into() } };
        assert!(check(vec![stray]).is_err(), "the default must be an option");
        let empty = AttributeDefinition { name: "s".into(), kind: AttributeKind::Select { options: vec![], default: String::new() } };
        assert!(check(vec![empty]).is_err(), "a select needs options");
        let outside = AttributeDefinition { name: "n".into(), kind: AttributeKind::Number { min: Some(0.0), max: Some(1.0), default: 2.0 } };
        assert!(check(vec![outside]).is_err(), "the default must be in range");
    }

    #[test]
    fn values_are_checked_against_the_label() {
        let width = AttributeDefinition { name: "width".into(), kind: AttributeKind::Number { min: Some(0.0), max: Some(10.0), default: 1.0 } };
        let label = label(vec![severity(), width]);
        assert_eq!(label.check_attributes(&values(json!({"severity": "high", "width": 3}))), Ok(()));
        assert_eq!(label.check_attributes(&Attributes::new()), Ok(()), "missing values take the default");
        assert!(label.check_attributes(&values(json!({"colour": "red"}))).is_err(), "not defined");
        assert!(label.check_attributes(&values(json!({"severity": "medium"}))).is_err(), "not an option");
        assert!(label.check_attributes(&values(json!({"severity": true}))).is_err(), "wrong type");
        assert!(label.check_attributes(&values(json!({"width": 11}))).is_err(), "out of range");
    }

    #[test]
    fn values_conform_to_a_label_keeping_what_fits_and_filling_defaults() {
        let label = label(vec![severity(), AttributeDefinition { name: "occluded".into(), kind: AttributeKind::Checkbox { default: false } }]);
        let conformed = label.conform_attributes(&values(json!({"severity": "high", "occluded": "yes", "colour": "red"})));
        assert_eq!(conformed, values(json!({"severity": "high", "occluded": false})));
    }

    #[test]
    fn type_names_are_the_json_tags() {
        for name in AttributeKind::TYPES {
            let kind = AttributeKind::blank(name).unwrap();
            assert_eq!((kind.type_name(), serde_json::to_value(&kind).unwrap()["type"].as_str()), (name, Some(name)));
        }
        assert_eq!(AttributeKind::blank("colour"), None);
    }
}
