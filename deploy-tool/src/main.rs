use byteorder::{BigEndian, WriteBytesExt};
use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::convert::Into;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::{env, fs, io};
use tar::Builder;

macro_rules! println {
    ($($arg:tt)*) => {
        std::println!("\x1b[35m{}\x1b[0m", format_args!($($arg)*));
    };
}

// ─── Configuration ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct Config {
    pub project: ProjectConfig,
}

#[derive(Debug, Deserialize)]
pub struct ProjectConfig {
    pub id: String,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// Returns the repository root by walking up from the current directory
/// until a `Cargo.toml` with `[workspace]` is found.
pub fn get_repo_root() -> Result<PathBuf, Box<dyn Error>> {
    let mut current = env::current_dir()?;
    loop {
        let cargo_toml = current.join("Cargo.toml");
        if cargo_toml.exists() {
            let content = fs::read_to_string(&cargo_toml)?;
            if content.contains("[workspace]") {
                return Ok(current);
            }
        }
        if !current.pop() {
            return Err("Could not find repository root (no workspace Cargo.toml found)".into());
        }
    }
}

/// Reads the deploy.toml config file from the repository root.
fn read_config() -> Result<Config, Box<dyn Error>> {
    let repo_root = get_repo_root()?;
    let config_path = repo_root.join("deploy.toml");
    let content = fs::read_to_string(&config_path).map_err(|e| {
        format!(
            "Could not read config file at {}: {}",
            config_path.display(),
            e
        )
    })?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

/// Initializes the global config. Must be called before using `config()`.
fn init_config() -> Result<(), Box<dyn Error>> {
    let cfg = read_config()?;
    CONFIG
        .set(cfg)
        .map_err(|_| "Config already initialized")?;
    Ok(())
}

/// Returns a reference to the global config.
pub fn config() -> &'static Config {
    CONFIG.get().expect("Config not initialized. Call init_config() first.")
}

fn default_storage_path(file: &str) -> PathBuf {
    let mut p = dirs::config_dir().expect("Could not find config directory");
    p.push(&config().project.id);
    p.push(file);
    p
}

#[derive(Parser)]
#[command(name = "deploy-tool")]
#[command(about = "Deployment tool for freenet pizza app")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Deploy the application
    Deploy {
        /// Version number
        #[arg(long, short, default_value = "1")]
        version: u32,
    },
    /// Launch application in development mode
    Dev {},
    /// Initial deployment of the webapp
    InitialWebDeploy {},
    /// Get web contract id
    GetWebContractId {},
}

fn execute(cmd: &mut Command) -> Result<(), Box<dyn Error>> {
    println!("Running: {:?}", cmd);
    let status = cmd.status()?;
    if !status.success() {
        return Err(format!("Command {:?} failed with status: {}", cmd, status).into());
    }
    Ok(())
}

fn cargo_bin_or_path(cmd: &str, pkg: &str) -> String {
    if let Ok(path) = std::env::var("PATH") {
        for p in std::env::split_paths(&path) {
            let bin_path = p.join(cmd);
            if bin_path.exists() {
                return bin_path.to_str().unwrap().into();
            }
        }
    }
    let mut home = dirs::home_dir().expect("Could not find home directory");
    home.push(".cargo");
    home.push("bin");
    home.push(cmd);
    if !home.exists() {
        println!("fdev not found, installing...");
        execute(Command::new("cargo").args(["install", pkg])).unwrap();
    }
    home.to_string_lossy().to_string()
}

fn cargo_build(package: &str) -> Result<(), Box<dyn Error>> {
    println!("Building package: {}", package);
    let args = vec![
        "build",
        "--release",
        "--target",
        "wasm32-unknown-unknown",
        "--package",
        package,
    ];
    execute(Command::new("cargo").args(args))
}

fn web_container_sign(
    input: PathBuf,
    output: PathBuf,
    parameters: PathBuf,
    version: u32,
) -> Result<(), Box<dyn Error>> {
    println!(
        "Signing web container: {} -> {}",
        input.display(),
        output.display()
    );
    execute(Command::new("cargo").args([
        "run",
        "--bin",
        "web-container-tool",
        "--",
        "sign",
        "--input",
        &input.to_string_lossy(),
        "--output",
        &output.to_string_lossy(),
        "--parameters",
        &parameters.to_string_lossy(),
        "--version",
        &version.to_string(),
    ]))
}

