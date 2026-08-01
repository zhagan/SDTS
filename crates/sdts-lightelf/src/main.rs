mod device;
mod protocol;

use std::time::Duration;

use anyhow::{bail, Context, Result};
use btleplug::api::WriteType;
use clap::{Parser, Subcommand, ValueEnum};
use futures_util::StreamExt;

use protocol::{ColorMode, ExtendedSettings, LightChannel, Settings};

#[derive(Parser)]
#[command(
    name = "sdts-lightelf",
    about = "Standalone BLE controller for the LightElf/TD5322A_ laser device.\n\
             Reverse-engineered from the official app — see src/protocol.rs for caveats."
)]
struct Cli {
    /// How long to scan for the device before giving up.
    #[arg(long, global = true, default_value_t = 5)]
    scan_seconds: u64,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List nearby TD5322A_ devices without connecting.
    Scan,
    /// Connect and dump every GATT service/characteristic/property. Use
    /// this if `set`/`raw` report success but nothing happens on the
    /// device — it tells you the real write characteristic and whether it
    /// actually supports the write type you're using.
    Inspect,
    /// Build and send a settings command (power/light/color mode/angle).
    Set(SetArgs),
    /// Send a raw uppercase hex command string, for protocol exploration.
    Raw {
        /// Full hex string, e.g. 00010203... 04050607.
        hex: String,
        /// Print the bytes instead of sending them.
        #[arg(long)]
        dry_run: bool,
        /// Use write-with-response instead of write-without-response.
        #[arg(long)]
        response: bool,
    },
}

#[derive(clap::Args)]
struct SetArgs {
    /// Power/intensity level, app-observed range 1..=512.
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..=512))]
    power: u16,

    /// Which of the device's three light buttons to select.
    #[arg(long, value_enum)]
    light: LightArg,

    /// Single- or dual-color output.
    #[arg(long, value_enum, default_value_t = ModeArg::Single)]
    mode: ModeArg,

    /// Beam/pattern angle preset (8 positions on the device).
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=7))]
    angle: u8,

    /// Raw `ch` field. Defaults to 0 (single connected device).
    #[arg(long, default_value_t = 0)]
    channel: u8,

    /// valArr[1] ("displaySize" in the app), app-documented range 10..=100.
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u8).range(10..=100))]
    display_size: u8,

    /// Per-channel level (valArr[2..4]) for --mode dual, 0..=100 each.
    /// Ignored in single mode (the device forces these to max there).
    /// Confirmed on hardware: all-zero produces no visible output.
    #[arg(long, num_args = 3, default_values_t = [100, 100, 100], value_parser = clap::value_parser!(u8).range(0..=100))]
    dual_channels: Vec<u8>,

    /// Send the "cmdNewType" extended block with this brightness value
    /// (0..=255) instead of the legacy 5-byte filler. Separate from
    /// --power: the app treats `brightness` as its own field, only present
    /// in this extended block, not part of valArr. Try this if --power
    /// doesn't visibly change anything.
    #[arg(long)]
    brightness: Option<u8>,

    /// Print the resulting hex frame instead of connecting and sending it.
    #[arg(long)]
    dry_run: bool,

    /// Use write-with-response instead of write-without-response.
    #[arg(long)]
    response: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum LightArg {
    One,
    Two,
    Three,
}

