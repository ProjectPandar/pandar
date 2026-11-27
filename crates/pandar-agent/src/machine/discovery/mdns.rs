//! mDNS discovery of Moonraker printers. Stock Moonraker advertises
//! `_moonraker._tcp.local.` with a PTR record per instance, an SRV record
//! carrying the API port, and A records for the host; Snapmaker's U1 firmware
//! advertises `_snapmaker._tcp.local.` instead, with the printer name and
//! model in TXT records and an SRV port that belongs to its MQTT broker.

use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    ops::Range,
    time::Duration,
};

use anyhow::Context;
use tokio::{net::UdpSocket, time::Instant};

use super::{DiscoveredPrinter, scan::local_discovery_targets};
use crate::machine::moonraker::KLIPPER_MODEL;

const MDNS_PORT: u16 = 5353;
const MDNS_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);

/// The mDNS multicast group plus every peer on the agent's local subnets.
pub(super) fn mdns_targets() -> anyhow::Result<Vec<SocketAddr>> {
    let peers = local_discovery_targets()?
        .into_iter()
        .map(|addr| SocketAddr::new(addr.ip(), MDNS_PORT));
    Ok(
        std::iter::once(SocketAddr::new(IpAddr::V4(MDNS_GROUP), MDNS_PORT))
            .chain(peers)
            .collect(),
    )
}

pub(super) const MOONRAKER_SERVICE: &str = "_moonraker._tcp.local";
pub(super) const SNAPMAKER_SERVICE: &str = "_snapmaker._tcp.local";
const SERVICES: &[&str] = &[MOONRAKER_SERVICE, SNAPMAKER_SERVICE];
const TYPE_A: u16 = 1;
const TYPE_PTR: u16 = 12;
const TYPE_TXT: u16 = 16;
const TYPE_SRV: u16 = 33;
const MAX_PACKET_BYTES: usize = 9000;

/// Queries for Moonraker and Snapmaker instances and collects responses until
/// `timeout` elapses. The query asks for unicast responses, so the socket
/// needs no multicast membership; besides the multicast group, every local
/// subnet peer is asked directly because many networks do not deliver multicast
/// from an agent host.
pub(super) async fn discover_mdns_printers(
    timeout: Duration,
    targets: impl IntoIterator<Item = SocketAddr>,
) -> anyhow::Result<Vec<DiscoveredPrinter>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .await
        .context("bind mDNS discovery UDP socket")?;
    let query = service_query();
    for target in targets {
        socket
            .send_to(&query, target)
            .await
            .with_context(|| format!("send mDNS query for {SERVICES:?} to {target}"))?;
    }

    let deadline = Instant::now() + timeout;
    let mut buf = vec![0_u8; MAX_PACKET_BYTES];
    let mut instances: BTreeMap<String, ServiceInstance> = BTreeMap::new();
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((len, source))) => {
                let source = match source.ip() {
                    IpAddr::V4(address) => address,
                    IpAddr::V6(_) => continue,
                };
                if let Err(error) = apply_response(&buf[..len], source, &mut instances) {
                    tracing::debug!(%source, error = %format!("{error:#}"), "ignoring mDNS packet");
                }
            }
            Ok(Err(err)) => return Err(err).context("receive mDNS response"),
            Err(_) => break,
        }
    }

    Ok(instances
        .into_values()
        .filter_map(|service| DiscoveredPrinter::from_service(&service))
        .collect())
}

/// One-question-per-service PTR query. The questions' class sets the
/// unicast-response (QU) bit per RFC 6762 and the message id is zero.
fn service_query() -> Vec<u8> {
    let mut packet = Vec::with_capacity(12 + 64);
    packet.extend_from_slice(&[0, 0, 0, 0, 0, SERVICES.len() as u8, 0, 0, 0, 0, 0, 0]);
    for service in SERVICES {
        for label in service.split('.') {
            packet.push(label.len() as u8);
            packet.extend_from_slice(label.as_bytes());
        }
        packet.push(0);
        packet.extend_from_slice(&TYPE_PTR.to_be_bytes());
        packet.extend_from_slice(&0x8001_u16.to_be_bytes());
    }
    packet
}

