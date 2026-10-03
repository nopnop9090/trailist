//! The buffer `Shell_NotifyIcon` hands the taskbar on this build.
//!
//! A live capture on Windows 11 25H2 (build 26200) shows `WM_COPYDATA` with
//! `dwData == 1`, the signature `0x34753423`, the `NIM_*` operation, and then
//! a `NOTIFYICONDATAW` laid out with 32-bit handles. The structure's own
//! `cbSize` is 956. A 64-bit `NOTIFYICONDATAW` does not line up with those
//! bytes: the tooltip "TrayListProbeIcon" starts 24 bytes into the structure,
//! which is the 32-bit offset of `szTip`.
//!
//! `Shell_NotifyIconGetRect` is a different copy (`dwData == 3`) carrying the
//! same signature, a small command, and a 32-bit `NOTIFYICONIDENTIFIER`.

/// `dwData` for `Shell_NotifyIcon`.
pub const COPYDATA_NOTIFY: usize = 1;
/// `dwData` for `Shell_NotifyIconGetRect`.
pub const COPYDATA_RECT: usize = 3;
/// `TRAYNOTIFYDATAW.dwSignature`.
pub const SIGNATURE: u32 = 0x34753423;

pub const NIM_ADD: u32 = 0;
pub const NIM_MODIFY: u32 = 1;
pub const NIM_DELETE: u32 = 2;
pub const NIM_SETFOCUS: u32 = 3;
pub const NIM_SETVERSION: u32 = 4;

pub const NIF_MESSAGE: u32 = 0x0000_0001;
pub const NIF_ICON: u32 = 0x0000_0002;
pub const NIF_TIP: u32 = 0x0000_0004;
pub const NIF_STATE: u32 = 0x0000_0008;
pub const NIF_GUID: u32 = 0x0000_0020;

pub const NIS_HIDDEN: u32 = 0x0000_0001;
pub const NIS_SHAREDICON: u32 = 0x0000_0002;

/// `NOTIFYICON_VERSION_4`.
pub const VERSION_4: u32 = 4;

const OFF_HWND: usize = 4;
const OFF_ID: usize = 8;
const OFF_FLAGS: usize = 12;
const OFF_CALLBACK: usize = 16;
const OFF_ICON: usize = 20;
const OFF_TIP: usize = 24;
const TIP_CHARS: usize = 128;
const OFF_STATE: usize = 280;
const OFF_VERSION: usize = 800;
const OFF_GUID: usize = 936;

/// One operation, already checked against the signature.
#[derive(Debug, Clone)]
pub struct NotifyOp {
    pub message: u32,
    pub hwnd: isize,
    pub id: u32,
    pub flags: u32,
    pub callback: u32,
    pub icon: isize,
    pub tip: String,
    pub hidden: bool,
    pub shared_icon: bool,
    pub version: Option<u32>,
    pub guid: Option<[u8; 16]>,
}

/// A `Shell_NotifyIconGetRect` request. The rectangle itself is not in this
/// buffer; the taskbar returns it from the window procedure.
#[derive(Debug, Clone)]
pub struct RectQuery {
    pub command: u32,
    pub hwnd: isize,
    pub id: u32,
    pub guid: Option<[u8; 16]>,
}

/// Parses a notify-icon `WM_COPYDATA` payload. `None` means this buffer is
/// some other taskbar request (app bars use `dwData == 0`).
pub fn parse(dw_data: usize, bytes: &[u8]) -> Option<NotifyOp> {
    if dw_data != COPYDATA_NOTIFY || bytes.len() < 8 {
        return None;
    }
    let signature = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
    if signature != SIGNATURE {
        return None;
    }
    let message = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    let nid = &bytes[8..];
    let declared = read_u32(nid, 0)? as usize;
    let available = nid.len().min(declared);
    if available < OFF_CALLBACK + 4 {
        return None;
    }

    let hwnd = widen(read_u32(nid, OFF_HWND)?);
    let id = read_u32(nid, OFF_ID)?;
    let flags = read_u32(nid, OFF_FLAGS).unwrap_or(0);
    let callback = read_u32(nid, OFF_CALLBACK).unwrap_or(0);
    let icon = read_u32(nid, OFF_ICON).map(widen).unwrap_or(0);
    let tip = if flags & NIF_TIP != 0 {
        read_tip(nid, available)
    } else {
        String::new()
    };
    let state = read_u32(nid, OFF_STATE).unwrap_or(0);
    let version = read_u32(nid, OFF_VERSION);
    let guid = if flags & NIF_GUID != 0 {
        read_guid(nid, available)
    } else {
        None
    };

    Some(NotifyOp {
        message,
        hwnd,
        id,
        flags,
        callback,
        icon,
        tip,
        hidden: flags & NIF_STATE != 0 && state & NIS_HIDDEN != 0,
        shared_icon: flags & NIF_STATE != 0 && state & NIS_SHAREDICON != 0,
        version,
        guid,
    })
}

