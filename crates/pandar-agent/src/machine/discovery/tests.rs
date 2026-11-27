//! Discovery unit tests: the SSDP parser, a UDP-level SSDP exchange, and an
//! mDNS exchange against a synthetic python-zeroconf-style response.

use std::collections::BTreeMap;

use super::*;

#[test]
fn parser_extracts_bambuddy_fields_and_source_host() {
    let packet = b"HTTP/1.1 200 OK\r\nUSN: SERIAL123::urn:bambulab-com:device:3dprinter:1\r\nDevName.bambu.com: Office X1C\r\nDevModel.bambu.com: X1 Carbon\r\nST: urn:bambulab-com:device:3dprinter:1\r\n\r\n";
    let source = "192.0.2.55:2021".parse().unwrap();

    assert_eq!(
        parse_ssdp_response(packet, source),
        Some(DiscoveredPrinter {
            serial_number: Some("SERIAL123".to_owned()),
            host: "192.0.2.55".to_owned(),
            name: Some("Office X1C".to_owned()),
            model: Some("X1 Carbon".to_owned()),
            source: "ssdp",
        })
    );
}

#[test]
fn parser_ignores_unrelated_packets() {
    let source = "192.0.2.55:2021".parse().unwrap();

    assert_eq!(
        parse_ssdp_response(b"HTTP/1.1 200 OK\r\nST: upnp:rootdevice\r\n\r\n", source),
        None
    );
    assert_eq!(
        parse_ssdp_response(b"HTTP/1.1 200 OK\r\nUSN: uuid:SERIAL123\r\n\r\n", source),
        None
    );
}

#[test]
fn deduplication_prefers_first_serial_or_host() {
    let printers = vec![
        printer(Some("SERIAL1"), "192.0.2.1"),
        printer(Some("SERIAL1"), "192.0.2.2"),
        printer(None, "192.0.2.3"),
        printer(None, "192.0.2.3"),
    ];

    assert_eq!(
        deduplicate_printers(printers),
        vec![
            printer(Some("SERIAL1"), "192.0.2.1"),
            printer(None, "192.0.2.3")
        ]
    );
}

fn printer(serial_number: Option<&str>, host: &str) -> DiscoveredPrinter {
    DiscoveredPrinter {
        serial_number: serial_number.map(str::to_owned),
        host: host.to_owned(),
        name: None,
        model: None,
        source: "ssdp",
    }
}

#[tokio::test]
async fn direct_host_discovery_parses_unicast_response() {
    let server = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let addr = server.local_addr().unwrap();
    let server_task = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        let (_len, peer) = server.recv_from(&mut buf).await.unwrap();
        let response = b"HTTP/1.1 200 OK\r\nUSN: SERIAL123\r\nDevName.bambu.com: Office X1C\r\nDevModel.bambu.com: X1 Carbon\r\nST: urn:bambulab-com:device:3dprinter:1\r\n\r\n";
        server.send_to(response, peer).await.unwrap();
    });

    let printer = discover_printer_at_addr(addr, 1).await.unwrap().unwrap();

    assert_eq!(printer.serial_number.as_deref(), Some("SERIAL123"));
    assert_eq!(printer.host, Ipv4Addr::LOCALHOST.to_string());
    assert_eq!(printer.name.as_deref(), Some("Office X1C"));
    assert_eq!(printer.model.as_deref(), Some("X1 Carbon"));
    server_task.await.unwrap();
}

#[test]
fn local_subnet_targets_cover_private_interface_peers_only() {
    let targets =
        scan::local_subnet_targets(Ipv4Addr::new(10, 1, 61, 3), Ipv4Addr::new(255, 255, 255, 0));

    assert_eq!(targets.len(), 253);
    assert!(targets.contains(&SocketAddr::from((Ipv4Addr::new(10, 1, 61, 84), 2021))));
    assert!(!targets.contains(&SocketAddr::from((Ipv4Addr::new(10, 1, 61, 0), 2021))));
    assert!(!targets.contains(&SocketAddr::from((Ipv4Addr::new(10, 1, 61, 3), 2021))));
    assert!(!targets.contains(&SocketAddr::from((Ipv4Addr::new(10, 1, 61, 255), 2021))));
}

