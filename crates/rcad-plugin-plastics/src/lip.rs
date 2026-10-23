//! (being written)
use rcad_plugin::schema::FeatureSchema;
use rcad_plugin::{BuildRequest, Op, PluginError};

pub fn schema() -> FeatureSchema {
    FeatureSchema {
        id: "lip".into(),
        name: "lip".into(),
        description: String::new(),
        inputs: vec![],
        params: vec![],
        choices: vec![],
    }
}

pub fn build(req: &BuildRequest) -> Result<Vec<Op>, PluginError> {
    Err(PluginError::Unsupported(format!(
        "{}: not implemented yet",
        req.feature
    )))
}
