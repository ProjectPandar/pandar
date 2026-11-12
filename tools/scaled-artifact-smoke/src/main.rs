mod agent;
mod fixture;
mod harness;
mod http;
mod live;
mod scenarios;
mod storage;

use std::{env, process::ExitCode};

use anyhow::{Context, bail};
use harness::{HarnessConfig, HarnessMode, ScenarioFilter};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("scaled artifact smoke failed: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|mode| mode == "--live-preflight") {
        if args.len() != 1 {
            usage()?;
        }
        return live::run_preflight();
    }
    let mut config = parse_args(args)?;
    let scratch = tempfile::tempdir().context("create smoke scratch dir")?;
    config.scratch = scratch.path().to_owned();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build smoke runtime")?;
    let result = runtime.block_on(harness::run(config));
    // Stop every Hub task before `scratch` removes their databases.
    drop(runtime);
    result
}

fn parse_args(args: impl IntoIterator<Item = String>) -> anyhow::Result<HarnessConfig> {
    let mut config = HarnessConfig::default();
    let mut args = args.into_iter();
    let Some(mode) = args.next() else {
        usage()?;
        unreachable!();
    };
    match mode.as_str() {
        "--dry-run" => config.mode = HarnessMode::DryRun,
        "--live" => config.mode = HarnessMode::Live,
        _ => usage()?,
    }
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--iterations" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--iterations requires a value"))?;
                config.iterations = value
                    .parse()
                    .map_err(|_| anyhow::anyhow!("--iterations must be a positive integer"))?;
            }
            "--concurrency" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--concurrency requires a value"))?;
                config.concurrency = value
                    .parse()
                    .map_err(|_| anyhow::anyhow!("--concurrency must be a positive integer"))?;
            }
            "--scenario" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--scenario requires a value"))?;
                config.scenario = ScenarioFilter::parse(&value)?;
            }
            _ => bail!("unknown argument {flag}"),
        }
    }
    config.validate()?;
    Ok(config)
}

fn usage() -> anyhow::Result<()> {
    bail!(
        "usage: pandar-scaled-artifact-smoke --dry-run [--iterations N] [--concurrency N] [--scenario all|artifact|fanout|restart|storage|terminal] | --live [--iterations N] [--concurrency N] [--scenario all|artifact|fanout|restart|terminal|nats-reconnect|postgres-reconnect] | --live-preflight"
    )
}

#[cfg(test)]
mod main {
    use super::*;

    mod tests {
        use super::*;

        #[test]
        fn parse_rejects_live_storage_scenario() {
            let err = parse_args(
                ["--live", "--scenario", "storage"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap_err();
            assert!(format!("{err:#}").contains("storage failure scenario is local dry-run only"));
        }

        #[test]
        fn live_all_excludes_manual_fault_scenarios() {
            let config = parse_args(
                ["--live", "--scenario", "all"]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();
            assert_eq!(
                config.included_scenarios(),
                vec![
                    ScenarioFilter::Artifact,
                    ScenarioFilter::Fanout,
                    ScenarioFilter::Restart,
                    ScenarioFilter::Terminal,
                ]
            );
        }
    }
}
