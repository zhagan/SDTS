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
//! `dmx_address` (valArr[0], 1..=512) is **not** a power/brightness
//! control — it's a DMX512 address (see [`Settings::dmx_address`] for how
//! that was confirmed: the physical unit's own screen echoed back the
//! value we sent, labeled "DMX mode, address"). That's also why sweeping
//! it end to end (`1` vs `512`) never visibly changed the output — it was
//! never a brightness field to begin with. `extended.brightness` (the
//! separate "cmdNewType" field) was *also* swept end to end (`1` vs `255`,
//! including a dark-room retest) with no visible change — this one now has
//! a plausible explanation too: the unit's own screen reports its laser
//! mode as **TTL**, which drives the diodes as a simple on/off switch with
//! no continuous/PWM dimming. So the lack of visible brightness control
//! may just be correct hardware behavior rather than a wrong or missing
//! field — **however**, retesting the same `1` vs `255` sweep after
//! switching the unit's laser mode from TTL to **AN** (analog) via its own
//! menu still showed no difference, which weakens that explanation: analog
//! drive should be capable of continuous dimming even if TTL isn't. So
//! `extended.brightness` doing nothing remains genuinely unexplained — it
//! may just be the wrong field, or the app's actual brightness command
//! lives somewhere we haven't traced yet. (Screen also showed a software
//! version, "VC2.3", for whatever that's worth later.) The output is also always some
//! animated pattern (e.g. rolling circles) rather than a static dot
//! regardless of these settings — that's the separate "draw" command
//! channel ([`DrawCommand`]), not something these fields affect.
//! `channel`, `xy_angle`'s visual effect, `display_size`'s effect beyond
//! "must be in range", and what (if anything) distinguishes `Two` from
//! `Three` are all still unconfirmed too.

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
    /// valArr[0]. Range 1..=512 in the app — almost certainly a **DMX512
    /// address**, not power/brightness as originally guessed: confirmed on
    /// hardware (TD5322A_V3.1.2BLE) by sending `300` here and reading the
    /// unit's own screen back, which reported "DMX mode, address 300".
    pub dmx_address: u16,
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
        s += &hex_u16(self.dmx_address);
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

/// One point in a draw/vector command, matching the app's point tuple
/// `[x, y, color, tag]` (`getDrawPointStr`/`X` in `utils/funcTools.js`).
#[derive(Debug, Clone, Copy)]
pub struct DrawPoint {
    /// Roughly -400..=400 based on the app editor's bounds check
    /// (`t<-398||i>398`) — not a hard device-enforced limit we've confirmed.
    pub x: i16,
    pub y: i16,
    /// Palette color index, 0..=15 — packed into the upper nibble alongside
    /// `tag` in the legacy (non-cmdNewType) point encoding this uses.
    pub color: u8,
    /// Movement tag, 0..=15. Confirmed meaning from the app's line editor:
    /// `0` ("fixed") = laser on, draw a line from the previous point to
    /// this one. `1` ("move") = laser off, jump to this point without
    /// drawing. Other tag values (`2` = "moves", and 14/15 from masking
    /// the editor's "clear"/"delete" 254/255 codes) are unconfirmed.
    pub tag: u8,
}

/// A draw command: an ordered list of points, each connected to the
/// previous one by either a blanked jump (`tag: 1`) or a drawn line
/// (`tag: 0`). A minimal line is two points: `[move-to start, draw-to end]`.
///
/// **Tried on hardware (TD5322A_V3.1.2BLE, unit switched to its own
/// "draw" mode via the physical menu first): no visible effect from
/// either `new_type: false` or `new_type: true`.** This is now a
/// confirmed dead end from static analysis alone — both plausible frame
/// shapes produced nothing, and this firmware never sends any
/// notification/ack we could use to tell "rejected" from "wrong shape
/// entirely" apart. Further progress here most likely needs a real packet
/// capture of the official app drawing a line (e.g. Android's Bluetooth
/// HCI snoop log), not more guessing from the JS source.
///
/// **The 15-byte header this encodes is an unconfirmed all-zero guess** —
/// though it does match the app's own default `pisObj.cnfValus` seen in
/// several places in the bundle (`[0,0,0,0,0,0,0,0,0,0,0,0,0,0]`), so it's
/// at least a plausible one, not an arbitrary one. The app builds it from
/// `cnfValus`, a per-pattern config array whose contents we've never
/// captured off a live device (no notification response has come back
/// from this firmware for anything we've sent so far) — most likely
/// culprit if the frame shape itself turns out to be right.
#[derive(Debug, Clone)]
pub struct DrawCommand {
    pub points: Vec<DrawPoint>,
    /// Selects which of the app's two frame/point encodings to build —
    /// this is a real fork in `H()`/`X()` based on a `cmdNewType` device
    /// capability flag we have no confirmed read on for this hardware.
    /// `false` (legacy): frame is `F0F1F2F3...F4F5F6F7`, points are 5
    /// bytes (`x,y,color<<4|tag`, color 0..=15). `true`: frame is
    /// `F01F0000...F4F5F6F7`, points are 6 bytes (`x,y,color,tag`, color
    /// gets its own full byte). Both tried on hardware with no visible
    /// result — see the struct docs above.
    pub new_type: bool,
}