impl From<LightArg> for LightChannel {
    fn from(v: LightArg) -> Self {
        match v {
            LightArg::One => LightChannel::One,
            LightArg::Two => LightChannel::Two,
            LightArg::Three => LightChannel::Three,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    Single,
    Dual,
}

impl From<ModeArg> for ColorMode {
    fn from(v: ModeArg) -> Self {
        match v {
            ModeArg::Single => ColorMode::Single,
            ModeArg::Dual => ColorMode::Dual,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Scan => run_scan(cli.scan_seconds).await,
        Command::Inspect => run_inspect(cli.scan_seconds).await,
        Command::Set(args) => run_set(cli.scan_seconds, args).await,
        Command::Raw { hex, dry_run, response } => run_raw(cli.scan_seconds, hex, dry_run, response).await,
    }
}

async fn run_inspect(scan_seconds: u64) -> Result<()> {
    println!("Scanning for {}* devices for {scan_seconds}s...", device::DEVICE_NAME_PREFIX);
    let (name, services) = device::inspect_first(scan_seconds).await?;
    println!("Connected to {name}. GATT tree:");
    for service in &services {
        println!("  service {}", service.service_uuid);
        for c in &service.chars {
            let mut props = Vec::new();
            if c.properties.contains(btleplug::api::CharPropFlags::READ) {
                props.push("read");
            }
            if c.properties.contains(btleplug::api::CharPropFlags::WRITE) {
                props.push("write");
            }
            if c.properties.contains(btleplug::api::CharPropFlags::WRITE_WITHOUT_RESPONSE) {
                props.push("write_without_response");
            }
            if c.properties.contains(btleplug::api::CharPropFlags::NOTIFY) {
                props.push("notify");
            }
            if c.properties.contains(btleplug::api::CharPropFlags::INDICATE) {
                props.push("indicate");
            }
            println!("    char {}  [{}]", c.uuid, props.join(", "));
        }
    }
    Ok(())
}

async fn run_scan(scan_seconds: u64) -> Result<()> {
    println!("Scanning for {}* devices for {scan_seconds}s...", device::DEVICE_NAME_PREFIX);
    let devices = device::scan(scan_seconds).await?;
    if devices.is_empty() {
        println!("No matching devices found. Confirm Bluetooth is on and the device is powered/advertising.");
        return Ok(());
    }
    for d in &devices {
        println!("  {}", d.name);
    }
    Ok(())
}

async fn run_set(scan_seconds: u64, args: SetArgs) -> Result<()> {
    let dual_channels: [u8; 3] = args
        .dual_channels
        .clone()
        .try_into()
        .expect("clap num_args = 3 guarantees exactly 3 values");
    let settings = Settings {
        power: args.power,
        channel: args.channel,
        xy_angle: args.angle,
        light: args.light.into(),
        color_mode: args.mode.into(),
        display_size: args.display_size,
        dual_channels,
        extended: args.brightness.map(|brightness| ExtendedSettings {
            brightness,
            grating: 0,
            device_password_status: 0,
            device_password: 0,
        }),
    };
    let hex = settings.to_hex_string();

    if args.dry_run {
        println!("{hex}");
        return Ok(());
    }

    println!("Connecting to {}* (up to {scan_seconds}s)...", device::DEVICE_NAME_PREFIX);
    let dev = device::connect_first(scan_seconds).await?;
    let write_type = if args.response { WriteType::WithResponse } else { WriteType::WithoutResponse };
    dev.subscribe_all_notifications().await?;
    let mut notifications = dev.notifications().await?;

    println!("Connected to {}. Sending handshake query (E0E1E2E3E4E5E6E7)...", dev.name);
    dev.send_as(&device::QUERY_FRAME, write_type).await?;
    report_notifications(&mut notifications, Duration::from_millis(1500)).await;

    println!("Sending ({write_type:?}): {hex}");
    dev.send_as(&settings.to_bytes(), write_type).await?;
    report_notifications(&mut notifications, Duration::from_millis(1000)).await;
    dev.disconnect().await?;
    println!("Done.");
    Ok(())
}

/// Print every notification received within `window`, or say there were
/// none. This is how we find out whether the device is acking/rejecting
/// what we send it, since a successful BLE write carries no application-
/// level confirmation on its own.
async fn report_notifications(
    stream: &mut (impl futures_util::Stream<Item = btleplug::api::ValueNotification> + Unpin),
    window: Duration,
) {
    let deadline = tokio::time::Instant::now() + window;
    let mut count = 0;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => break,
            next = stream.next() => match next {
                Some(n) => {
                    count += 1;
                    let hex: String = n.value.iter().map(|b| format!("{b:02X}")).collect();
                    println!("  notify {}: {hex}", n.uuid);
                }
                None => break,
            },
        }
    }
    if count == 0 {
        println!("  (no notifications within {window:?})");
    }
}

async fn run_raw(scan_seconds: u64, hex: String, dry_run: bool, response: bool) -> Result<()> {
    let hex = hex.trim().to_uppercase();
    if hex.len() % 2 != 0 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("hex string must have an even number of hex digits");
    }
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();

    if dry_run {
        println!("{hex}");
        return Ok(());
    }

    let dev = device::connect_first(scan_seconds)
        .await
        .context("connecting before raw send")?;
    let write_type = if response { WriteType::WithResponse } else { WriteType::WithoutResponse };
    dev.subscribe_all_notifications().await?;
    let mut notifications = dev.notifications().await?;

    println!("Connected to {}. Sending handshake query (E0E1E2E3E4E5E6E7)...", dev.name);
    dev.send_as(&device::QUERY_FRAME, write_type).await?;
    report_notifications(&mut notifications, Duration::from_millis(1500)).await;

    println!("Sending ({write_type:?}) {} bytes: {hex}", bytes.len());
    dev.send_as(&bytes, write_type).await?;
    report_notifications(&mut notifications, Duration::from_millis(1000)).await;
    dev.disconnect().await?;
    Ok(())
}
