use rollshot_automation::{
    CapabilityApiVersion, IrSchemaVersion, LanguageSchemaVersion, OutputSchemaVersion,
    CAPABILITY_API_V1, IR_SCHEMA_V1, LANGUAGE_SCHEMA_V1, OUTPUT_SCHEMA_V1,
};

#[test]
fn installed_schema_versions_are_explicit_and_round_trip() {
    assert_eq!(LANGUAGE_SCHEMA_V1, LanguageSchemaVersion(1));
    assert_eq!(IR_SCHEMA_V1, IrSchemaVersion(1));
    assert_eq!(CAPABILITY_API_V1, CapabilityApiVersion(1));
    assert_eq!(OUTPUT_SCHEMA_V1, OutputSchemaVersion(1));

    let json = serde_json::to_string(&(
        LANGUAGE_SCHEMA_V1,
        IR_SCHEMA_V1,
        CAPABILITY_API_V1,
        OUTPUT_SCHEMA_V1,
    ))
    .unwrap();
    let decoded: (
        LanguageSchemaVersion,
        IrSchemaVersion,
        CapabilityApiVersion,
        OutputSchemaVersion,
    ) = serde_json::from_str(&json).unwrap();
    assert_eq!(
        decoded,
        (
            LANGUAGE_SCHEMA_V1,
            IR_SCHEMA_V1,
            CAPABILITY_API_V1,
            OUTPUT_SCHEMA_V1,
        )
    );
}
