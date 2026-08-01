//! BLE transport for the `TD5322A_`-advertised LightElf device.
//!
//! GATT layout (service -> notify char -> write char) reverse-engineered
//! from the official app: it tries a primary service/characteristic set
//! and falls back to a second set seen on some firmware revisions.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use btleplug::api::{CharPropFlags, Central, Manager as _, Peripheral as _, ScanFilter, ValueNotification, WriteType};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures_util::Stream;
use tokio::time;
use uuid::{uuid, Uuid};

pub const DEVICE_NAME_PREFIX: &str = "TD5322A_";

const SERVICE_PRIMARY: Uuid = uuid!("0000ffe0-0000-1000-8000-00805f9b34fb");
const WRITE_PRIMARY: Uuid = uuid!("0000ffe2-0000-1000-8000-00805f9b34fb");

const SERVICE_FALLBACK: Uuid = uuid!("0000ff00-0000-1000-8000-00805f9b34fb");
const WRITE_FALLBACK: Uuid = uuid!("0000ff02-0000-1000-8000-00805f9b34fb");

/// Bytes per BLE write, and the delay between them. Matches the app's
/// `doSendData` chunking (20ms between writes); it doesn't negotiate MTU,
/// so it assumes the classic 20-byte payload limit.
const CHUNK_LEN: usize = 20;
const CHUNK_DELAY: Duration = Duration::from_millis(20);

/// The app's `getQueryCmd(this.randomCheck)` with a real 4-byte random
/// challenge (`genRandomCheck()` in `pages/main/main.js`), confirmed from
/// an actual packet capture of the official app: `E0E1E2E3` + 4 random
/// bytes + `E4E5E6E7`. We'd previously sent this with an *empty* payload
/// (`randomCheck`'s uninitialized default, `[]`, before `genRandomCheck()`
/// ever populates it) — almost certainly why this firmware never
/// responded to us; the real capture shows the device answering this
/// exact shape with a ~483-byte status burst. The device's response
/// embeds a checksum derived from these bytes (see `checkRcvData` in the
/// app), which we don't validate — any 4 random bytes should still get a
/// real response back.
pub fn query_frame() -> [u8; 12] {
    use rand::Rng;
    let mut frame = [0xE0, 0xE1, 0xE2, 0xE3, 0, 0, 0, 0, 0xE4, 0xE5, 0xE6, 0xE7];
    rand::thread_rng().fill(&mut frame[4..8]);
    frame
}

pub struct DiscoveredDevice {
    pub name: String,
    pub peripheral: Peripheral,
}

async fn get_adapter() -> Result<Adapter> {
    let manager = Manager::new().await.context("initializing BLE manager")?;
    let adapters = manager.adapters().await.context("listing BLE adapters")?;
    adapters
        .into_iter()
        .next()
        .context("no BLE adapter found (is Bluetooth on?)")
}

/// Scan for `scan_secs` seconds and return every peripheral whose
/// advertised name starts with [`DEVICE_NAME_PREFIX`].
pub async fn scan(scan_secs: u64) -> Result<Vec<DiscoveredDevice>> {
    let adapter = get_adapter().await?;
    adapter
        .start_scan(ScanFilter::default())
        .await
        .context("starting BLE scan")?;
    time::sleep(Duration::from_secs(scan_secs)).await;

    let mut found = Vec::new();
    for peripheral in adapter.peripherals().await.context("listing peripherals")? {
        let Some(props) = peripheral.properties().await.context("reading peripheral properties")? else {
            continue;
        };
        let Some(name) = props.local_name else {
            continue;
        };
        if name.starts_with(DEVICE_NAME_PREFIX) {
            found.push(DiscoveredDevice { name, peripheral });
        }
    }
    let _ = adapter.stop_scan().await;
    Ok(found)
}

/// Scan until a device is found (or `scan_secs` elapses) and connect to
/// the first match. Returns a connected [`LightElfDevice`] ready to send
/// [`crate::protocol::Settings`] frames to.
pub async fn connect_first(scan_secs: u64) -> Result<LightElfDevice> {
    let devices = scan(scan_secs).await?;
    let device = devices
        .into_iter()
        .next()
        .with_context(|| format!("no {DEVICE_NAME_PREFIX}* device found within {scan_secs}s"))?;
    connect(device).await
}