#[derive(Debug)]
struct ServiceInstance {
    /// Instance name as advertised, e.g. `Moonraker-1a2b3c4d @ U1._moonraker._tcp.local`.
    instance: String,
    port: Option<u16>,
    target: Option<String>,
    address: Option<Ipv4Addr>,
    txt: BTreeMap<String, String>,
}

impl ServiceInstance {
    fn advertised(instance: &str) -> Self {
        Self {
            instance: instance.to_owned(),
            port: None,
            target: None,
            address: None,
            txt: BTreeMap::new(),
        }
    }
}

impl DiscoveredPrinter {
    fn from_service(service: &ServiceInstance) -> Option<Self> {
        let address = service.address?;
        let txt = |key: &str| service.txt.get(key).map(String::as_str);
        let (name, model, host) = if service
            .instance
            .to_ascii_lowercase()
            .ends_with(&format!(".{SNAPMAKER_SERVICE}"))
        {
            // The SRV port is Snapmaker's MQTT broker; Moonraker is reached on
            // the nginx port, which the Agent probes for the camera anyway.
            (txt("device_name"), txt("machine_type"), address.to_string())
        } else {
            let port = service
                .port
                .map(|port| format!("{address}:{port}"))
                .unwrap_or_else(|| address.to_string());
            (None, None, port)
        };
        Some(Self {
            serial_number: None,
            host,
            name: Some(
                name.map(ToOwned::to_owned)
                    .unwrap_or_else(|| instance_display_name(&service.instance)),
            ),
            model: Some(
                model
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| KLIPPER_MODEL.to_owned()),
            ),
            source: "mdns",
        })
    }
}

/// Folds one response packet into the accumulated instances. Responders may
/// split PTR, SRV, TXT and A records across packets, so each is matched by
/// owner name and the printer's address is resolved once its target is known.
fn apply_response(
    packet: &[u8],
    source: Ipv4Addr,
    instances: &mut BTreeMap<String, ServiceInstance>,
) -> anyhow::Result<()> {
    let records = parse_records(packet)?;
    let mut addresses = BTreeMap::new();
    for record in &records {
        let service_instance = SERVICES.iter().any(|service| {
            record
                .name
                .to_ascii_lowercase()
                .ends_with(&format!(".{service}"))
        });
        match record.rtype {
            TYPE_PTR if SERVICES.iter().any(|s| record.name.eq_ignore_ascii_case(s)) => {
                let (instance, _) = read_name(packet, record.rdata.start)?;
                let entry = instances
                    .entry(instance.to_ascii_lowercase())
                    .or_insert_with(|| ServiceInstance::advertised(&instance));
                entry.address = entry.address.or(Some(source));
            }
            TYPE_SRV if service_instance => {
                let rdata = record.rdata(packet)?;
                let port = u16::from_be_bytes([rdata[4], rdata[5]]);
                let (target, _) = read_name(packet, record.rdata.start + 6)?;
                let instance = instances
                    .entry(record.name.to_ascii_lowercase())
                    .or_insert_with(|| ServiceInstance::advertised(&record.name));
                instance.port = Some(port);
                instance.target = Some(target);
            }
            TYPE_TXT if service_instance => {
                let props = parse_txt(record.rdata(packet)?)?;
                instances
                    .entry(record.name.to_ascii_lowercase())
                    .or_insert_with(|| ServiceInstance::advertised(&record.name))
                    .txt = props;
            }
            TYPE_A => {
                let rdata = record.rdata(packet)?;
                if let Some(octets) = rdata.get(..4) {
                    addresses.insert(
                        record.name.to_ascii_lowercase(),
                        Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]),
                    );
                }
            }
            _ => {}
        }
    }
    for instance in instances.values_mut() {
        if let Some(target) = &instance.target
            && let Some(address) = addresses.get(&target.to_ascii_lowercase())
        {
            instance.address = Some(*address);
        }
    }
    Ok(())
}

