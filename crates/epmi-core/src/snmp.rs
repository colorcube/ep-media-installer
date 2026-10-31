use std::net::UdpSocket;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{EpmiError, Result};
use crate::media::PrinterMediaSlot;

pub const DEFAULT_PRINTER_HOST: &str = "192.168.1.100";
pub const DEFAULT_SNMP_PORT: u16 = 161;

/// Base OID (`1.3.6.1.4.1.1248.1.2.2.44.1.1.2.1`) in BER encoding.
pub const BASE_OID_BER: [u8; 15] = [
    0x2B, 0x06, 0x01, 0x04, 0x01, 0x89, 0x60, 0x01, 0x02, 0x02, 0x2C, 0x01, 0x01, 0x02, 0x01,
];

/// Compact 9-byte directory entry returned for each of the 30 custom media slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotSummaryEntry {
    pub slot: u8,
    pub base_sn_id: u8,
    pub user_media_id: u32,
    pub media_category: u8,
    pub flag: u8,
    pub registered: bool,
}

fn ber_len(length: usize) -> Vec<u8> {
    if length < 0x80 {
        vec![length as u8]
    } else if length <= 0xFF {
        vec![0x81, length as u8]
    } else {
        vec![0x82, ((length >> 8) & 0xFF) as u8, (length & 0xFF) as u8]
    }
}

fn ber_tlv(tag: u8, val: &[u8]) -> Vec<u8> {
    let len_bytes = ber_len(val.len());
    let mut out = Vec::with_capacity(1 + len_bytes.len() + val.len());
    out.push(tag);
    out.extend_from_slice(&len_bytes);
    out.extend_from_slice(val);
    out
}

fn ber_read_tlv(buf: &[u8], pos: usize) -> Result<(u8, &[u8], usize)> {
    let tag = *buf
        .get(pos)
        .ok_or_else(|| EpmiError::SnmpProtocol("Truncated BER tag".to_string()))?;
    let b0 = *buf
        .get(pos.saturating_add(1))
        .ok_or_else(|| EpmiError::SnmpProtocol("Truncated BER length".to_string()))?;
    let mut cur = pos.saturating_add(2);
    let length = if (b0 & 0x80) == 0 {
        usize::from(b0)
    } else {
        let nbytes = usize::from(b0 & 0x7F);
        if nbytes == 0 || nbytes > 4 {
            return Err(EpmiError::SnmpProtocol(format!(
                "Invalid BER length byte 0x{b0:02x}"
            )));
        }
        let mut len_acc = 0usize;
        for _ in 0..nbytes {
            let b = *buf.get(cur).ok_or_else(|| {
                EpmiError::SnmpProtocol("Truncated multi-byte BER length".to_string())
            })?;
            len_acc = (len_acc << 8) | usize::from(b);
            cur = cur.saturating_add(1);
        }
        len_acc
    };
    let end = cur
        .checked_add(length)
        .ok_or_else(|| EpmiError::SnmpProtocol("BER TLV length overflow".to_string()))?;
    let val = buf
        .get(cur..end)
        .ok_or_else(|| EpmiError::SnmpProtocol("BER TLV value exceeds packet".to_string()))?;
    Ok((tag, val, end))
}

fn encode_oid_subids(raw_bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw_bytes.len() * 2);
    for &b in raw_bytes {
        if b < 0x80 {
            out.push(b);
        } else {
            out.push(0x81);
            out.push(b.wrapping_sub(0x80));
        }
    }
    out
}