/// The app's `m(e,4)` sign-magnitude encoding (not two's complement):
/// negative values set bit 15 and store the magnitude in the low 15 bits.
fn encode_signed16(v: i16) -> u16 {
    if v < 0 {
        0x8000 | ((-(v as i32)) as u16 & 0x7FFF)
    } else {
        v as u16
    }
}

impl DrawCommand {
    /// Encode into the uppercase hex command string, mirroring
    /// `getDrawCmdStr` -> `H(X(...), features, 0)` with no `picsPlay`/
    /// `textStopTime` features set (both unconfirmed for this device, so
    /// taking the plainest path through those two branches regardless of
    /// `new_type`).
    pub fn to_hex_string(&self) -> String {
        let mut body = String::new();
        body.push_str(&"00".repeat(15)); // cnfValus[0..14] header — see struct docs
        body.push_str("00"); // marker byte the app always appends before the point list
        body.push_str(&format!("{:04X}", self.points.len()));

        for p in &self.points {
            body.push_str(&format!("{:04X}", encode_signed16(p.x)));
            body.push_str(&format!("{:04X}", encode_signed16(p.y)));
            if self.new_type {
                body.push_str(&format!("{:02X}", p.color));
                body.push_str(&format!("{:02X}", p.tag & 0x0F));
            } else {
                body.push_str(&format!("{:02X}", (p.color << 4) | (p.tag & 0x0F)));
            }
        }

        if self.new_type {
            format!("F01F0000{body}F4F5F6F7")
        } else {
            format!("F0F1F2F3{body}F4F5F6F7")
        }
    }

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
            dmx_address: 300,
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
    fn dmx_address_encodes_as_big_endian_u16_right_after_the_header() {
        let mut s = base();
        s.dmx_address = 0x1234;
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

    #[test]
    fn draw_frame_uses_the_f0f1_sentinels_with_no_checksum() {
        let cmd = DrawCommand { points: vec![], new_type: false };
        let hex = cmd.to_hex_string();
        assert!(hex.starts_with("F0F1F2F3"));
        assert!(hex.ends_with("F4F5F6F7"));
    }

    #[test]
    fn draw_header_is_15_zero_bytes_plus_marker_before_the_point_count() {
        let cmd = DrawCommand { points: vec![], new_type: false };
        let hex = cmd.to_hex_string();
        // "F0F1F2F3" (8 chars) + 15 zero bytes (30 chars) + marker (2 chars) = 40.
        assert_eq!(&hex[8..40], &"00".repeat(16));
        assert_eq!(&hex[40..44], "0000"); // zero points
    }

    #[test]
    fn draw_point_packs_color_into_upper_nibble_and_tag_into_lower_nibble() {
        let cmd = DrawCommand {
            points: vec![DrawPoint { x: 0, y: 0, color: 2, tag: 1 }],
            new_type: false,
        };
        let hex = cmd.to_hex_string();
        assert_eq!(&hex[40..44], "0001"); // point count
        assert_eq!(&hex[44..54], "0000000021"); // x=0000 y=0000 (2<<4)|1=0x21
    }

    #[test]
    fn draw_point_encodes_negative_coordinates_as_sign_magnitude_not_twos_complement() {
        let cmd = DrawCommand {
            points: vec![DrawPoint { x: -5, y: -300, color: 0, tag: 0 }],
            new_type: false,
        };
        let hex = cmd.to_hex_string();
        // -5 -> 0x8000|5 = 0x8005; -300 -> 0x8000|0x012C = 0x812C.
        assert_eq!(&hex[44..54], "8005812C00");
    }

    #[test]
    fn draw_line_is_a_move_point_followed_by_a_draw_point() {
        let cmd = DrawCommand {
            points: vec![
                DrawPoint { x: 0, y: 0, color: 0, tag: 1 },
                DrawPoint { x: 100, y: 50, color: 2, tag: 0 },
            ],
            new_type: false,
        };
        let hex = cmd.to_hex_string();
        assert_eq!(&hex[40..44], "0002");
        assert_eq!(&hex[44..54], "0000000001"); // move to (0,0)
        assert_eq!(&hex[54..64], "0064003220"); // draw to (100,50), color 2
    }

    #[test]
    fn new_type_frame_uses_the_f01f_sentinel_and_6_byte_points() {
        let cmd = DrawCommand {
            points: vec![DrawPoint { x: 0, y: 0, color: 200, tag: 1 }],
            new_type: true,
        };
        let hex = cmd.to_hex_string();
        assert!(hex.starts_with("F01F0000"));
        assert!(hex.ends_with("F4F5F6F7"));
        // "F01F0000" (8) + 15 zero bytes (30) + marker (2) + count (4) = 44.
        assert_eq!(&hex[44..44 + 12], "00000000C801"); // x=0000 y=0000 color=C8(200) tag=01
    }
}