/// `"Moonraker-1a2b3c4d @ U1._moonraker._tcp.local"` → `"U1"`.
fn instance_display_name(instance: &str) -> String {
    let label = SERVICES
        .iter()
        .find_map(|service| instance.strip_suffix(&format!(".{service}")))
        .unwrap_or(instance);
    label.rsplit(" @ ").next().unwrap_or(label).to_owned()
}

/// TXT rdata: length-prefixed `key=value` strings.
fn parse_txt(rdata: &[u8]) -> anyhow::Result<BTreeMap<String, String>> {
    let mut props = BTreeMap::new();
    let mut offset = 0;
    while offset < rdata.len() {
        let len = usize::from(rdata[offset]);
        let entry = rdata
            .get(offset + 1..offset + 1 + len)
            .context("TXT entry exceeds record")?;
        let entry = String::from_utf8_lossy(entry);
        if let Some((key, value)) = entry.split_once('=') {
            props.insert(key.to_owned(), value.to_owned());
        }
        offset += 1 + len;
    }
    Ok(props)
}

struct Record {
    name: String,
    rtype: u16,
    rdata: Range<usize>,
}

impl Record {
    fn rdata<'a>(&self, packet: &'a [u8]) -> anyhow::Result<&'a [u8]> {
        packet
            .get(self.rdata.clone())
            .context("record data exceeds packet")
    }
}

/// Parses every resource record after the question section.
fn parse_records(packet: &[u8]) -> anyhow::Result<Vec<Record>> {
    let header = packet.get(..12).context("mDNS packet has no header")?;
    let u16_at = |at: usize| u16::from_be_bytes([header[at], header[at + 1]]);
    let mut offset = 12;
    for _ in 0..u16_at(4) {
        let (_, next) = read_name(packet, offset)?;
        offset = next + 4;
    }
    let mut records = Vec::new();
    for _ in 0..(u16_at(6) + u16_at(8) + u16_at(10)) {
        let (name, next) = read_name(packet, offset)?;
        let fixed = packet
            .get(next..next + 10)
            .context("resource record exceeds packet")?;
        let rtype = u16::from_be_bytes([fixed[0], fixed[1]]);
        let rdlength = usize::from(u16::from_be_bytes([fixed[8], fixed[9]]));
        let rdata = next + 10..next + 10 + rdlength;
        packet
            .get(rdata.clone())
            .context("record data exceeds packet")?;
        offset = rdata.end;
        records.push(Record { name, rtype, rdata });
    }
    Ok(records)
}

/// Reads a possibly-compressed domain name, returning it as a dot-joined
/// string (without the root label) together with the offset after the name
/// as it appears inline.
fn read_name(packet: &[u8], offset: usize) -> anyhow::Result<(String, usize)> {
    let mut labels = Vec::new();
    let mut cursor = offset;
    let mut end = None;
    for _ in 0..128 {
        let len = usize::from(*packet.get(cursor).context("domain name exceeds packet")?);
        if len & 0xC0 == 0xC0 {
            if end.is_none() {
                end = Some(cursor + 2);
            }
            let next = usize::from(
                *packet
                    .get(cursor + 1)
                    .context("name pointer exceeds packet")?,
            );
            cursor = ((len & 0x3F) << 8) | next;
        } else if len == 0 {
            end.get_or_insert(cursor + 1);
            return Ok((labels.join("."), end.unwrap()));
        } else if len > 63 {
            anyhow::bail!("domain label is too long");
        } else {
            cursor += 1;
            let label = packet
                .get(cursor..cursor + len)
                .context("label exceeds packet")?;
            labels.push(String::from_utf8_lossy(label).into_owned());
            cursor += len;
        }
    }
    anyhow::bail!("domain name has too many labels or loops")
}