pub fn build_snmp_get_raw_oid(cmd_bytes: &[u8], community: &str, req_id: u8) -> Vec<u8> {
    let subids = encode_oid_subids(cmd_bytes);
    let mut oid_bytes = Vec::with_capacity(BASE_OID_BER.len() + subids.len());
    oid_bytes.extend_from_slice(&BASE_OID_BER);
    oid_bytes.extend_from_slice(&subids);

    let oid_tlv = ber_tlv(0x06, &oid_bytes);
    let null_tlv = ber_tlv(0x05, &[]);
    let mut vb_inner = Vec::with_capacity(oid_tlv.len() + null_tlv.len());
    vb_inner.extend_from_slice(&oid_tlv);
    vb_inner.extend_from_slice(&null_tlv);
    let varbind = ber_tlv(0x30, &vb_inner);
    let varbind_list = ber_tlv(0x30, &varbind);

    let req_id_tlv = ber_tlv(0x02, &[req_id & 0x7F]);
    let zero_tlv = ber_tlv(0x02, &[0x00]);
    let mut pdu_inner =
        Vec::with_capacity(req_id_tlv.len() + zero_tlv.len() * 2 + varbind_list.len());
    pdu_inner.extend_from_slice(&req_id_tlv);
    pdu_inner.extend_from_slice(&zero_tlv);
    pdu_inner.extend_from_slice(&zero_tlv);
    pdu_inner.extend_from_slice(&varbind_list);
    let pdu = ber_tlv(0xA0, &pdu_inner);

    let ver_tlv = ber_tlv(0x02, &[0x00]);
    let comm_tlv = ber_tlv(0x04, community.as_bytes());
    let mut msg_inner = Vec::with_capacity(ver_tlv.len() + comm_tlv.len() + pdu.len());
    msg_inner.extend_from_slice(&ver_tlv);
    msg_inner.extend_from_slice(&comm_tlv);
    msg_inner.extend_from_slice(&pdu);
    ber_tlv(0x30, &msg_inner)
}

pub fn parse_snmp_response(resp: &[u8]) -> Result<Vec<u8>> {
    let (_msg_tag, msg_val, _) = ber_read_tlv(resp, 0)?;
    let (_ver_tag, _ver, pos) = ber_read_tlv(msg_val, 0)?;
    let (_comm_tag, _comm, pos) = ber_read_tlv(msg_val, pos)?;
    let (_pdu_tag, pdu_val, _) = ber_read_tlv(msg_val, pos)?;
    let (_req_tag, _req_id, ppos) = ber_read_tlv(pdu_val, 0)?;
    let (_err_tag, err_stat_b, ppos) = ber_read_tlv(pdu_val, ppos)?;
    let (_idx_tag, _err_idx_b, ppos) = ber_read_tlv(pdu_val, ppos)?;

    let err_stat = err_stat_b.first().copied().unwrap_or(0);
    if err_stat != 0 {
        return Err(EpmiError::SnmpProtocol(format!(
            "SNMP GetResponse error-status={err_stat}"
        )));
    }

    let (_vbl_tag, vbl_val, _) = ber_read_tlv(pdu_val, ppos)?;
    let (_vb_tag, vb_val, _) = ber_read_tlv(vbl_val, 0)?;
    let (_oid_tag, _oid_b, vpos) = ber_read_tlv(vb_val, 0)?;
    let (_val_tag, val_b, _) = ber_read_tlv(vb_val, vpos)?;
    Ok(val_b.to_vec())
}

pub fn extract_ex_payload(reply: &[u8]) -> Result<(u8, u8, u8, Vec<u8>)> {
    let idx = reply.windows(3).position(|w| w == b"ex:").ok_or_else(|| {
        EpmiError::SnmpProtocol("Missing 'ex:' marker in printer reply".to_string())
    })?;
    let hdr_start = idx.saturating_add(3);
    let hdr_end = hdr_start.saturating_add(5);
    let hdr = reply
        .get(hdr_start..hdr_end)
        .ok_or_else(|| EpmiError::SnmpProtocol("Truncated 'ex:' 5-byte header".to_string()))?;
    let res_code = hdr[0];
    let m2 = hdr[1];
    let m3 = hdr[2];
    let payload_len = usize::from(u16::from_le_bytes([hdr[3], hdr[4]]));
    let pay_end = hdr_end.saturating_add(payload_len);
    let payload = reply
        .get(hdr_end..pay_end)
        .ok_or_else(|| EpmiError::SnmpProtocol("Truncated 'ex:' payload".to_string()))?;
    Ok((res_code, m2, m3, payload.to_vec()))
}

