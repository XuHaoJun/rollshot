pub fn run<I, S>(args: I) -> Result<String, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect::<Vec<_>>();

    match args.get(1).map(String::as_str) {
        None | Some("--help" | "-h") => Ok(help()),
        Some("--version" | "-V") => Ok(format!("rollshot {}\n", env!("CARGO_PKG_VERSION"))),
        Some("probe") => Ok(probe()),
        Some("stitch-folder") => stitch_folder(&args[2..]),
        Some(command) => Err(format!("unknown command: {command}\n\n{}", help())),
    }
}

fn help() -> String {
    String::from(
        "rollshot\n\
         \n\
         Usage:\n\
           rollshot probe\n\
           rollshot stitch-folder <frames-dir>\n\
           rollshot --version\n",
    )
}

fn probe() -> String {
    format!(
        "rollshot {}\n\
         os: {}\n\
         real capture: unavailable in bootstrap phase\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
    )
}

fn stitch_folder(args: &[String]) -> Result<String, String> {
    let frames_dir = args
        .first()
        .ok_or_else(|| String::from("usage: rollshot stitch-folder <frames-dir>"))?;

    Ok(format!(
        "stitch-folder: {frames_dir}\n\
         status: not available in bootstrap phase\n",
    ))
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn probe_reports_bootstrap_status() {
        let output = run(["rollshot", "probe"]).expect("probe should succeed");

        assert!(output.contains("rollshot"));
        assert!(output.contains("real capture: unavailable"));
    }

    #[test]
    fn stitch_folder_reports_deferred_status() {
        let output = run(["rollshot", "stitch-folder", "tests/fixtures"]).expect("command runs");

        assert!(output.contains("stitch-folder"));
        assert!(output.contains("not available in bootstrap phase"));
    }
}
