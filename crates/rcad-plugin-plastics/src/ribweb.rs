//! (being written)
use rcad_plugin::schema::FeatureSchema;
use rcad_plugin::{BuildRequest, Op, PluginError};

fn stub(id: &str) -> FeatureSchema {
    FeatureSchema {
        id: id.into(),
        name: id.into(),
        description: String::new(),
        inputs: vec![],
        params: vec![],
        choices: vec![],
    }
}
pub fn rib_schema() -> FeatureSchema {
    stub("rib")
}
pub fn web_schema() -> FeatureSchema {
    stub("web")
}
pub fn build_rib(req: &BuildRequest) -> Result<Vec<Op>, PluginError> {
    Err(PluginError::Unsupported(format!(
        "{}: not implemented yet",
        req.feature
    )))
}
pub fn build_web(req: &BuildRequest) -> Result<Vec<Op>, PluginError> {
    build_rib(req)
}
