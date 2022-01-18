//! This is the command-line interface to RAMP.

#[macro_use]
extern crate log;

use std::collections::BTreeMap;

use anyhow::Result;
use clap::Parser;
use fs_err::File;
use rand::rngs::StdRng;
use rand::SeedableRng;
use serde::Deserialize;
use simplelog::{ColorChoice, ConfigBuilder, LevelFilter, TermLogger, TerminalMode};

use ramp::utilities;
use ramp::{Input, Model, Population, Snapshot, MSOA};

#[tokio::main]
async fn main() -> Result<()> {
    // Specify the logging format
    TermLogger::init(
        LevelFilter::Info,
        ConfigBuilder::new()
            .set_time_format_str("%H:%M:%S%.3f")
            .set_location_level(LevelFilter::Error)
            .build(),
        TerminalMode::Stderr,
        ColorChoice::Auto,
    )?;

    let args = Args::parse();

    let mut rng = if let Some(seed) = args.rng_seed {
        StdRng::seed_from_u64(seed)
    } else {
        StdRng::from_entropy()
    };

    match args.action {
        Action::Init { region } => {
            let input = region.to_input().await?;
            let population = Population::create(input, &mut rng).await?;

            info!("By the end, {}", utilities::memory_usage());
            let output = format!("processed_data/{:?}.bin", region);
            info!("Writing population to {}", output);
            utilities::write_binary(&population, output)?;
        }
        Action::PythonCache { region } => {
            info!("Loading population");
            let population =
                utilities::read_binary::<Population>(format!("processed_data/{:?}.bin", region))?;
            let output = format!("processed_data/python_cache_{:?}", region);
            info!("Writing Python cache files to {}", output);
            population.write_python_cache(output)?;
        }
        Action::Snapshot { region } => {
            info!("Loading population");
            let population =
                utilities::read_binary::<Population>(format!("processed_data/{:?}.bin", region))?;
            // TODO Based on input parameters like start-date, maybe trim the lockdown list
            let output = format!("processed_data/snapshot_{:?}.npz", region);
            info!("Writing snapshot to {}", output);
            Snapshot::convert_to_npz(population, output, &mut rng)?;
        }
        Action::RunModel { region } => {
            info!("Loading population");
            let population =
                utilities::read_binary::<Population>(format!("processed_data/{:?}.bin", region))?;
            let mut model = Model::new(population, rng)?;
            model.run()?;
        }
    }

    Ok(())
}

#[derive(Parser)]
#[clap(about, version, author)]
struct Args {
    #[clap(subcommand)]
    action: Action,
    /// By default, the output will be different every time the tool is run, based on a different
    /// random number generator seed. Specify this to get deterministic behavior, given the same
    /// input.
    #[clap(long)]
    rng_seed: Option<u64>,
}

#[derive(clap::ArgEnum, Clone, Copy, Debug)]
/// Which counties to operate on
enum Region {
    WestYorkshireSmall,
    WestYorkshireLarge,
    Devon,
    TwoCounties,
    National,
}

// TODO Reading in the Population is slow; just combine the two actions
#[derive(clap::Subcommand, Clone)]
enum Action {
    /// Import raw data and build an activity model for a region
    Init {
        #[clap(arg_enum)]
        region: Region,
    },
    /// Transform a Population to the Python InitialisationCache format
    PythonCache {
        #[clap(arg_enum)]
        region: Region,
    },
    /// Transform a Population into a Snapshot
    Snapshot {
        #[clap(arg_enum)]
        region: Region,
    },
    /// Run the model, for a fixed number of days
    RunModel {
        #[clap(arg_enum)]
        region: Region,
    },
}

impl Region {
    async fn to_input(self) -> Result<Input> {
        let mut input = Input {
            initial_cases_per_msoa: BTreeMap::new(),
        };

        // Determine the MSOAs to operate on using CSV files from the original repo
        let csv_input = match self {
            Region::WestYorkshireSmall => "Input_Test_3.csv",
            Region::WestYorkshireLarge => "Input_WestYorkshire.csv",
            Region::Devon => "Input_Devon.csv",
            Region::TwoCounties => "Input_Test_accross.csv",
            Region::National => {
                for msoa in MSOA::all_msoas_nationally().await? {
                    input.initial_cases_per_msoa.insert(msoa, default_cases());
                }
                return Ok(input);
            }
        };
        let csv_path = format!("model_parameters/{}", csv_input);
        for rec in csv::Reader::from_reader(File::open(csv_path)?).deserialize() {
            let rec: InitialCaseRow = rec?;
            input.initial_cases_per_msoa.insert(rec.msoa, rec.cases);
        }
        Ok(input)
    }
}

#[derive(Deserialize)]
struct InitialCaseRow {
    #[serde(rename = "MSOA11CD")]
    msoa: MSOA,
    // This field is missing from some of the input files
    #[serde(default = "default_cases")]
    cases: usize,
}

fn default_cases() -> usize {
    5
}
