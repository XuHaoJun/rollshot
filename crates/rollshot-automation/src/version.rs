use serde::{Deserialize, Serialize};

macro_rules! version_type {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        pub struct $name(pub u16);
    };
}

version_type!(LanguageSchemaVersion);
version_type!(IrSchemaVersion);
version_type!(CapabilityApiVersion);
version_type!(OutputSchemaVersion);

pub const LANGUAGE_SCHEMA_V1: LanguageSchemaVersion = LanguageSchemaVersion(1);
pub const IR_SCHEMA_V1: IrSchemaVersion = IrSchemaVersion(1);
pub const CAPABILITY_API_V1: CapabilityApiVersion = CapabilityApiVersion(1);
pub const OUTPUT_SCHEMA_V1: OutputSchemaVersion = OutputSchemaVersion(1);
