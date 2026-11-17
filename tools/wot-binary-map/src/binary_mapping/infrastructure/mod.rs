pub mod callbacks;
pub mod pe;
pub mod registrations;
pub mod rtti;
pub mod rust;

use crate::binary_mapping::application::{BinaryInvestigation, Findings};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

pub struct StaticInvestigation;

impl BinaryInvestigation for StaticInvestigation {
    fn inspect(&self, executable: &Path) -> Result<Findings> {
        let bytes =
            std::fs::read(executable).with_context(|| format!("read {}", executable.display()))?;
        let image = pe::Image::parse(&bytes)?;
        let (registration_function, mut interfaces) = registrations::extract(&image)?;
        callbacks::resolve(&image, &mut interfaces);
        let native_types = rtti::extract(&image);
        Ok(Findings {
            sha256: Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            image_base: image.base,
            registration_function,
            interfaces,
            native_types,
        })
    }
}
