use crate::binary_mapping::domain::{Interface, Investigation, NativeType};
use anyhow::Result;
use std::path::Path;

pub struct Findings {
    pub sha256: String,
    pub image_base: u64,
    pub registration_function: u64,
    pub interfaces: Vec<Interface>,
    pub native_types: Vec<NativeType>,
}

pub trait BinaryInvestigation {
    fn inspect(&self, executable: &Path) -> Result<Findings>;
}

pub fn investigate(executable: &Path, source: &impl BinaryInvestigation) -> Result<Investigation> {
    let findings = source.inspect(executable)?;
    let report = Investigation {
        format_version: 1, executable: executable.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        sha256: findings.sha256, image_base: findings.image_base,
        registration_function: findings.registration_function, interfaces: findings.interfaces, native_types: findings.native_types,
        limitations: vec![
            "Native RTTI describes polymorphic type identity and inheritance, not complete member layouts or object sizes.".into(),
            "Wire structures contain raw payloads; field semantics require analysis of serializers and handlers.".into(),
            "Callback framing remains unresolved unless a bounded read-only evaluator proves its length; virtual table slot counts are executable-pointer runs, not proven class method counts.".into(),
            "Script entities, non-polymorphic types, optimized-away types and dynamically constructed registrations are not completely covered.".into(),
        ],
    };
    Ok(report)
}
