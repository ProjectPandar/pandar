use std::path::PathBuf;

#[derive(Debug)]
pub(crate) struct Args {
    pub studio_abi_series: String,
    pub label: String,
    pub archive: PathBuf,
    pub checksum: PathBuf,
    pub cli_name: String,
    pub plugin_name: String,
    pub source_name: String,
    pub repo_root: PathBuf,
    pub abi_probe: PathBuf,
    pub abi_probe_args: Vec<String>,
}

pub(crate) fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut label = None;
    let mut studio_abi_series = None;
    let mut archive = None;
    let mut checksum = None;
    let mut cli_name = None;
    let mut plugin_name = None;
    let mut source_name = None;
    let mut repo_root = None;
    let mut abi_probe = None;
    let mut abi_probe_args = Vec::new();
    let mut args = args;

    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("{flag} requires a value\n{}", usage()))?;
        match flag.as_str() {
            "--studio-abi-series" if studio_abi_series.is_none() => studio_abi_series = Some(value),
            "--label" if label.is_none() => label = Some(value),
            "--archive" if archive.is_none() => archive = Some(PathBuf::from(value)),
            "--checksum" if checksum.is_none() => checksum = Some(PathBuf::from(value)),
            "--cli-name" if cli_name.is_none() => cli_name = Some(value),
            "--plugin-name" if plugin_name.is_none() => plugin_name = Some(value),
            "--source-name" if source_name.is_none() => source_name = Some(value),
            "--repo-root" if repo_root.is_none() => repo_root = Some(PathBuf::from(value)),
            "--abi-probe" if abi_probe.is_none() => abi_probe = Some(PathBuf::from(value)),
            "--abi-probe-arg" => {
                if value == "--plugin"
                    || value.starts_with("--plugin=")
                    || value == "--studio-abi-series"
                    || value.starts_with("--studio-abi-series=")
                {
                    return Err(
                        "--plugin and --studio-abi-series are reserved; release-smoke supplies both"
                            .to_owned(),
                    );
                }
                abi_probe_args.push(value);
            }
            "--studio-abi-series"
            | "--label"
            | "--archive"
            | "--checksum"
            | "--cli-name"
            | "--plugin-name"
            | "--source-name"
            | "--repo-root"
            | "--abi-probe" => {
                return Err(format!("{flag} was provided twice"));
            }
            _ => return Err(format!("unknown argument {flag}\n{}", usage())),
        }
    }

    Ok(Args {
        studio_abi_series: studio_abi_series.ok_or_else(usage)?,
        label: label.ok_or_else(usage)?,
        archive: archive.ok_or_else(usage)?,
        checksum: checksum.ok_or_else(usage)?,
        cli_name: cli_name.ok_or_else(usage)?,
        plugin_name: plugin_name.ok_or_else(usage)?,
        source_name: source_name.ok_or_else(usage)?,
        repo_root: repo_root.ok_or_else(usage)?,
        abi_probe: abi_probe.ok_or_else(usage)?,
        abi_probe_args,
    })
}

fn usage() -> String {
    concat!(
        "usage: pandar-release-smoke ",
        "--studio-abi-series <MM.mm.pp> ",
        "--label <linux-amd64|macos-amd64|macos-arm64|windows-amd64> ",
        "--archive <path> --checksum <path> ",
        "--cli-name <filename> --plugin-name <filename> --source-name <filename> ",
        "--repo-root <path> --abi-probe <native-executable> ",
        "[--abi-probe-arg <value>]..."
    )
    .to_owned()
}