pub fn decode_slot_tlv(slot: u8, payload: &[u8]) -> PrinterMediaSlot {
    let mut info = PrinterMediaSlot::empty(slot);
    let mut pos = 0usize;
    while pos.saturating_add(2) <= payload.len() {
        let tag = payload[pos];
        let ln = usize::from(payload[pos + 1]);
        let start = pos + 2;
        let end = start.saturating_add(ln);
        let Some(val) = payload.get(start..end) else {
            break;
        };
        pos = end;
        match (tag, val) {
            (0x02, [b]) => info.base_sn_id = *b,
            (0x03, [b]) => info.platen_gap = *b,
            (0x04, [b]) => info.drying_time = *b,
            (0x19, [b0, b1]) => info.thickness_01mm = u16::from_le_bytes([*b0, *b1]),
            (0x22, [b]) => info.paper_width_check = *b,
            (0x23, bytes) => {
                let s = String::from_utf8_lossy(bytes);
                info.media_name = s.trim_end_matches('\0').to_string();
            }
            (0x24, [b0, b1, b2, b3]) => {
                info.user_media_id = u32::from_le_bytes([*b0, *b1, *b2, *b3]);
            }
            (0x25, [b]) => info.media_category = *b,
            (0x26, [b]) => info.registered = *b != 0,
            (0x28, [c0, c1, r0, r1]) => {
                info.cut_sheet_feed_offset = i16::from_le_bytes([*c0, *c1]);
                info.roll_paper_feed_offset = i16::from_le_bytes([*r0, *r1]);
            }
            (0x2B, [b0, b1]) => {
                info.margin_sets = u16::from_le_bytes([*b0, *b1]);
            }
            _ => {}
        }
    }
    info
}

pub fn build_slot_write_cmd(slot: u8, tlv_payload: &[u8]) -> Vec<u8> {
    let total_len = (tlv_payload.len().saturating_add(8)) as u16;
    let [len_l, len_h] = total_len.to_le_bytes();
    let mut cmd = Vec::with_capacity(12 + tlv_payload.len());
    cmd.extend_from_slice(&[
        0x65, 0x78, len_l, len_h, 0x00, 0x00, 0x00, 0x00, 0x1B, 0x00, slot, 0x80,
    ]);
    cmd.extend_from_slice(tlv_payload);
    cmd
}

/// Build the two NVRAM slot write command packets for a custom media slot (`1..=30`).
pub fn encode_slot_write_packets(slot_data: &PrinterMediaSlot) -> Result<(Vec<u8>, Vec<u8>, u32)> {
    if !(1..=30).contains(&slot_data.slot) {
        return Err(EpmiError::InvalidSlot(slot_data.slot));
    }

    let media_id = if slot_data.user_media_id > 0 {
        slot_data.user_media_id
    } else {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(12_345_678);
        910_000_000 + (nanos % 89_999_999)
    };

    let mut name_bytes = slot_data.media_name.as_bytes();
    if name_bytes.len() > 60 {
        name_bytes = &name_bytes[..60];
    }

    let thickness = slot_data.thickness_01mm.clamp(1, 1000);
    let drying = slot_data.drying_time.min(100);
    let width_chk = u8::from(slot_data.paper_width_check != 0);

    let mut tlv1 = Vec::with_capacity(48 + name_bytes.len());
    tlv1.extend_from_slice(&[0x02, 0x01, slot_data.base_sn_id]);
    tlv1.extend_from_slice(&[0x03, 0x01, slot_data.platen_gap]);
    tlv1.extend_from_slice(&[0x04, 0x01, drying]);
    tlv1.extend_from_slice(&[0x19, 0x02]);
    tlv1.extend_from_slice(&thickness.to_le_bytes());
    tlv1.extend_from_slice(&[0x22, 0x01, width_chk]);
    tlv1.extend_from_slice(&[0x23, name_bytes.len() as u8]);
    tlv1.extend_from_slice(name_bytes);
    tlv1.extend_from_slice(&[0x24, 0x04]);
    tlv1.extend_from_slice(&media_id.to_le_bytes());
    tlv1.extend_from_slice(&[0x25, 0x01, slot_data.media_category]);
    tlv1.extend_from_slice(&[0x26, 0x01, 0x01]);
    tlv1.extend_from_slice(&[0x28, 0x04]);
    tlv1.extend_from_slice(&slot_data.cut_sheet_feed_offset.to_le_bytes());
    tlv1.extend_from_slice(&slot_data.roll_paper_feed_offset.to_le_bytes());

    let pkt1 = build_slot_write_cmd(slot_data.slot, &tlv1);

    let mut tlv2 = Vec::with_capacity(4);
    tlv2.extend_from_slice(&[0x2B, 0x02]);
    tlv2.extend_from_slice(&slot_data.margin_sets.to_le_bytes());
    let pkt2 = build_slot_write_cmd(slot_data.slot, &tlv2);

    Ok((pkt1, pkt2, media_id))
}

