mod device;
mod protocol;

use std::time::Duration;

use anyhow::{bail, Context, Result};
use btleplug::api::WriteType;
use clap::{Parser, Subcommand, ValueEnum};
use futures_util::StreamExt;

use protocol::{ColorMode, DrawCommand, DrawPoint, ExtendedSettings, LightChannel, ModeCommand, PowerCommand, Settings};

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
    /// Connect and dump every GATT service/characteristic/property/current
    /// value (actively reading anything readable, not just passively
    /// waiting on notifications), plus whatever comes back from sending
    /// the handshake query. Use this if `set`/`raw` report success but
    /// nothing happens on the device.
    Inspect,
    /// Build and send a settings command (DMX address/light/color mode/angle).
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
    /// Draw a single line: blanked jump to the start point, then a lit
    /// line to the end point. The minimal case of the app's vector draw
    /// protocol — see DrawCommand's docs for the unconfirmed 15-byte
    /// header this sends.
    DrawLine {
        #[arg(long, allow_hyphen_values = true)]
        from_x: i16,
        #[arg(long, allow_hyphen_values = true)]
        from_y: i16,
        #[arg(long, allow_hyphen_values = true)]
        to_x: i16,
        #[arg(long, allow_hyphen_values = true)]
        to_y: i16,
        /// Palette color index. Confirmed on hardware: 1=red, 2=green,
        /// 3=blue. 0..=15 range in the default (legacy) encoding; full
        /// 0..=255 range under --cmd-new-type. (0 itself isn't a real
        /// color — it's what the shape-start marker point uses.)
        #[arg(long, default_value_t = 1)]
        color: u8,
        /// Use the alternate "cmdNewType" frame/point encoding (see
        /// DrawCommand's docs) instead of the legacy one tried first.
        #[arg(long)]
        cmd_new_type: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        response: bool,
    },
    /// Draw a circle: a shape-start marker followed by evenly-spaced
    /// outline points, tagged the way a real captured circle from the
    /// official app was (see DrawCommand's docs).
    DrawCircle {
        #[arg(long, allow_hyphen_values = true, default_value_t = 0)]
        center_x: i16,
        #[arg(long, allow_hyphen_values = true, default_value_t = 0)]
        center_y: i16,
        /// Real captured circles from the app were roughly 100..=113.
        #[arg(long, default_value_t = 100)]
        radius: u16,
        /// Outline point count. Real captured circles all used 44.
        #[arg(long, default_value_t = 44, value_parser = clap::value_parser!(u16).range(3..=800))]
        points: u16,
        /// Palette color index. Confirmed on hardware: 1=red, 2=green, 3=blue.
        #[arg(long, default_value_t = 1)]
        color: u8,
        #[arg(long)]
        cmd_new_type: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        response: bool,
    },
    /// Turn the device on or off — a distinct command from `set`, traced
    /// from the app's `onOffChange`. Not yet tried on hardware.
    Power {
        #[arg(value_enum)]
        state: PowerArg,
        /// Use the alternate "cmdNewType" frame encoding (see
        /// PowerCommand's docs) instead of the legacy one tried first.
        #[arg(long)]
        cmd_new_type: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        response: bool,
    },
    /// Set the device's operating mode. Confirmed on hardware: 0=dmx,
    /// 1=random, 2=line, 3=anime, 4=text, 5=ilda, 6=mark, 7=program,
    /// 8=draw, 9..=12=unused. See ModeCommand's docs for details.
    Mode {
        #[arg(value_parser = clap::value_parser!(u8).range(0..=12))]
        cur_mode: u8,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        response: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum PowerArg {
    On,
    Off,
}

#[derive(clap::Args)]
struct SetArgs {
    /// DMX512 address (valArr[0]), 1..=512. Confirmed on hardware: not a
    /// power/brightness control despite the app's naming — the unit's own
    /// screen echoes this back as "DMX mode, address <n>".
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..=512))]
    dmx_address: u16,

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
    /// (0..=255) instead of the legacy 5-byte filler. Also swept end to
    /// end with no visible effect so far — see protocol.rs's module docs.
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
        Command::DrawLine { from_x, from_y, to_x, to_y, color, cmd_new_type, dry_run, response } => {
            run_draw_line(cli.scan_seconds, from_x, from_y, to_x, to_y, color, cmd_new_type, dry_run, response).await
        }
        Command::DrawCircle { center_x, center_y, radius, points, color, cmd_new_type, dry_run, response } => {
            run_draw_circle(cli.scan_seconds, center_x, center_y, radius, points, color, cmd_new_type, dry_run, response).await
        }
        Command::Power { state, cmd_new_type, dry_run, response } => {
            run_power(cli.scan_seconds, matches!(state, PowerArg::On), cmd_new_type, dry_run, response).await
        }
        Command::Mode { cur_mode, dry_run, response } => run_mode(cli.scan_seconds, cur_mode, dry_run, response).await,
    }
}