#[test]
fn local_subnet_targets_are_bounded_and_never_scan_public_networks() {
    let broad_private_targets =
        scan::local_subnet_targets(Ipv4Addr::new(10, 1, 61, 3), Ipv4Addr::new(255, 0, 0, 0));

    assert_eq!(broad_private_targets.len(), 1021);
    assert!(
        broad_private_targets.contains(&SocketAddr::from((Ipv4Addr::new(10, 1, 61, 84), 2021)))
    );
    assert!(
        scan::local_subnet_targets(
            Ipv4Addr::new(203, 0, 113, 3),
            Ipv4Addr::new(255, 255, 255, 0),
        )
        .is_empty()
    );
}

#[tokio::test]
async fn discovery_finds_unicast_printer_when_another_target_does_not_respond() {
    let silent_target = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let printer = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let printer_addr = printer.local_addr().unwrap();
    let printer_task = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        let (_len, peer) = printer.recv_from(&mut buf).await.unwrap();
        let response = b"HTTP/1.1 200 OK\r\nUSN: FALLBACK123\r\nDevName.bambu.com: Unicast Printer\r\nDevModel.bambu.com: P1S\r\nST: urn:bambulab-com:device:3dprinter:1\r\n\r\n";
        printer.send_to(response, peer).await.unwrap();
    });

    let result = discover_printers_at_targets(
        Duration::from_secs(1),
        [silent_target.local_addr().unwrap(), printer_addr],
    )
    .await
    .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].serial_number.as_deref(), Some("FALLBACK123"));
    assert_eq!(result[0].name.as_deref(), Some("Unicast Printer"));
    printer_task.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs a real Moonraker printer on the LAN; set PANDAR_MDNS_PROBE=1"]
async fn mdns_discovery_finds_a_real_moonraker() {
    if std::env::var("PANDAR_MDNS_PROBE").is_err() {
        return;
    }
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
        )
        .with_test_writer()
        .try_init();
    let found = discover_printers(4).await.unwrap();
    let moonraker: Vec<DiscoveredPrinter> = found
        .printers
        .into_iter()
        .filter(|printer| printer.source == "mdns")
        .collect();
    assert!(!moonraker.is_empty(), "no _moonraker._tcp services found");
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/e2e-artifacts");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("moonraker-mdns.json"),
        serde_json::to_string_pretty(&moonraker).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn mdns_discovery_resolves_moonraker_instances_from_split_compressed_responses() {
    let responder = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let responder_addr = responder.local_addr().unwrap();
    let instance = "Moonraker-1a2b3c4d @ U1._moonraker._tcp.local.";
    let responder_task = tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        let (len, peer) = responder.recv_from(&mut buf).await.unwrap();
        // One PTR question per service, both asking for unicast responses.
        assert_eq!(
            &buf[..len],
            b"\0\0\0\0\0\x02\0\0\0\0\0\0\x0a_moonraker\x04_tcp\x05local\0\0\x0c\x80\x01\x0a_snapmaker\x04_tcp\x05local\0\0\x0c\x80\x01"
        );
        // The PTR and TXT records arrive first; SRV and the compressed A record follow.
        let mut pointer = MdnsResponse::new(1, 1);
        pointer.record(mdns::MOONRAKER_SERVICE, 12, |p| p.name(instance));
        pointer.record(instance, 16, |p| {
            p.bytes.extend_from_slice(b"\x08version=2")
        });
        responder.send_to(&pointer.bytes, peer).await.unwrap();
        let mut records = MdnsResponse::new(2, 0);
        records.record(instance, 33, |p| {
            p.bytes.extend_from_slice(&[0, 0, 0, 0, 0x1b, 0xd5]);
            p.name("U1.local.");
        });
        records.record("U1.local.", 1, |p| {
            p.bytes.extend_from_slice(&[10, 1, 61, 14]);
        });
        responder.send_to(&records.bytes, peer).await.unwrap();
    });

    let printers = discover_mdns_printers(Duration::from_secs(2), [responder_addr])
        .await
        .unwrap();

    assert_eq!(
        printers,
        vec![DiscoveredPrinter {
            serial_number: None,
            host: "10.1.61.14:7125".to_owned(),
            name: Some("U1".to_owned()),
            model: Some("Klipper".to_owned()),
            source: "mdns",
        }]
    );
    responder_task.await.unwrap();
}

