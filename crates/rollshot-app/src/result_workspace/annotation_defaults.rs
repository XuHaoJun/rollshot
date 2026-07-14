use rollshot_image_document::{
    NumberSize, NumberStyle, Rgb8, ShapeKind, StrokeStyle, TextSize, TextStyle,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShapeDefaults {
    pub stroke: StrokeStyle,
    pub fill_enabled: bool,
    pub fill_color: Rgb8,
}

impl Default for ShapeDefaults {
    fn default() -> Self {
        Self {
            stroke: StrokeStyle::default(),
            fill_enabled: false,
            fill_color: Rgb8::new(0xE5, 0x48, 0x4D),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnnotationDefaults {
    pub number: NumberStyle,
    pub text: TextStyle,
    pub line: StrokeStyle,
    pub arrow: StrokeStyle,
    pub rectangle: ShapeDefaults,
    pub ellipse: ShapeDefaults,
    pub last_shape: ShapeKind,
}

impl Default for AnnotationDefaults {
    fn default() -> Self {
        Self {
            number: NumberStyle::default(),
            text: TextStyle::default(),
            line: StrokeStyle::default(),
            arrow: StrokeStyle::default(),
            rectangle: ShapeDefaults::default(),
            ellipse: ShapeDefaults::default(),
            last_shape: ShapeKind::Rectangle,
        }
    }
}

impl AnnotationDefaults {
    pub fn shape(&self, kind: ShapeKind) -> &ShapeDefaults {
        match kind {
            ShapeKind::Rectangle => &self.rectangle,
            ShapeKind::Ellipse => &self.ellipse,
        }
    }

    pub fn shape_mut(&mut self, kind: ShapeKind) -> &mut ShapeDefaults {
        match kind {
            ShapeKind::Rectangle => &mut self.rectangle,
            ShapeKind::Ellipse => &mut self.ellipse,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedAnnotationDefaults {
    pub values: AnnotationDefaults,
    pub warnings: Vec<String>,
}

pub fn load_from(path: &Path) -> LoadedAnnotationDefaults {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return LoadedAnnotationDefaults {
                values: AnnotationDefaults::default(),
                warnings: vec![],
            };
        }
        Err(_) => {
            return LoadedAnnotationDefaults {
                values: AnnotationDefaults::default(),
                warnings: vec!["failed to read config file".into()],
            };
        }
    };

    let table: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(_) => {
            return LoadedAnnotationDefaults {
                values: AnnotationDefaults::default(),
                warnings: vec!["malformed config.toml — using defaults".into()],
            };
        }
    };

    let section = match table.get("annotation_defaults") {
        Some(toml::Value::Table(t)) => t.clone(),
        Some(_) => {
            return LoadedAnnotationDefaults {
                values: AnnotationDefaults::default(),
                warnings: vec!["annotation_defaults is not a table — using defaults".into()],
            };
        }
        None => {
            return LoadedAnnotationDefaults {
                values: AnnotationDefaults::default(),
                warnings: vec![],
            };
        }
    };

    let mut warnings = Vec::new();
    let number = load_number_style(&section, &mut warnings);
    let text = load_text_style(&section, &mut warnings);
    let line = load_stroke_style(&section, "line", &mut warnings);
    let arrow = load_stroke_style(&section, "arrow", &mut warnings);
    let rectangle = load_shape_defaults(&section, "rectangle", &mut warnings);
    let ellipse = load_shape_defaults(&section, "ellipse", &mut warnings);
    let last_shape = load_last_shape(&section, &mut warnings);

    LoadedAnnotationDefaults {
        values: AnnotationDefaults {
            number,
            text,
            line,
            arrow,
            rectangle,
            ellipse,
            last_shape,
        },
        warnings,
    }
}

fn load_number_style(parent: &toml::Table, warnings: &mut Vec<String>) -> NumberStyle {
    let table = match parent.get("number") {
        Some(toml::Value::Table(t)) => t,
        None => return NumberStyle::default(),
        _ => {
            warnings.push("annotation_defaults.number is not a table — using defaults".into());
            return NumberStyle::default();
        }
    };

    let accent = table
        .get("accent")
        .and_then(deserialize_rgb8)
        .unwrap_or_else(|| {
            if table.contains_key("accent") {
                warnings.push("annotation_defaults.number.accent invalid — using default".into());
            }
            NumberStyle::default().accent
        });

    let size = table
        .get("size")
        .and_then(deserialize_number_size)
        .unwrap_or_else(|| {
            if table.contains_key("size") {
                warnings.push("annotation_defaults.number.size invalid — using default".into());
            }
            NumberStyle::default().size
        });

    NumberStyle { accent, size }
}

fn load_text_style(parent: &toml::Table, warnings: &mut Vec<String>) -> TextStyle {
    let table = match parent.get("text") {
        Some(toml::Value::Table(t)) => t,
        None => return TextStyle::default(),
        _ => {
            warnings.push("annotation_defaults.text is not a table — using defaults".into());
            return TextStyle::default();
        }
    };

    let font_size = table
        .get("font_size")
        .and_then(deserialize_text_size)
        .unwrap_or_else(|| {
            if table.contains_key("font_size") {
                warnings.push("annotation_defaults.text.font_size invalid — using default".into());
            }
            TextStyle::default().font_size
        });

    let text_color = table
        .get("text_color")
        .and_then(deserialize_rgb8)
        .unwrap_or_else(|| TextStyle::default().text_color);

    let background = match table.get("background") {
        Some(value) => deserialize_rgb8(value).or_else(|| {
            warnings.push("annotation_defaults.text.background invalid — using default".into());
            TextStyle::default().background
        }),
        None if table
            .get("background_enabled")
            .and_then(toml::Value::as_bool)
            == Some(false) =>
        {
            None
        }
        None => TextStyle::default().background,
    };

    TextStyle {
        font_size,
        text_color,
        background,
    }
}

fn load_stroke_style(parent: &toml::Table, key: &str, warnings: &mut Vec<String>) -> StrokeStyle {
    let table = match parent.get(key) {
        Some(toml::Value::Table(t)) => t,
        None => return StrokeStyle::default(),
        _ => {
            warnings.push(format!(
                "annotation_defaults.{key} is not a table — using defaults"
            ));
            return StrokeStyle::default();
        }
    };

    let defaults = StrokeStyle::default();
    let mut invalid = false;
    let color = match table.get("color") {
        Some(value) => match deserialize_rgb8(value) {
            Some(color) => color,
            None => {
                invalid = true;
                defaults.color
            }
        },
        None => defaults.color,
    };
    let width = match table.get("width") {
        Some(value) => match value.as_float().map(|width| width as f32) {
            Some(width) if width.is_finite() && width > 0.0 => width,
            _ => {
                invalid = true;
                defaults.width
            }
        },
        None => defaults.width,
    };
    let opacity = match table.get("opacity") {
        Some(value) if value.as_float() == Some(1.0) => 1.0,
        Some(_) => {
            invalid = true;
            1.0
        }
        None => 1.0,
    };

    if invalid {
        warnings.push(format!(
            "annotation_defaults.{key} contains invalid stroke values — using defaults"
        ));
    }

    StrokeStyle {
        color,
        width,
        opacity,
    }
}

fn load_shape_defaults(
    parent: &toml::Table,
    key: &str,
    warnings: &mut Vec<String>,
) -> ShapeDefaults {
    let table = match parent.get(key) {
        Some(toml::Value::Table(t)) => t,
        None => return ShapeDefaults::default(),
        _ => {
            warnings.push(format!(
                "annotation_defaults.{key} is not a table — using defaults"
            ));
            return ShapeDefaults::default();
        }
    };

    let stroke = load_stroke_style(table, "stroke", warnings);

    let fill_enabled = table
        .get("fill_enabled")
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);

    let fill_color = table
        .get("fill_color")
        .and_then(deserialize_rgb8)
        .unwrap_or_else(|| {
            if table.contains_key("fill_color") {
                warnings.push(format!(
                    "annotation_defaults.{key}.fill_color invalid — using default"
                ));
            }
            ShapeDefaults::default().fill_color
        });

    ShapeDefaults {
        stroke,
        fill_enabled,
        fill_color,
    }
}

fn load_last_shape(parent: &toml::Table, warnings: &mut Vec<String>) -> ShapeKind {
    match parent.get("last_shape").and_then(toml::Value::as_str) {
        Some("rectangle") => ShapeKind::Rectangle,
        Some("ellipse") => ShapeKind::Ellipse,
        Some(other) => {
            warnings.push(format!(
                "annotation_defaults.last_shape invalid ({other:?}) — using default"
            ));
            ShapeKind::Rectangle
        }
        None => ShapeKind::Rectangle,
    }
}

fn deserialize_rgb8(v: &toml::Value) -> Option<Rgb8> {
    // Try table form: { r: 0, g: 0, b: 0 }
    if let toml::Value::Table(t) = v {
        let r = u8::try_from(t.get("r").and_then(|v| v.as_integer())?).ok()?;
        let g = u8::try_from(t.get("g").and_then(|v| v.as_integer())?).ok()?;
        let b = u8::try_from(t.get("b").and_then(|v| v.as_integer())?).ok()?;
        return Some(Rgb8::new(r, g, b));
    }
    None
}

fn deserialize_number_size(v: &toml::Value) -> Option<NumberSize> {
    let s = v.as_str()?;
    match s {
        "Small" => Some(NumberSize::Small),
        "Medium" => Some(NumberSize::Medium),
        "Large" => Some(NumberSize::Large),
        _ => None,
    }
}

fn deserialize_text_size(v: &toml::Value) -> Option<TextSize> {
    let s = v.as_str()?;
    match s {
        "Px14" => Some(TextSize::Px14),
        "Px18" => Some(TextSize::Px18),
        "Px24" => Some(TextSize::Px24),
        "Px32" => Some(TextSize::Px32),
        _ => None,
    }
}

pub fn save_to(path: &Path, values: &AnnotationDefaults) -> Result<(), String> {
    save_to_with_writer(path, values, write_temp_sync_rename)
}

pub(crate) fn save_to_with_writer(
    path: &Path,
    values: &AnnotationDefaults,
    writer: impl FnOnce(&Path, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let mut root = read_existing_table_or_empty(path)?;
    let mut persisted = values.clone();
    persisted.line.opacity = 1.0;
    persisted.arrow.opacity = 1.0;
    persisted.rectangle.stroke.opacity = 1.0;
    persisted.ellipse.stroke.opacity = 1.0;
    let mut section = toml::Value::try_from(&persisted)
        .map_err(|e| format!("serialize annotation defaults: {e}"))?;
    if let Some(text) = section
        .as_table_mut()
        .and_then(|section| section.get_mut("text"))
        .and_then(toml::Value::as_table_mut)
    {
        text.insert(
            "background_enabled".into(),
            toml::Value::Boolean(values.text.background.is_some()),
        );
    }
    root.insert("annotation_defaults".into(), section);
    let text = toml::to_string_pretty(&root).map_err(|e| format!("serialize config.toml: {e}"))?;
    let parent = path
        .parent()
        .ok_or_else(|| "configuration path has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("create config directory: {e}"))?;
    writer(path, text.as_bytes())
}

fn read_existing_table_or_empty(path: &Path) -> Result<toml::Table, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .parse::<toml::Table>()
            .map_err(|e| format!("invalid config.toml: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(toml::Table::new()),
        Err(e) => Err(format!("failed to read config.toml: {e}")),
    }
}

fn write_temp_sync_rename(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "configuration path has no parent".to_string())?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "configuration path has no file name".to_string())?;
    let temp_name = format!(
        ".{}.tmp.{}",
        file_name.to_string_lossy(),
        std::process::id()
    );
    let temp_path = parent.join(&temp_name);
    let result = (|| -> Result<(), String> {
        std::fs::write(&temp_path, bytes).map_err(|e| format!("write temp config: {e}"))?;
        let file = std::fs::File::open(&temp_path)
            .map_err(|e| format!("open temp config for sync: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("sync temp config: {e}"))?;
        std::fs::rename(&temp_path, path).map_err(|e| format!("rename temp config: {e}"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    struct TestContext {
        dir: TempDir,
    }

    impl TestContext {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().unwrap(),
            }
        }

        fn path(&self) -> std::path::PathBuf {
            self.dir.path().join("config.toml")
        }

        fn write_config(&self, content: &str) {
            fs::write(self.path(), content).unwrap();
        }

        fn read_table(&self) -> toml::Table {
            fs::read_to_string(self.path())
                .unwrap()
                .parse::<toml::Table>()
                .unwrap()
        }
    }

    #[test]
    fn missing_file_returns_defaults_with_no_warnings() {
        let ctx = TestContext::new();
        let loaded = load_from(&ctx.path());
        assert_eq!(loaded.values, AnnotationDefaults::default());
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn missing_fields_use_canonical_defaults() {
        let ctx = TestContext::new();
        ctx.write_config("[annotation_defaults.number]\nsize = \"Large\"\n");
        let loaded = load_from(&ctx.path());
        assert_eq!(loaded.values.number.size, NumberSize::Large);
        assert_eq!(loaded.values.number.accent, NumberStyle::default().accent);
        assert_eq!(loaded.values.text, TextStyle::default());
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn missing_two_point_sections_use_independent_canonical_defaults() {
        let ctx = TestContext::new();
        ctx.write_config("[annotation_defaults.number]\nsize = \"Medium\"\n");
        let loaded = load_from(&ctx.path());
        assert_eq!(loaded.values.line, StrokeStyle::default());
        assert_eq!(loaded.values.arrow, StrokeStyle::default());
    }

    #[test]
    fn invalid_line_width_does_not_reset_arrow_defaults() {
        let ctx = TestContext::new();
        ctx.write_config(
            "[annotation_defaults.line]\nwidth = -2.0\nopacity = 0.5\n\
             [annotation_defaults.arrow]\nwidth = 9.0\nopacity = 1.0\n\
             [annotation_defaults.arrow.color]\nr = 1\ng = 2\nb = 3\n",
        );
        let loaded = load_from(&ctx.path());
        assert_eq!(loaded.values.line, StrokeStyle::default());
        assert_eq!(loaded.values.arrow.width, 9.0);
        assert_eq!(loaded.values.arrow.color, Rgb8::new(1, 2, 3));
        assert_eq!(loaded.warnings.len(), 1);
    }

    #[test]
    fn save_preserves_unrelated_config_and_round_trips_two_point_defaults() {
        let ctx = TestContext::new();
        ctx.write_config("[daemon]\ncapture_region_hotkey = \"Alt+Shift+6\"\n");
        let mut values = AnnotationDefaults::default();
        values.line.width = 7.0;
        values.arrow.color = Rgb8::new(10, 20, 30);
        save_to(&ctx.path(), &values).unwrap();
        let text = std::fs::read_to_string(ctx.path()).unwrap();
        assert!(text.contains("capture_region_hotkey"));
        let loaded = load_from(&ctx.path());
        assert_eq!(loaded.values, values);
    }

    #[test]
    fn save_preserves_unrelated_and_unknown_sections() {
        let ctx = TestContext::new();
        ctx.write_config(
            "[daemon]\ncapture_region_hotkey = \"Alt+Shift+6\"\n[future]\nvalue = 9\n",
        );
        save_to(&ctx.path(), &AnnotationDefaults::default()).unwrap();
        let table = ctx.read_table();
        assert!(table.contains_key("daemon"));
        assert!(table.contains_key("future"));
        assert!(table.contains_key("annotation_defaults"));
    }

    #[test]
    fn failed_atomic_write_leaves_existing_config_bytes_unchanged() {
        let ctx = TestContext::new();
        let original = b"[daemon]\ncapture_region_hotkey = \"Alt+Shift+6\"\n";
        ctx.write_config(std::str::from_utf8(original).unwrap());
        let result = save_to_with_writer(
            &ctx.path(),
            &AnnotationDefaults::default(),
            |_path, _bytes| Err("injected atomic write failure".to_string()),
        );
        assert_eq!(result, Err("injected atomic write failure".to_string()));
        assert_eq!(fs::read(ctx.path()).unwrap(), original);
    }

    #[test]
    fn invalid_field_falls_back_with_one_warning() {
        let ctx = TestContext::new();
        ctx.write_config("[annotation_defaults.text]\nfont_size = \"Px99\"\n");
        let loaded = load_from(&ctx.path());
        assert_eq!(loaded.values.text.font_size, TextStyle::default().font_size);
        assert_eq!(
            loaded.values.text.text_color,
            TextStyle::default().text_color
        );
        assert_eq!(
            loaded.values.text.background,
            TextStyle::default().background
        );
        assert!(!loaded.warnings.is_empty());
    }

    #[test]
    fn round_trip_preserves_non_default_values() {
        let ctx = TestContext::new();
        let defaults = AnnotationDefaults {
            number: NumberStyle {
                accent: Rgb8::new(0x00, 0xFF, 0x00),
                size: NumberSize::Large,
            },
            text: TextStyle {
                font_size: TextSize::Px32,
                text_color: Rgb8::new(0x00, 0x00, 0x00),
                background: None,
            },
            ..AnnotationDefaults::default()
        };
        save_to(&ctx.path(), &defaults).unwrap();
        let loaded = load_from(&ctx.path());
        assert_eq!(loaded.values, defaults);
    }

    #[test]
    fn malformed_file_returns_defaults_with_warning() {
        let ctx = TestContext::new();
        ctx.write_config("not = valid = toml {{{{");
        let loaded = load_from(&ctx.path());
        assert_eq!(loaded.values, AnnotationDefaults::default());
        assert!(!loaded.warnings.is_empty());
    }

    // -- shape defaults (Task 3) ----------------------------------------------

    #[test]
    fn missing_file_produces_canonical_rectangle_and_ellipse_defaults() {
        let ctx = TestContext::new();
        let loaded = load_from(&ctx.path());
        assert_eq!(loaded.values.rectangle, ShapeDefaults::default());
        assert_eq!(loaded.values.ellipse, ShapeDefaults::default());
        assert_eq!(loaded.values.last_shape, ShapeKind::Rectangle);
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn missing_shape_fields_produce_independent_canonical_defaults() {
        let ctx = TestContext::new();
        ctx.write_config("[annotation_defaults.rectangle.stroke]\nwidth = 8.0\nopacity = 1.0\n");
        let loaded = load_from(&ctx.path());
        assert_eq!(loaded.values.rectangle.stroke.width, 8.0);
        assert_eq!(loaded.values.ellipse, ShapeDefaults::default());
    }

    #[test]
    fn malformed_rectangle_does_not_reset_ellipse_or_last_shape() {
        let ctx = TestContext::new();
        ctx.write_config(
            "[annotation_defaults]\nlast_shape = \"ellipse\"\n\
             [annotation_defaults.rectangle]\nfill_color = 99\n\
             [annotation_defaults.ellipse]\nfill_enabled = true\n\
             [annotation_defaults.ellipse.fill_color]\nr = 10\ng = 20\nb = 30\n",
        );
        let loaded = load_from(&ctx.path());
        assert_eq!(
            loaded.values.rectangle.fill_color,
            ShapeDefaults::default().fill_color
        );
        assert!(loaded.values.ellipse.fill_enabled);
        assert_eq!(loaded.values.ellipse.fill_color, Rgb8::new(10, 20, 30));
        assert_eq!(loaded.values.last_shape, ShapeKind::Ellipse);
    }

    #[test]
    fn non_opaque_shape_stroke_rejected_to_opacity_one_with_warning() {
        let ctx = TestContext::new();
        ctx.write_config("[annotation_defaults.rectangle.stroke]\nopacity = 0.5\n");
        let loaded = load_from(&ctx.path());
        assert_eq!(loaded.values.rectangle.stroke.opacity, 1.0);
        assert!(!loaded.warnings.is_empty());
    }

    #[test]
    fn rectangle_and_ellipse_changes_never_contaminate() {
        let ctx = TestContext::new();
        let mut values = AnnotationDefaults::default();
        values.rectangle.stroke.width = 12.0;
        values.rectangle.fill_enabled = true;
        values.ellipse.stroke.color = Rgb8::new(0, 255, 0);
        save_to(&ctx.path(), &values).unwrap();
        let loaded = load_from(&ctx.path());
        assert_eq!(loaded.values.rectangle.stroke.width, 12.0);
        assert!(loaded.values.rectangle.fill_enabled);
        assert_eq!(loaded.values.ellipse.stroke.color, Rgb8::new(0, 255, 0));
        assert_eq!(
            loaded.values.ellipse.stroke.width,
            StrokeStyle::default().width
        );
        assert!(!loaded.values.ellipse.fill_enabled);
    }

    #[test]
    fn disabling_and_reloading_fill_retains_fill_color() {
        let ctx = TestContext::new();
        let mut values = AnnotationDefaults::default();
        values.rectangle.fill_enabled = true;
        values.rectangle.fill_color = Rgb8::new(10, 20, 30);
        save_to(&ctx.path(), &values).unwrap();
        values.rectangle.fill_enabled = false;
        save_to(&ctx.path(), &values).unwrap();
        let loaded = load_from(&ctx.path());
        assert!(!loaded.values.rectangle.fill_enabled);
        assert_eq!(loaded.values.rectangle.fill_color, Rgb8::new(10, 20, 30));
    }

    #[test]
    fn valid_last_shape_round_trips_and_malformed_falls_back() {
        let ctx = TestContext::new();
        let values = AnnotationDefaults {
            last_shape: ShapeKind::Ellipse,
            ..AnnotationDefaults::default()
        };
        save_to(&ctx.path(), &values).unwrap();
        let loaded = load_from(&ctx.path());
        assert_eq!(loaded.values.last_shape, ShapeKind::Ellipse);

        ctx.write_config("[annotation_defaults]\nlast_shape = \"bogus\"\n");
        let loaded = load_from(&ctx.path());
        assert_eq!(loaded.values.last_shape, ShapeKind::Rectangle);
        assert!(!loaded.warnings.is_empty());
    }

    #[test]
    fn save_forces_shape_stroke_opacities_and_preserves_existing_fields() {
        let ctx = TestContext::new();
        ctx.write_config(
            "[annotation_defaults.number]\nsize = \"Large\"\n\
             [annotation_defaults.line]\nwidth = 7.0\nopacity = 1.0\n",
        );
        let mut values = load_from(&ctx.path()).values;
        values.rectangle.stroke.opacity = 0.3;
        values.ellipse.stroke.opacity = 0.8;
        save_to(&ctx.path(), &values).unwrap();
        let loaded = load_from(&ctx.path());
        assert_eq!(loaded.values.rectangle.stroke.opacity, 1.0);
        assert_eq!(loaded.values.ellipse.stroke.opacity, 1.0);
        assert_eq!(loaded.values.number.size, NumberSize::Large);
        assert_eq!(loaded.values.line.width, 7.0);
    }
}
