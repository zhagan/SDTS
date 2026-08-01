//! Wire protocol for the LightElf-branded `TD5322A_` BLE laser device.
//!
//! Reverse-engineered statically from the official "Light Elf"
//! (com.canwin.lightelf) Android app — specifically `getSettingCmd` (`k` in
//! the minified bundle) in the app's uni-app JS bundle
//! (`assets/apps/__UNI__2C82991/www/app-service.js`, `utils/funcTools.js`
//! source). The frame layout and field widths come directly from that
//! function. The *semantic* meaning of some fields (`valArr[2..4]`, the
//! new-protocol brightness/grating/password block, and exactly what
//! `light`/`channel` select on the physical unit) is inferred from
//! variable names and UI bindings and has **not** been confirmed against a
//! live device beyond what's noted below. Verify with `--dry-run` against a
//! packet sniff before trusting field semantics you haven't tested yourself.
//!
//! Live device notes (TD5322A_V3.1.2BLE, tested 2026-08-01): the handshake
//! `QUERY_FRAME` never gets a response, and neither does the settings
//! frame — this firmware appears to send no acks at all, so success can
//! only be judged visually (hence sending `QUERY_FRAME` first anyway: the
//! very first write right after connecting seems to get silently dropped
//! while the link settles, so a disposable frame is sent ahead of every
//! real command as a primer).
//!
//! `light` selects between two distinct diode heads, confirmed via
//! `mode: Dual` channel isolation — see [`LightChannel`] for what each
//! produces. `Single` mode (which force-sets `dual_channels` to max)
//! confirms the same split at the extremes: `light: One` → white (R+G+B
//! all maxed), `light: Two`/`Three` → pink/magenta (R+B maxed, no G on that
//! head).
//!
//! `power` (valArr[0], 1..=512) and `extended.brightness` (the separate
//! "cmdNewType" field) were both swept end to end (`1` vs `512`, `1` vs
//! `255`) with no visible difference either time — neither is confirmed to
//! do anything. This might be a real no-op, or might just be imperceptible
//! by eye at these power levels/indoor lighting; wasn't re-tested in a dark
//! room or through a camera. `channel`, `xy_angle`'s visual effect,
//! `display_size`'s effect beyond "must be in range", and what (if
//! anything) distinguishes `Two` from `Three` are all still unconfirmed too.

/// Which of the app's three "light1/2/3" buttons to select — picks between
/// two distinct diode heads on this hardware (TD5322A_V3.1.2BLE):
///
/// - `One`: full RGB head. In `Dual` mode, `dual_channels[0/1/2]`
///   (red/green/blue) each independently confirmed working here.
/// - `Two` / `Three`: red+blue-only head. `dual_channels[1]` (green) is a
///   confirmed no-op on this head; red and blue confirmed working. `Two`
///   and `Three` look identical to each other in every test so far —
///   whatever distinguishes them hasn't been found yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightChannel {
    One = 1,
    Two = 2,
    Three = 3,
}

/// Single vs. dual color output ("cfg1"/"cfg2" in the app). Single-color
/// mode forces the app's `valArr[2..4]` bytes to `0xFF` (unused channels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Single,
    Dual,
}

/// Extended fields the app only sends when the connected device reports
/// the "cmdNewType" capability during its handshake. Leave `Settings.extended`
/// as `None` unless you've confirmed your unit needs this.
#[derive(Debug, Clone)]
pub struct ExtendedSettings {
    pub brightness: u8,
    pub grating: u8,
    pub device_password_status: u8,
    pub device_password: u16,
}

/// One full "settings" command frame, matching the app's `settingData` object.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Power/intensity level. App-observed range: 1..=512.
    pub power: u16,
    /// Raw `ch` field — meaning unconfirmed, defaults to 0 in the app for
    /// a single connected device.
    pub channel: u8,
    /// Beam/pattern angle preset, 0..=7 in the app (`setXyAng`, 8 presets).
    pub xy_angle: u8,
    pub light: LightChannel,
    pub color_mode: ColorMode,
    /// `valArr[1]` ("displaySize" in the app). App's device-type validation
    /// table gives this a range of 10..=100 — we'd been hardcoding it to 0
    /// (out of range) until this field existed. Untested whether the
    /// out-of-range value mattered; worth ruling out.
    pub display_size: u8,
    /// `valArr[2..4]` = `[red, green, blue]`, each 0..=100. **Only takes
    /// effect in `ColorMode::Dual`** — the app hard-overrides these to
    /// `0xFF` in `Single` mode regardless of what's stored here, and so does
    /// this encoder.
    ///
    /// Confirmed on hardware (TD5322A_V3.1.2BLE, `light: Two`): `[100,0,0]`
    /// → red, `[0,0,100]` → blue, `[0,100,0]` → **no visible output**. That
    /// unit is almost certainly a red+blue-only laser with no green diode —
    /// the green index exists in the protocol but there's nothing on this
    /// hardware for it to drive. `[0,0,0]` → nothing (all channels off).
    pub dual_channels: [u8; 3],
    pub extended: Option<ExtendedSettings>,
}