fn find_write_char(
    characteristics: &std::collections::BTreeSet<btleplug::api::Characteristic>,
    device_name: &str,
) -> Result<btleplug::api::Characteristic> {
    characteristics
        .iter()
        .find(|c| c.service_uuid == SERVICE_PRIMARY && c.uuid == WRITE_PRIMARY)
        .or_else(|| {
            characteristics
                .iter()
                .find(|c| c.service_uuid == SERVICE_FALLBACK && c.uuid == WRITE_FALLBACK)
        })
        .cloned()
        .with_context(|| {
            format!(
                "device {device_name} exposes neither the primary ({SERVICE_PRIMARY}/{WRITE_PRIMARY}) \
                 nor fallback ({SERVICE_FALLBACK}/{WRITE_FALLBACK}) write characteristic"
            )
        })
}

pub async fn connect(device: DiscoveredDevice) -> Result<LightElfDevice> {
    let peripheral = device.peripheral;
    peripheral.connect().await.context("BLE connect failed")?;
    peripheral
        .discover_services()
        .await
        .context("GATT service discovery failed")?;

    let write_char = find_write_char(&peripheral.characteristics(), &device.name)?;

    Ok(LightElfDevice {
        name: device.name,
        peripheral,
        write_char,
    })
}

/// Connect to the first matching device and dump its full GATT tree
/// (every service/characteristic/property/current-value), plus anything
/// that comes back from a handshake query. Use this when `set`/`raw`
/// connect fine but nothing happens on the device.
///
/// Beyond the plain GATT dump, this does two things `set`/`raw` don't:
/// sends [`query_frame`] and passively waits for notifications (same as
/// they do), *and* actively **reads** every characteristic that advertises
/// the `read` property. The app does exactly that read right after
/// subscribing to notifications on connect — worth trying since this
/// firmware has never sent us a single notification, and a GATT read gets
/// a characteristic's current value directly without needing the device to
/// proactively push anything.
pub async fn inspect_first(scan_secs: u64) -> Result<InspectReport> {
    let devices = scan(scan_secs).await?;
    let device = devices
        .into_iter()
        .next()
        .with_context(|| format!("no {DEVICE_NAME_PREFIX}* device found within {scan_secs}s"))?;
    let name = device.name.clone();
    let peripheral = device.peripheral;
    peripheral.connect().await.context("BLE connect failed")?;
    peripheral
        .discover_services()
        .await
        .context("GATT service discovery failed")?;

    let characteristics = peripheral.characteristics();
    for c in &characteristics {
        if c.properties.intersects(CharPropFlags::NOTIFY | CharPropFlags::INDICATE) {
            let _ = peripheral.subscribe(c).await;
        }
    }

    let mut notifications = Vec::new();
    if let Ok(write_char) = find_write_char(&characteristics, &name) {
        if let Ok(mut stream) = peripheral.notifications().await {
            // Real capture evidence: the device only answered the *second*
            // query attempt, sent 3s after the first (matching the app's
            // own goQueryCmd retry timer) — a single short wait isn't
            // enough, so retry up to 3 times like the app does.
            for _attempt in 0..3 {
                let _ = peripheral.write(&write_char, &query_frame(), WriteType::WithoutResponse).await;
                let deadline = tokio::time::Instant::now() + Duration::from_millis(3000);
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep_until(deadline) => break,
                        next = futures_util::StreamExt::next(&mut stream) => match next {
                            Some(n) => notifications.push((n.uuid, n.value)),
                            None => break,
                        },
                    }
                }
                if !notifications.is_empty() {
                    break;
                }
            }
        }
    }

    let mut by_service: std::collections::BTreeMap<Uuid, Vec<CharReport>> = std::collections::BTreeMap::new();
    for c in &characteristics {
        let value = if c.properties.contains(CharPropFlags::READ) {
            peripheral.read(c).await.ok()
        } else {
            None
        };
        by_service.entry(c.service_uuid).or_default().push(CharReport {
            uuid: c.uuid,
            properties: c.properties,
            value,
        });
    }
    let _ = peripheral.disconnect().await;

    let services = by_service
        .into_iter()
        .map(|(service_uuid, mut chars)| {
            chars.sort_by_key(|c| c.uuid);
            ServiceReport { service_uuid, chars }
        })
        .collect();
    Ok(InspectReport { name, services, notifications })
}

