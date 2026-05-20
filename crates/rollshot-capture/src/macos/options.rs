use crate::error::CaptureError;
use crate::types::{CaptureOptions, Region, RegionMode};

pub(super) const NO_PERMISSION_PROMPT_ENV: &str = "ROLLSHOT_NO_PERMISSION_PROMPT";

pub(super) fn options_to_scap_options(
    options: &CaptureOptions,
) -> Result<scap::capturer::Options, CaptureError> {
    let crop_area = region_to_scap_area(&options.region)?;

    Ok(scap::capturer::Options {
        fps: options.fps,
        show_cursor: options.show_cursor,
        show_highlight: false,
        target: None,
        crop_area,
        output_type: scap::frame::FrameType::BGRAFrame,
        output_resolution: scap::capturer::Resolution::Captured,
        excluded_targets: None,
        captures_audio: false,
        exclude_current_process_audio: false,
    })
}

pub(super) fn region_to_scap_area(
    region: &RegionMode,
) -> Result<Option<scap::capturer::Area>, CaptureError> {
    match region {
        RegionMode::FullSource => Ok(None),
        RegionMode::PortalPicker => Err(CaptureError::InvalidConfig {
            message: "--region portal is only supported with --backend linux-portal".to_string(),
        }),
        RegionMode::Manual(region) => {
            if region.x < 0 || region.y < 0 {
                return Err(CaptureError::InvalidConfig {
                    message: "macOS manual region origin must be non-negative".to_string(),
                });
            }

            Ok(Some(scap::capturer::Area {
                origin: scap::capturer::Point {
                    x: region.x as f64,
                    y: region.y as f64,
                },
                size: scap::capturer::Size {
                    width: region.width as f64,
                    height: region.height as f64,
                },
            }))
        }
    }
}

pub(super) fn manual_region(region: &RegionMode) -> Option<Region> {
    match region {
        RegionMode::Manual(region) => Some(*region),
        RegionMode::FullSource | RegionMode::PortalPicker => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{options_to_scap_options, region_to_scap_area, NO_PERMISSION_PROMPT_ENV};
    use crate::error::CaptureError;
    use crate::types::{CaptureOptions, Region, RegionMode};

    #[test]
    fn no_permission_prompt_env_name_is_stable() {
        assert_eq!(NO_PERMISSION_PROMPT_ENV, "ROLLSHOT_NO_PERMISSION_PROMPT");
    }

    #[test]
    fn manual_region_maps_to_scap_area() {
        let area = region_to_scap_area(&RegionMode::Manual(Region {
            x: 10,
            y: 20,
            width: 300,
            height: 200,
        }))
        .expect("valid region")
        .expect("area");
        assert_eq!(area.origin.x, 10.0);
        assert_eq!(area.origin.y, 20.0);
        assert_eq!(area.size.width, 300.0);
        assert_eq!(area.size.height, 200.0);
    }

    #[test]
    fn negative_manual_region_origin_is_rejected() {
        let err = region_to_scap_area(&RegionMode::Manual(Region {
            x: -1,
            y: 0,
            width: 300,
            height: 200,
        }))
        .expect_err("negative origin rejected");
        assert!(matches!(err, CaptureError::InvalidConfig { .. }));
    }

    #[test]
    fn portal_picker_is_rejected_on_macos() {
        let err = region_to_scap_area(&RegionMode::PortalPicker).expect_err("portal rejected");
        assert!(matches!(err, CaptureError::InvalidConfig { .. }));
    }

    #[test]
    fn capture_options_map_to_scap_options() {
        let options = CaptureOptions {
            region: RegionMode::Manual(Region {
                x: 4,
                y: 5,
                width: 640,
                height: 480,
            }),
            fps: 12,
            show_cursor: true,
            prefer_portal_region: true,
        };
        let scap_options = options_to_scap_options(&options).expect("valid options");
        assert_eq!(scap_options.fps, 12);
        assert!(scap_options.show_cursor);
        assert!(!scap_options.show_highlight);
        assert!(!scap_options.captures_audio);
        assert!(!scap_options.exclude_current_process_audio);
        assert!(matches!(
            scap_options.output_type,
            scap::frame::FrameType::BGRAFrame
        ));
        assert!(matches!(
            scap_options.output_resolution,
            scap::capturer::Resolution::Captured
        ));
        assert!(scap_options.crop_area.is_some());
    }
}