fn web_container_generate() -> Result<(), Box<dyn Error>> {
    println!("Generating web container keys...");
    execute(Command::new("cargo").args(["run", "--bin", "web-container-tool", "--", "generate"]))
}

fn fdev_publish(
    contract_wasm: PathBuf,
    webapp_parameters: PathBuf,
    state: PathBuf,
) -> Result<(), Box<dyn Error>> {
    println!("Publishing contract...");
    execute(Command::new(cargo_bin_or_path("fdev", "fdev")).args([
        "publish",
        "--code",
        &contract_wasm.to_string_lossy(),
        "--parameters",
        &webapp_parameters.to_string_lossy(),
        "contract",
        "--state",
        &state.to_string_lossy(),
    ]))
}

fn find_public_dir(root: &Path) -> io::Result<Option<PathBuf>> {
    if !root.is_dir() {
        return Ok(None);
    }

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            if path.file_name().map(|n| n == "public").unwrap_or(false) {
                return Ok(Some(path));
            }

            if let Some(found) = find_public_dir(&path)? {
                return Ok(Some(found));
            }
        }
    }

    Ok(None)
}

fn build_dx_app(
    contract_wasm: PathBuf,
    webapp_parameters: PathBuf,
    package: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    println!("Building app...");

    let contract_id = fdev_get_contract_id(contract_wasm, webapp_parameters)?;

    let temp_dir = env::temp_dir();

    println!("temp {:?}", temp_dir);

    execute(
        Command::new(cargo_bin_or_path("dx", "dioxus-cli"))
            .env("CARGO_TARGET_DIR", &temp_dir)
            .args([
                "build",
                "--package",
                package,
                "--base-path",
                format!("/v1/contract/web/{}/", contract_id).as_str(),
                "--release",
            ]),
    )?;

    let public_dir = find_public_dir(&temp_dir)?.expect("public path");

    Ok(public_dir)
}

