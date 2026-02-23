use clap::{Parser, Subcommand};
use ed25519_dalek::{Signature, Signer, SigningKey};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Metadata stored in web container state, signed by the publisher.
#[derive(Serialize, Deserialize)]
struct WebContainerMetadata {
    version: u32,
    signature: Signature,
}

#[derive(Parser)]
#[command(name = "web-container-tool")]
#[command(about = "Web container key management and signing tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a new Ed25519 keypair and save to a TOML file
    Generate {
        /// Output file (default: ~/.config/freenet-search-engine/web-container-keys.toml)
        #[arg(long, short)]
        output: Option<String>,
    },
    /// Sign a compressed webapp archive
    Sign {
        /// Input compressed webapp file (e.g. webapp.tar.xz)
        #[arg(long, short)]
        input: String,
        /// Output file for CBOR metadata (signature + version)
        #[arg(long, short)]
        output: String,
        /// Output file for contract parameters (32-byte verifying key)
        #[arg(long)]
        parameters: String,
        /// Version number (must be higher than previously published)
        #[arg(long, short)]
        version: u32,
        /// Key file to use (default: ~/.config/freenet-search-engine/web-container-keys.toml)
        #[arg(long, short)]
        key_file: Option<String>,
    },
}

fn default_keys_path() -> PathBuf {
    let project_id = env::var("PROJECT_ID").expect("PROJECT_ID environment variable must be set");
    let mut p = dirs::config_dir().expect("Could not find config directory");
    p.push(project_id);
    p.push("web-container-keys.toml");
    p
}

fn generate_keys(output_path: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
    let verifying_key = signing_key.verifying_key();

    let sk_str = bs58::encode(signing_key.to_bytes()).into_string();
    let vk_str = bs58::encode(verifying_key.to_bytes()).into_string();

    let config = toml::toml! {
        [keys]
        signing_key = sk_str
        verifying_key = vk_str
    };

    let path = output_path
        .map(PathBuf::from)
        .unwrap_or_else(default_keys_path);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&path, toml::to_string(&config)?)?;
    println!("Keys written to: {}", path.display());
    Ok(())
}

fn read_signing_key(key_file: Option<&str>) -> Result<SigningKey, Box<dyn std::error::Error>> {
    let path = key_file
        .map(PathBuf::from)
        .unwrap_or_else(default_keys_path);
    let config: toml::Table = toml::from_str(&fs::read_to_string(&path)?)?;

    let sk_str = config["keys"]["signing_key"]
        .as_str()
        .ok_or("Missing keys.signing_key in config")?;

    // Support both plain base58 and River's "river:v1:sk:" prefixed format
    let raw = sk_str.strip_prefix("river:v1:sk:").unwrap_or(sk_str);

    let decoded = bs58::decode(raw).into_vec()?;
    let bytes: [u8; 32] = decoded
        .try_into()
        .map_err(|_| "Signing key must be 32 bytes")?;

    Ok(SigningKey::from_bytes(&bytes))
}

fn sign_webapp(
    input: String,
    output: String,
    parameters: String,
    version: u32,
    key_file: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let signing_key = read_signing_key(key_file.as_deref())?;
    let webapp_bytes = fs::read(&input)?;

    // Sign (version || webapp)
    let mut message = version.to_be_bytes().to_vec();
    message.extend_from_slice(&webapp_bytes);
    let signature = signing_key.sign(&message);

    // Write CBOR metadata
    let metadata = WebContainerMetadata { version, signature };
    let mut metadata_bytes = Vec::new();
    ciborium::ser::into_writer(&metadata, &mut metadata_bytes)?;

    let mut out = fs::File::create(&output)?;
    out.write_all(&metadata_bytes)?;
    println!(
        "Metadata written to: {} ({} bytes)",
        output,
        metadata_bytes.len()
    );

    // Write 32-byte verifying key as parameters
    let vk = signing_key.verifying_key();
    fs::write(&parameters, vk.to_bytes())?;
    println!("Parameters written to: {} (32 bytes)", parameters);

    // Preflight-check
    vk.verify_strict(&message, &signature).unwrap();

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Generate { output } => generate_keys(output),
        Commands::Sign {
            input,
            output,
            parameters,
            version,
            key_file,
        } => sign_webapp(input, output, parameters, version, key_file),
    }
}