pub struct InspectReport {
    pub name: String,
    pub services: Vec<ServiceReport>,
    pub notifications: Vec<(Uuid, Vec<u8>)>,
}

pub struct ServiceReport {
    pub service_uuid: Uuid,
    pub chars: Vec<CharReport>,
}

pub struct CharReport {
    pub uuid: Uuid,
    pub properties: CharPropFlags,
    pub value: Option<Vec<u8>>,
}

pub struct LightElfDevice {
    pub name: String,
    peripheral: Peripheral,
    write_char: btleplug::api::Characteristic,
}

impl LightElfDevice {
    /// Send raw command bytes, chunked the way the app chunks its writes.
    /// The real device may only honor one ATT write type — see [`inspect_first`].
    pub async fn send_as(&self, payload: &[u8], write_type: WriteType) -> Result<()> {
        if payload.is_empty() {
            bail!("refusing to send an empty payload");
        }
        for (i, chunk) in payload.chunks(CHUNK_LEN).enumerate() {
            if i > 0 {
                time::sleep(CHUNK_DELAY).await;
            }
            self.peripheral
                .write(&self.write_char, chunk, write_type)
                .await
                .with_context(|| format!("BLE write failed on chunk {i}"))?;
        }
        Ok(())
    }

    pub async fn disconnect(&self) -> Result<()> {
        self.peripheral.disconnect().await.context("BLE disconnect failed")
    }

    /// Subscribe to every characteristic that advertises notify/indicate
    /// (on this device: `0000ff01` and `0000ff02`). Call before
    /// [`Self::notifications`] so nothing sent right after connecting is missed.
    pub async fn subscribe_all_notifications(&self) -> Result<()> {
        for c in self.peripheral.characteristics() {
            if c.properties.intersects(CharPropFlags::NOTIFY | CharPropFlags::INDICATE) {
                self.peripheral
                    .subscribe(&c)
                    .await
                    .with_context(|| format!("subscribing to notifications on {}", c.uuid))?;
            }
        }
        Ok(())
    }

    /// Stream of every notification/indication from any subscribed
    /// characteristic on this peripheral (btleplug multiplexes them all
    /// through one stream, tagged with `.uuid`).
    pub async fn notifications(&self) -> Result<impl Stream<Item = ValueNotification> + '_> {
        self.peripheral.notifications().await.context("opening notification stream")
    }

    /// Send [`query_frame`], retrying up to 3 times with a 3s wait each
    /// time, matching the app's own `goQueryCmd` retry timer — real
    /// capture evidence shows the device only answering the *second*
    /// attempt, sent 3s after the first, so a single short wait misses it.
    /// Stops early once any notification arrives. Call
    /// [`Self::subscribe_all_notifications`] and get a stream from
    /// [`Self::notifications`] first, or this will see nothing.
    pub async fn query_with_retries(
        &self,
        stream: &mut (impl Stream<Item = ValueNotification> + Unpin),
        write_type: WriteType,
    ) -> Result<Vec<ValueNotification>> {
        let mut received = Vec::new();
        for _attempt in 0..3 {
            self.send_as(&query_frame(), write_type).await?;
            let deadline = tokio::time::Instant::now() + Duration::from_millis(3000);
            loop {
                tokio::select! {
                    _ = tokio::time::sleep_until(deadline) => break,
                    next = futures_util::StreamExt::next(stream) => match next {
                        Some(n) => received.push(n),
                        None => break,
                    },
                }
            }
            if !received.is_empty() {
                break;
            }
        }
        Ok(received)
    }
}