fn fdev_get_contract_id(
    contract_wasm: PathBuf,
    webapp_parameters: PathBuf,
) -> Result<String, Box<dyn Error>> {
    let output = Command::new(cargo_bin_or_path("fdev", "fdev"))
        .args([
            "get-contract-id",
            "--code",
            &contract_wasm.to_string_lossy(),
            "--parameters",
            &webapp_parameters.to_string_lossy(),
        ])
        .output()?; // run and capture

    if !output.status.success() {
        return Err(format!(
            "Command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    let contract_id = String::from_utf8(output.stdout)?.trim().to_string();

    Ok(contract_id)
}

fn dev() -> Result<(), Box<dyn Error>> {
    Ok(())
}

fn add_dir_to_tar<W: io::Write>(
    tar: &mut Builder<W>,
    src_dir: &Path,
    base: &Path,
) -> io::Result<()> {
    for entry in fs::read_dir(src_dir)? {
        let entry = entry?;
        let path = entry.path();

        // path inside archive (relative)
        let archive_path: PathBuf = path.strip_prefix(base).unwrap().into();

        if path.is_dir() {
            tar.append_dir(&archive_path, &path)?;
            add_dir_to_tar(tar, &path, base)?;
        } else if path.is_file() {
            tar.append_path_with_name(&path, &archive_path)?;
        }
    }

    Ok(())
}

fn deploy(version: u32) -> Result<(), Box<dyn Error>> {
    let contract_wasm = default_storage_path("web.contract.wasm");
    let webapp_archive = default_storage_path("webapp.bootstrap.tar.xz");
    let webapp_metadata = default_storage_path("webapp.metadata");
    let webapp_parameters = default_storage_path("webapp.parameters");

    let version_saved = std::fs::read_to_string(default_storage_path("version"))?;
    let version_parsed: u32 = version_saved.parse().unwrap();
    let version_chosen = std::cmp::max(version_parsed, version);

    let out = build_dx_app(contract_wasm.clone(), webapp_parameters.clone(), "pizza-ui")?;

    // Create the tar.xz archive
    let file = std::fs::File::create(&webapp_archive)?;
    let enc = xz2::write::XzEncoder::new(file, 6);
    let mut tar = tar::Builder::new(enc);
    add_dir_to_tar(&mut tar, &out, &out)?;
    // IMPORTANT: Must explicitly finish the XZ encoder to ensure all data is written
    // before the file is read. into_inner() returns the encoder which must be finished.
    tar.into_inner()?.finish()?;

    web_container_sign(
        webapp_archive.clone(),
        webapp_metadata.clone(),
        webapp_parameters.clone(),
        version_chosen.clone(),
    )?;

    let state_path = default_storage_path("webapp.state");
    merge_state(&webapp_metadata, &webapp_archive, &state_path)?;

    fdev_publish(contract_wasm, webapp_parameters, state_path)?;

    fs::write(
        default_storage_path("version"),
        (version_chosen + 1).to_string(),
    )?;

    Ok(())
}

fn initial_web_deploy() -> Result<(), Box<dyn Error>> {
    println!("Performing initial web deployment...");

    // NOTE: this means all folders needed already exist after this
    let keys_path = default_storage_path("web-container-keys.toml");
    if !keys_path.exists() {
        web_container_generate()?;
    }

    cargo_build("web-container-contract")?;

    let contract_wasm_src =
        PathBuf::from("target/wasm32-unknown-unknown/release/web_container_contract.wasm");
    let contract_wasm = default_storage_path("web.contract.wasm");
    std::fs::copy(contract_wasm_src, &contract_wasm)?;
    let webapp_archive = default_storage_path("webapp.bootstrap.tar.xz");
    // Create an empty .tar.xz archive
    let file = std::fs::File::create(&webapp_archive)?;
    let enc = xz2::write::XzEncoder::new(file, 6);
    let mut tar = tar::Builder::new(enc);
    let mut header = tar::Header::new_gnu();
    let content = b"<tt>wip</tt>";
    header.set_size(content.len() as u64);
    header.set_mode(0o644);
    tar.append_data(&mut header, "index.html", &content[..])?;
    // IMPORTANT: Must explicitly finish the XZ encoder to ensure all data is written
    // before the file is read. into_inner() returns the encoder which must be finished.
    tar.into_inner()?.finish()?;
    let webapp_metadata = default_storage_path("webapp.metadata");
    let webapp_parameters = default_storage_path("webapp.parameters");

    web_container_sign(
        webapp_archive.clone(),
        webapp_metadata.clone(),
        webapp_parameters.clone(),
        1,
    )?;

    let state_path = default_storage_path("webapp.state");
    merge_state(&webapp_metadata, &webapp_archive, &state_path)?;

    fdev_publish(contract_wasm, webapp_parameters, state_path)?;

    Ok(())
}

fn get_web_contract_id() -> Result<(), Box<dyn Error>> {
    let contract_wasm = default_storage_path("web.contract.wasm");
    let webapp_parameters = default_storage_path("webapp.parameters");

    let contract_id = fdev_get_contract_id(contract_wasm, webapp_parameters)?;

    println!("{}", contract_id);

    Ok(())
}

fn merge_state(
    metadata_path: &PathBuf,
    webapp_archive_path: &PathBuf,
    output_state_path: &PathBuf,
) -> Result<(), Box<dyn Error>> {
    let metadata_bytes = std::fs::read(metadata_path)?;
    let webapp_bytes = std::fs::read(webapp_archive_path)?;
    let mut state = Vec::new();
    state.write_u64::<BigEndian>(metadata_bytes.len() as u64)?;
    state.extend_from_slice(&metadata_bytes);
    state.write_u64::<BigEndian>(webapp_bytes.len() as u64)?;
    state.extend_from_slice(&webapp_bytes);
    std::fs::write(output_state_path, state)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    init_config()?;

    let cli = Cli::parse();
    match cli.command {
        Commands::Deploy { version } => deploy(version),
        Commands::Dev {} => dev(),
        Commands::InitialWebDeploy {} => initial_web_deploy(),
        Commands::GetWebContractId {} => get_web_contract_id(),
    }
}