/// Parses a get-rect copy. `None` when the buffer is not that request.
pub fn parse_rect(dw_data: usize, bytes: &[u8]) -> Option<RectQuery> {
    if dw_data != COPYDATA_RECT || bytes.len() < 40 {
        return None;
    }
    let signature = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
    if signature != SIGNATURE {
        return None;
    }
    let guid = {
        let mut raw = [0u8; 16];
        raw.copy_from_slice(&bytes[24..40]);
        if raw == [0u8; 16] {
            None
        } else {
            Some(raw)
        }
    };
    Some(RectQuery {
        command: u32::from_le_bytes(bytes[4..8].try_into().ok()?),
        hwnd: widen(u32::from_le_bytes(bytes[16..20].try_into().ok()?)),
        id: u32::from_le_bytes(bytes[20..24].try_into().ok()?),
        guid,
    })
}

/// USER handles travel as 32-bit values in this buffer. Sign-extending makes
/// a handle the 64-bit window manager will recognise.
pub fn widen(value: u32) -> isize {
    value as i32 as isize
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    if offset + 4 > bytes.len() {
        return None;
    }
    Some(u32::from_le_bytes(bytes[offset..offset + 4].try_into().ok()?))
}

fn read_tip(bytes: &[u8], available: usize) -> String {
    if OFF_TIP + 2 > bytes.len() {
        return String::new();
    }
    let end = (OFF_TIP + TIP_CHARS * 2).min(available).min(bytes.len());
    let mut units = Vec::new();
    for pair in bytes[OFF_TIP..end].chunks_exact(2) {
        let unit = u16::from_le_bytes([pair[0], pair[1]]);
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    String::from_utf16_lossy(&units)
}

fn read_guid(bytes: &[u8], available: usize) -> Option<[u8; 16]> {
    if OFF_GUID + 16 > available || OFF_GUID + 16 > bytes.len() {
        return None;
    }
    let mut guid = [0u8; 16];
    guid.copy_from_slice(&bytes[OFF_GUID..OFF_GUID + 16]);
    if guid == [0u8; 16] {
        None
    } else {
        Some(guid)
    }
}

/// Stable identity. A GUID wins over the window and id, matching the lookup
/// rule in `NOTIFYICONDATA`.
pub fn identity(op: &NotifyOp) -> String {
    if let Some(guid) = op.guid {
        format!("guid:{}", guid_text(&guid))
    } else {
        format!("hwnd:{}:id:{}", op.hwnd, op.id)
    }
}

pub fn guid_text(guid: &[u8; 16]) -> String {
    format!(
        "{{{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        guid[3], guid[2], guid[1], guid[0], guid[5], guid[4], guid[7], guid[6], guid[8], guid[9],
        guid[10], guid[11], guid[12], guid[13], guid[14], guid[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u32(buf: &mut [u8], offset: usize, value: u32) {
        buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn sample() -> Vec<u8> {
        let mut nid = vec![0u8; 956];
        put_u32(&mut nid, 0, 956);
        put_u32(&mut nid, OFF_HWND, 0xF2342);
        put_u32(&mut nid, OFF_ID, 1);
        put_u32(
            &mut nid,
            OFF_FLAGS,
            NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_GUID | NIF_STATE,
        );
        put_u32(&mut nid, OFF_CALLBACK, 0x8000);
        put_u32(&mut nid, OFF_ICON, 0x1002B);
        put_u32(&mut nid, OFF_STATE, NIS_HIDDEN);
        put_u32(&mut nid, OFF_VERSION, VERSION_4);
        let tip: Vec<u16> = "Probe tip".encode_utf16().collect();
        for (index, unit) in tip.iter().enumerate() {
            let at = OFF_TIP + index * 2;
            nid[at..at + 2].copy_from_slice(&unit.to_le_bytes());
        }
        let guid = [
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE,
            0xFF, 0x01,
        ];
        nid[OFF_GUID..OFF_GUID + 16].copy_from_slice(&guid);

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&SIGNATURE.to_le_bytes());
        bytes.extend_from_slice(&NIM_ADD.to_le_bytes());
        bytes.extend_from_slice(&nid);
        bytes
    }

    #[test]
    fn reads_the_32_bit_wire_layout() {
        let op = parse(COPYDATA_NOTIFY, &sample()).expect("signature should parse");
        assert_eq!(op.message, NIM_ADD);
        assert_eq!(op.hwnd, 0xF2342);
        assert_eq!(op.id, 1);
        assert_eq!(op.callback, 0x8000);
        assert_eq!(op.icon, 0x1002B);
        assert_eq!(op.tip, "Probe tip");
        assert!(op.hidden);
        assert_eq!(op.version, Some(VERSION_4));
        assert!(identity(&op).starts_with("guid:{44332211-6655-8877-99AA-BBCCDDEEFF01}"));
    }

    #[test]
    fn ignores_other_copydata() {
        assert!(parse(0, &[0; 32]).is_none());
        assert!(parse(COPYDATA_NOTIFY, &[0; 32]).is_none());
    }

    #[test]
    fn reads_a_rect_query() {
        let mut bytes = vec![0u8; 40];
        bytes[0..4].copy_from_slice(&SIGNATURE.to_le_bytes());
        put_u32(&mut bytes, 4, 1);
        put_u32(&mut bytes, 8, 32);
        put_u32(&mut bytes, 16, 0xF2342);
        put_u32(&mut bytes, 20, 1);
        let query = parse_rect(COPYDATA_RECT, &bytes).expect("rect query");
        assert_eq!(query.command, 1);
        assert_eq!(query.hwnd, 0xF2342);
        assert_eq!(query.id, 1);
    }
}