/// Snapmaker's U1 firmware advertises `_snapmaker._tcp.local` with the printer
/// name and model in TXT records and its MQTT broker on the SRV port; the
/// discovery result must point at the bare host so Moonraker is reached on the
/// nginx port.
#[tokio::test]
async fn mdns_discovery_reads_snapmaker_services_without_the_mqtt_port() {
    let responder = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let responder_addr = responder.local_addr().unwrap();
    let responder_task = tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        let (len, peer) = responder.recv_from(&mut buf).await.unwrap();
        assert_eq!(u16::from_be_bytes([buf[4], buf[5]]), 2, "two questions");
        let _ = len;
        let mut response = MdnsResponse::new(4, 0);
        response.record(mdns::SNAPMAKER_SERVICE, 12, |p| {
            p.name("U1._snapmaker._tcp.local.")
        });
        response.record("U1._snapmaker._tcp.local.", 33, |p| {
            p.bytes.extend_from_slice(&[0, 0, 0, 0, 0x07, 0x5c]);
            p.name("U1-8110026101000146S3Z9.local.");
        });
        response.record("U1._snapmaker._tcp.local.", 16, |p| {
            p.bytes.extend_from_slice(b"\x0dversion=2.0.0\x19machine_type=Snapmaker U1\x14device_name=Index U1\x0dip=10.1.61.14");
        });
        response.record("U1-8110026101000146S3Z9.local.", 1, |p| {
            p.bytes.extend_from_slice(&[10, 1, 61, 14]);
        });
        responder.send_to(&response.bytes, peer).await.unwrap();
    });

    let printers = discover_mdns_printers(Duration::from_secs(2), [responder_addr])
        .await
        .unwrap();

    assert_eq!(
        printers,
        vec![DiscoveredPrinter {
            serial_number: None,
            host: "10.1.61.14".to_owned(),
            name: Some("Index U1".to_owned()),
            model: Some("Snapmaker U1".to_owned()),
            source: "mdns",
        }]
    );
    responder_task.await.unwrap();
}

/// Minimal mDNS response encoder with name compression, laid out the way
/// python-zeroconf emits records. Names are matched without their root dot.
struct MdnsResponse {
    bytes: Vec<u8>,
    offsets: BTreeMap<String, usize>,
}

impl MdnsResponse {
    fn new(answers: u16, additionals: u16) -> Self {
        let mut bytes = vec![0, 0, 0x84, 0, 0, 0];
        bytes.extend_from_slice(&answers.to_be_bytes());
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&additionals.to_be_bytes());
        Self {
            bytes,
            offsets: BTreeMap::new(),
        }
    }

    fn name(&mut self, name: &str) {
        let name = name.trim_end_matches('.');
        if let Some(offset) = self.offsets.get(name).copied() {
            self.bytes
                .extend_from_slice(&[0xc0 | (offset >> 8) as u8, offset as u8]);
            return;
        }
        self.offsets.insert(name.to_owned(), self.bytes.len());
        let (label, rest) = name.split_once('.').unwrap_or((name, ""));
        self.bytes.push(label.len() as u8);
        self.bytes.extend_from_slice(label.as_bytes());
        if rest.is_empty() {
            self.bytes.push(0);
        } else {
            self.name(rest);
        }
    }

    fn record(&mut self, name: &str, rtype: u16, rdata: impl FnOnce(&mut Self)) {
        self.name(name);
        self.bytes.extend_from_slice(&rtype.to_be_bytes());
        self.bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0x78]);
        let length_at = self.bytes.len();
        self.bytes.extend_from_slice(&[0, 0]);
        let start = self.bytes.len();
        rdata(self);
        let length = (self.bytes.len() - start) as u16;
        self.bytes[length_at..length_at + 2].copy_from_slice(&length.to_be_bytes());
    }
}