/// SNMPv1 UDP client for reading, writing, and clearing custom media slots `1..=30`.
#[derive(Debug, Clone)]
pub struct SnmpMediaClient {
    pub host: String,
    pub port: u16,
    pub community: String,
    pub timeout: Duration,
}

impl Default for SnmpMediaClient {
    fn default() -> Self {
        Self {
            host: DEFAULT_PRINTER_HOST.to_string(),
            port: DEFAULT_SNMP_PORT,
            community: "public".to_string(),
            timeout: Duration::from_millis(2500),
        }
    }
}

impl SnmpMediaClient {
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            ..Self::default()
        }
    }

    pub fn send_ex_command(&self, cmd_bytes: &[u8], req_id: u8) -> Result<Vec<u8>> {
        let pkt = build_snmp_get_raw_oid(cmd_bytes, &self.community, req_id);
        let sock = UdpSocket::bind("0.0.0.0:0")?;
        sock.set_read_timeout(Some(self.timeout))?;
        sock.set_write_timeout(Some(self.timeout))?;
        let target = format!("{}:{}", self.host, self.port);
        sock.send_to(&pkt, &target)?;

        let mut buf = vec![0u8; 65535];
        let (n, _) = sock.recv_from(&mut buf)?;
        parse_snmp_response(&buf[..n])
    }

    /// Read the compact 30-slot directory in a single SNMP query.
    pub fn read_slot_directory(&self) -> Result<Vec<SlotSummaryEntry>> {
        let cmd: [u8; 12] = [
            0x65, 0x78, 0x08, 0x00, 0x01, 0x00, 0x00, 0x00, 0x1B, 0x00, 0x00, 0xC0,
        ];
        let reply = self.send_ex_command(&cmd, 1)?;
        let (res_code, _m2, _m3, payload) = extract_ex_payload(&reply)?;
        if res_code != 0 {
            return Err(EpmiError::PrinterExError(res_code));
        }

        let mut entries = Vec::with_capacity(30);
        for chunk in payload.chunks_exact(9) {
            let slot = chunk[0];
            let base_sn_id = chunk[1];
            let user_media_id = u32::from_le_bytes([chunk[2], chunk[3], chunk[4], chunk[5]]);
            let media_category = chunk[6];
            let flag = chunk[7];
            let registered = chunk[8] != 0;
            entries.push(SlotSummaryEntry {
                slot,
                base_sn_id,
                user_media_id,
                media_category,
                flag,
                registered,
            });
        }
        Ok(entries)
    }

    /// Read all TLV attributes of a single custom media slot (`1..=30`).
    pub fn read_slot(&self, slot: u8) -> Result<PrinterMediaSlot> {
        if !(1..=30).contains(&slot) {
            return Err(EpmiError::InvalidSlot(slot));
        }
        let cmd: [u8; 12] = [
            0x65, 0x78, 0x08, 0x00, 0x01, 0x00, 0x00, 0x00, 0x1B, 0x00, slot, 0x00,
        ];
        let reply = self.send_ex_command(&cmd, slot)?;
        let (res_code, _m2, _m3, payload) = extract_ex_payload(&reply)?;
        if res_code != 0 {
            return Err(EpmiError::PrinterExError(res_code));
        }
        Ok(decode_slot_tlv(slot, &payload))
    }

    /// Read all custom media slots (`1..=30`). Queries the compact directory first so only
    /// occupied slots require individual slot reads.
    pub fn read_all_slots(&self, only_registered: bool) -> Result<Vec<PrinterMediaSlot>> {
        let dir = self.read_slot_directory()?;
        let mut slots = Vec::with_capacity(30);
        for entry in dir {
            if only_registered && !entry.registered {
                continue;
            }
            if entry.registered {
                slots.push(self.read_slot(entry.slot)?);
            } else {
                let mut empty = PrinterMediaSlot::empty(entry.slot);
                empty.base_sn_id = entry.base_sn_id;
                empty.user_media_id = entry.user_media_id;
                empty.media_category = entry.media_category;
                slots.push(empty);
            }
        }
        Ok(slots)
    }

    /// Write a custom media configuration into printer NVRAM slot `1..=30` and verify it.
    pub fn write_slot(&self, slot_data: &PrinterMediaSlot) -> Result<PrinterMediaSlot> {
        let (cmd1, cmd2, _media_id) = encode_slot_write_packets(slot_data)?;
        let rep1 = self.send_ex_command(&cmd1, slot_data.slot)?;
        let (res1, _, _, _) = extract_ex_payload(&rep1)?;
        if res1 != 0 {
            return Err(EpmiError::PrinterExError(res1));
        }

        let rep2 = self.send_ex_command(&cmd2, slot_data.slot.saturating_add(30))?;
        let (res2, _, _, _) = extract_ex_payload(&rep2)?;
        if res2 != 0 {
            return Err(EpmiError::PrinterExError(res2));
        }

        self.read_slot(slot_data.slot)
    }

    /// Clear/unregister custom media slot `1..=30` in printer NVRAM (`slot + 0x80`).
    pub fn clear_slot(&self, slot: u8) -> Result<PrinterMediaSlot> {
        if !(1..=30).contains(&slot) {
            return Err(EpmiError::InvalidSlot(slot));
        }
        let cmd: [u8; 12] = [
            0x65,
            0x78,
            0x08,
            0x00,
            0x01,
            0x00,
            0x00,
            0x00,
            0x1B,
            0x00,
            slot.wrapping_add(0x80),
            0x80,
        ];
        let rep = self.send_ex_command(&cmd, slot)?;
        let (res_code, _, _, _) = extract_ex_payload(&rep)?;
        if res_code != 0 {
            return Err(EpmiError::PrinterExError(res_code));
        }
        self.read_slot(slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_sample_slot1_tlv() {
        // Sample NVRAM slot 1 TLV payload
        let raw_payload: [u8; 56] = [
            0x26, 0x01, 0x01, 0x23, 0x16, 0x50, 0x68, 0x6f, 0x74, 0x6f, 0x6c, 0x75, 0x78, 0x20,
            0x53, 0x74, 0x75, 0x64, 0x69, 0x6f, 0x44, 0x55, 0x4f, 0x20, 0x32, 0x35, 0x30, 0x24,
            0x04, 0x17, 0xa4, 0x97, 0x36, 0x02, 0x01, 0x0d, 0x25, 0x01, 0x01, 0x04, 0x01, 0x00,
            0x22, 0x01, 0x01, 0x19, 0x02, 0x1b, 0x00, 0x28, 0x04, 0x00, 0x00, 0x00, 0x00, 0x2b,
        ];
        let mut full = raw_payload.to_vec();
        full.extend_from_slice(&[0x02, 0x00, 0x00]);

        let slot = decode_slot_tlv(1, &full);
        assert_eq!(slot.slot, 1);
        assert!(slot.registered);
        assert_eq!(slot.media_name, "Photolux StudioDUO 250");
        assert_eq!(slot.user_media_id, 915_907_607);
        assert_eq!(slot.base_sn_id, 13);
        assert_eq!(slot.base_media_key(), "PremiumLuster");
        assert_eq!(slot.media_category, 1);
        assert_eq!(slot.thickness_01mm, 27);
        assert_eq!(slot.suggested_driver_key(), "Slot01_Photolux_StudioDUO_250");
    }

    #[test]
    fn test_encode_write_packets_roundtrip() {
        let mut s = PrinterMediaSlot::empty(5);
        s.registered = true;
        s.media_name = "Photolux Fineart Natural Fibre".to_string();
        s.user_media_id = 917_942_035;
        s.base_sn_id = 174;
        s.media_category = 1;
        s.platen_gap = 15;
        s.thickness_01mm = 30;
        let res = encode_slot_write_packets(&s);
        assert!(res.is_ok());
        if let Ok((pkt1, pkt2, mid)) = res {
            assert_eq!(mid, 917_942_035);
            assert_eq!(&pkt1[0..2], b"ex");
            assert_eq!(pkt1[10], 5);
            assert_eq!(pkt1[11], 0x80);
            let decoded = decode_slot_tlv(5, &pkt1[12..]);
            assert_eq!(decoded.media_name, "Photolux Fineart Natural Fibre");
            assert_eq!(decoded.user_media_id, 917_942_035);
            assert_eq!(decoded.base_sn_id, 174);
            assert_eq!(decoded.platen_gap, 15);
            assert_eq!(decoded.thickness_01mm, 30);
            assert_eq!(
                pkt2,
                vec![
                    0x65, 0x78, 0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1b, 0x00, 0x05, 0x80, 0x2b,
                    0x02, 0x00, 0x00
                ]
            );
        }
    }
}