const FRAME_HEADER: &str = "00010203";
const FRAME_TRAILER: &str = "04050607";

fn hex_u8(v: u8) -> String {
    format!("{v:02X}")
}

fn hex_u16(v: u16) -> String {
    format!("{v:04X}")
}

impl Settings {
    /// Encode into the uppercase hex command string the app builds.
    pub fn to_hex_string(&self) -> String {
        // Single mode force-overrides valArr[2..4] to 0xFF no matter what's
        // passed in (matches the app's `0==e.cfg&&(c="FF",h="FF",l="FF")`);
        // Dual mode sends the actual per-channel values.
        let (v2, v3, v4) = match self.color_mode {
            ColorMode::Single => (0xFFu8, 0xFFu8, 0xFFu8),
            ColorMode::Dual => (self.dual_channels[0], self.dual_channels[1], self.dual_channels[2]),
        };
        let cfg: u8 = match self.color_mode {
            ColorMode::Single => 0x00,
            ColorMode::Dual => 0xFF,
        };

        let mut s = String::from(FRAME_HEADER);
        s += &hex_u16(self.power);
        s += &hex_u8(self.channel);
        s += &hex_u8(self.display_size);
        s += &hex_u8(self.xy_angle);
        s += &hex_u8(v2);
        s += &hex_u8(v3);
        s += &hex_u8(v4);
        s += &hex_u8(self.light as u8);
        s += &hex_u8(cfg);
        s += &hex_u8(0); // reserved
        match &self.extended {
            None => s += "0000000000",
            Some(ext) => {
                s += &hex_u8(ext.brightness);
                s += &hex_u8(ext.grating);
                s += &hex_u8(ext.device_password_status);
                s += &hex_u16(ext.device_password);
            }
        }
        s += FRAME_TRAILER;
        s
    }

    /// Raw bytes to write to the BLE write characteristic. The app builds
    /// the hex string above and then converts it to a binary buffer before
    /// `writeBLECharacteristicValue` — this does the same conversion.
    pub fn to_bytes(&self) -> Vec<u8> {
        hex_string_to_bytes(&self.to_hex_string())
    }
}

fn hex_string_to_bytes(hex: &str) -> Vec<u8> {
    debug_assert!(hex.len() % 2 == 0);
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("valid hex produced by this module"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Settings {
        Settings {
            power: 300,
            channel: 0,
            xy_angle: 0,
            light: LightChannel::One,
            color_mode: ColorMode::Single,
            display_size: 100,
            dual_channels: [0, 0, 0],
            extended: None,
        }
    }

    #[test]
    fn dual_mode_sends_the_actual_channel_values() {
        let mut s = base();
        s.color_mode = ColorMode::Dual;
        s.dual_channels = [10, 20, 30];
        let hex = s.to_hex_string();
        assert_eq!(&hex[18..24], "0A141E");
    }

    #[test]
    fn legacy_frame_has_expected_length_and_sentinels() {
        let hex = base().to_hex_string();
        assert!(hex.starts_with(FRAME_HEADER));
        assert!(hex.ends_with(FRAME_TRAILER));
        assert_eq!(hex.len(), 48); // 24 bytes
    }

    #[test]
    fn single_color_forces_unused_channels_to_ff() {
        let hex = base().to_hex_string();
        // valArr[2..4] occupy hex chars 18..24 (bytes 9..12 of the frame).
        assert_eq!(&hex[18..24], "FFFFFF");
    }

    #[test]
    fn power_encodes_as_big_endian_u16_right_after_the_header() {
        let mut s = base();
        s.power = 0x1234;
        let hex = s.to_hex_string();
        assert_eq!(&hex[8..12], "1234");
    }

    #[test]
    fn extended_settings_replace_the_legacy_filler() {
        let mut s = base();
        s.extended = Some(ExtendedSettings {
            brightness: 0x64,
            grating: 0x01,
            device_password_status: 0x00,
            device_password: 0x0000,
        });
        let hex = s.to_hex_string();
        assert_eq!(&hex[30..40], "6401000000");
    }

    #[test]
    fn to_bytes_matches_hex_string() {
        let s = base();
        assert_eq!(s.to_bytes().len(), s.to_hex_string().len() / 2);
        assert_eq!(s.to_bytes()[0], 0x00);
        assert_eq!(s.to_bytes()[1], 0x01);
    }
}
