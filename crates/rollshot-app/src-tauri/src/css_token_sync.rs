#[cfg(test)]
mod tests {
    use rollshot_overlay_core::tokens;

    const CSS: &str = include_str!("../../src/App.css");

    fn assert_var(name: &str, value: &str) {
        let needle = format!("{name}: {value};");
        assert!(
            CSS.contains(&needle),
            "App.css :root is missing or drifted from `{needle}` \
             (rollshot_overlay_core::tokens is the source of truth)"
        );
    }

    #[test]
    fn css_crop_tokens_match_rust_tokens() {
        assert_var("--crop-border", &tokens::CROP_BORDER.to_css());
        assert_var(
            "--crop-border-width",
            &format!("{}px", tokens::CROP_BORDER_WIDTH),
        );
        assert_var("--crop-border-halo", &tokens::CROP_BORDER_HALO.to_css());
        assert_var("--crop-mask", &tokens::CROP_MASK.to_css());
        assert_var("--crop-dim", &tokens::CROP_DIM.to_css());
        assert_var("--crop-guide", &tokens::CROP_GUIDE.to_css());
        assert_var(
            "--crop-guide-width",
            &format!("{}px", tokens::CROP_GUIDE_WIDTH),
        );
    }
}