async fn run_inspect(scan_seconds: u64) -> Result<()> {
    println!("Scanning for {}* devices for {scan_seconds}s...", device::DEVICE_NAME_PREFIX);
    let report = device::inspect_first(scan_seconds).await?;
    println!("Connected to {}. GATT tree:", report.name);
    for service in &report.services {
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
            print!("    char {}  [{}]", c.uuid, props.join(", "));
            match &c.value {
                Some(v) if v.is_empty() => println!("  value: (empty)"),
                Some(v) => {
                    let hex: String = v.iter().map(|b| format!("{b:02X}")).collect();
                    let ascii: String = v.iter().map(|&b| if b.is_ascii_graphic() { b as char } else { '.' }).collect();
                    println!("  value: {hex}  ascii: {ascii}");
                }
                None => println!(),
            }
        }
    }
    println!("Handshake query response ({} notifications, up to 3 retries):", report.notifications.len());
    for (uuid, value) in &report.notifications {
        let hex: String = value.iter().map(|b| format!("{b:02X}")).collect();
        println!("  notify {uuid}: {hex}");
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
        dmx_address: args.dmx_address,
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

    println!("Connected to {}. Sending handshake query (retrying up to 3x, 3s apart)...", dev.name);
    let query_responses = dev.query_with_retries(&mut notifications, write_type).await?;
    print_notifications(&query_responses);

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

fn print_notifications(notifications: &[btleplug::api::ValueNotification]) {
    if notifications.is_empty() {
        println!("  (no notifications)");
        return;
    }
    for n in notifications {
        let hex: String = n.value.iter().map(|b| format!("{b:02X}")).collect();
        println!("  notify {}: {hex}", n.uuid);
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

    println!("Connected to {}. Sending handshake query (retrying up to 3x, 3s apart)...", dev.name);
    let query_responses = dev.query_with_retries(&mut notifications, write_type).await?;
    print_notifications(&query_responses);

    println!("Sending ({write_type:?}) {} bytes: {hex}", bytes.len());
    dev.send_as(&bytes, write_type).await?;
    report_notifications(&mut notifications, Duration::from_millis(1000)).await;
    dev.disconnect().await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_draw_line(
    scan_seconds: u64,
    from_x: i16,
    from_y: i16,
    to_x: i16,
    to_y: i16,
    color: u8,
    cmd_new_type: bool,
    dry_run: bool,
    response: bool,
) -> Result<()> {
    if !cmd_new_type && color > 15 {
        bail!("--color must be 0..=15 in the legacy encoding (it's packed into a nibble); pass --cmd-new-type for the full 0..=255 range");
    }
    let cmd = DrawCommand {
        points: vec![
            DrawPoint { x: from_x, y: from_y, color: 0, tag: 2 }, // shape-start marker
            DrawPoint { x: to_x, y: to_y, color, tag: 3 },        // outline point, also the shape's last
        ],
        new_type: cmd_new_type,
    };
    let hex = cmd.to_hex_string();

    if dry_run {
        println!("{hex}");
        return Ok(());
    }

    println!("Connecting to {}* (up to {scan_seconds}s)...", device::DEVICE_NAME_PREFIX);
    let dev = device::connect_first(scan_seconds).await?;
    let write_type = if response { WriteType::WithResponse } else { WriteType::WithoutResponse };
    dev.subscribe_all_notifications().await?;
    let mut notifications = dev.notifications().await?;

    println!("Connected to {}. Sending handshake query (retrying up to 3x, 3s apart)...", dev.name);
    let query_responses = dev.query_with_retries(&mut notifications, write_type).await?;
    print_notifications(&query_responses);

    println!("Sending draw-line ({write_type:?}): {hex}");
    dev.send_as(&cmd.to_bytes(), write_type).await?;
    report_notifications(&mut notifications, Duration::from_millis(1000)).await;
    dev.disconnect().await?;
    println!("Done.");
    Ok(())
}

/// Build a circle's point list: a shape-start marker at the first outline
/// point, then evenly-spaced outline points around the circumference,
/// with the last point tagged 3 instead of 0 — matching a real captured
/// circle from the official app (see DrawCommand's docs).
fn circle_points(center_x: i16, center_y: i16, radius: u16, count: u16, color: u8) -> Vec<DrawPoint> {
    let point_at = |i: u16| {
        let angle = std::f64::consts::TAU * f64::from(i) / f64::from(count);
        let x = f64::from(center_x) + f64::from(radius) * angle.cos();
        let y = f64::from(center_y) + f64::from(radius) * angle.sin();
        (x.round() as i16, y.round() as i16)
    };
    let (first_x, first_y) = point_at(0);
    let mut points = vec![DrawPoint { x: first_x, y: first_y, color: 0, tag: 2 }];
    for i in 0..count {
        let (x, y) = point_at(i);
        let tag = if i == count - 1 { 3 } else { 0 };
        points.push(DrawPoint { x, y, color, tag });
    }
    points
}

#[allow(clippy::too_many_arguments)]
async fn run_draw_circle(
    scan_seconds: u64,
    center_x: i16,
    center_y: i16,
    radius: u16,
    points: u16,
    color: u8,
    cmd_new_type: bool,
    dry_run: bool,
    response: bool,
) -> Result<()> {
    if !cmd_new_type && color > 15 {
        bail!("--color must be 0..=15 in the legacy encoding (it's packed into a nibble); pass --cmd-new-type for the full 0..=255 range");
    }
    let cmd = DrawCommand {
        points: circle_points(center_x, center_y, radius, points, color),
        new_type: cmd_new_type,
    };
    let hex = cmd.to_hex_string();

    if dry_run {
        println!("{hex}");
        return Ok(());
    }

    println!("Connecting to {}* (up to {scan_seconds}s)...", device::DEVICE_NAME_PREFIX);
    let dev = device::connect_first(scan_seconds).await?;
    let write_type = if response { WriteType::WithResponse } else { WriteType::WithoutResponse };
    dev.subscribe_all_notifications().await?;
    let mut notifications = dev.notifications().await?;

    println!("Connected to {}. Sending handshake query (retrying up to 3x, 3s apart)...", dev.name);
    let query_responses = dev.query_with_retries(&mut notifications, write_type).await?;
    print_notifications(&query_responses);

    println!("Sending draw-circle ({write_type:?}, {points} points): {hex}");
    dev.send_as(&cmd.to_bytes(), write_type).await?;
    report_notifications(&mut notifications, Duration::from_millis(1000)).await;
    dev.disconnect().await?;
    println!("Done.");
    Ok(())
}

async fn run_power(scan_seconds: u64, on: bool, cmd_new_type: bool, dry_run: bool, response: bool) -> Result<()> {
    let cmd = PowerCommand { on, new_type: cmd_new_type };
    let hex = cmd.to_hex_string();

    if dry_run {
        println!("{hex}");
        return Ok(());
    }

    println!("Connecting to {}* (up to {scan_seconds}s)...", device::DEVICE_NAME_PREFIX);
    let dev = device::connect_first(scan_seconds).await?;
    let write_type = if response { WriteType::WithResponse } else { WriteType::WithoutResponse };
    dev.subscribe_all_notifications().await?;
    let mut notifications = dev.notifications().await?;

    println!("Connected to {}. Sending handshake query (retrying up to 3x, 3s apart)...", dev.name);
    let query_responses = dev.query_with_retries(&mut notifications, write_type).await?;
    print_notifications(&query_responses);

    println!("Sending power {} ({write_type:?}): {hex}", if on { "on" } else { "off" });
    dev.send_as(&cmd.to_bytes(), write_type).await?;
    report_notifications(&mut notifications, Duration::from_millis(1000)).await;
    dev.disconnect().await?;
    println!("Done.");
    Ok(())
}

async fn run_mode(scan_seconds: u64, cur_mode: u8, dry_run: bool, response: bool) -> Result<()> {
    let cmd = ModeCommand { cur_mode };
    let hex = cmd.to_hex_string();

    if dry_run {
        println!("{hex}");
        return Ok(());
    }

    println!("Connecting to {}* (up to {scan_seconds}s)...", device::DEVICE_NAME_PREFIX);
    let dev = device::connect_first(scan_seconds).await?;
    let write_type = if response { WriteType::WithResponse } else { WriteType::WithoutResponse };
    dev.subscribe_all_notifications().await?;
    let mut notifications = dev.notifications().await?;

    println!("Connected to {}. Sending handshake query (retrying up to 3x, 3s apart)...", dev.name);
    let query_responses = dev.query_with_retries(&mut notifications, write_type).await?;
    print_notifications(&query_responses);

    println!("Sending mode {cur_mode} ({write_type:?}): {hex}");
    dev.send_as(&cmd.to_bytes(), write_type).await?;
    report_notifications(&mut notifications, Duration::from_millis(1000)).await;
    dev.disconnect().await?;
    println!("Done.");
    Ok(())
}
