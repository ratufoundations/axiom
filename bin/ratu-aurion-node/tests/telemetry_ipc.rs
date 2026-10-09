#![forbid(unsafe_code)]

#[cfg(unix)]
mod unix_ipc_tests {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    // Menggunakan path soket sementara yang unik
    fn temp_socket_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.join(format!("ratu_aurion_test_{}_{}_{}.sock", name, std::process::id(), timestamp))
    }

    #[test]
    fn test_ipc_request_response_roundtrip() {
        let sock_path = temp_socket_path("roundtrip");
        let collector = Arc::new(ratu_aurion_node::telemetry::NodeTelemetryCollector::new());
        collector.epoch.store(3, Ordering::Relaxed);
        collector.total_tx_committed.store(500, Ordering::Relaxed);

        let _server_handle = ratu_aurion_node::telemetry::IpcServer::start(&sock_path, Arc::clone(&collector))
            .expect("Gagal menjalankan server IPC");

        thread::sleep(Duration::from_millis(50));

        // 1. Klien menghubungkan ke soket
        let mut client = UnixStream::connect(&sock_path).expect("Gagal koneksi ke soket IPC");
        client.set_read_timeout(Some(Duration::from_secs(1))).unwrap();

        // 2. Kirim pesan permintaan sah (AXTI + command 1)
        let mut req = [0u8; 8];
        req[0..4].copy_from_slice(&ratu_aurion_node::telemetry::IPC_REQ_MAGIC);
        req[4..6].copy_from_slice(&1u16.to_le_bytes());
        client.write_all(&req).expect("Gagal menulis request");
        client.flush().unwrap();

        // 3. Baca balasan 128 byte
        let mut resp = [0u8; 128];
        client.read_exact(&mut resp).expect("Gagal membaca snapshot");

        // 4. Verifikasi isi balasan
        assert_eq!(&resp[0..4], &ratu_aurion_node::telemetry::IPC_RESP_MAGIC);
        let epoch = u64::from_le_bytes(resp[8..16].try_into().unwrap());
        let txs = u64::from_le_bytes(resp[28..36].try_into().unwrap());
        assert_eq!(epoch, 3);
        assert_eq!(txs, 500);

        // Verifikasi checksum BLAKE3
        let expected_hash = blake3::hash(&resp[0..96]);
        assert_eq!(expected_hash.as_bytes(), &resp[96..128]);

        let _ = std::fs::remove_file(&sock_path);
    }

    #[test]
    fn test_ipc_malformed_request_rejection() {
        let sock_path = temp_socket_path("malformed");
        let collector = Arc::new(ratu_aurion_node::telemetry::NodeTelemetryCollector::new());
        let _server_handle = ratu_aurion_node::telemetry::IpcServer::start(&sock_path, Arc::clone(&collector))
            .expect("Gagal menjalankan server IPC");

        thread::sleep(Duration::from_millis(50));

        let mut client = UnixStream::connect(&sock_path).expect("Gagal koneksi ke soket");
        client.set_read_timeout(Some(Duration::from_millis(200))).unwrap();

        // Kirim magic salah: b"ERRR"
        let bad_req = [0x45, 0x52, 0x52, 0x52, 0x01, 0x00, 0x00, 0x00];
        client.write_all(&bad_req).unwrap();
        client.flush().unwrap();

        let mut resp = [0u8; 128];
        let res = client.read_exact(&mut resp);

        // Server wajib menolak permintaan dan memutus koneksi tanpa respon snapshot
        assert!(res.is_err());

        let _ = std::fs::remove_file(&sock_path);
    }

    #[test]
    fn test_ipc_malformed_request_handling() {
        let sock_path = temp_socket_path("malformed_handling");
        let collector = Arc::new(ratu_aurion_node::telemetry::NodeTelemetryCollector::new());
        let _server_handle = ratu_aurion_node::telemetry::IpcServer::start(&sock_path, Arc::clone(&collector))
            .expect("Gagal menjalankan server IPC");

        thread::sleep(Duration::from_millis(50));

        // 1. Kasus magic rusak
        {
            let mut client = UnixStream::connect(&sock_path).expect("Gagal koneksi ke soket");
            client.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
            let bad_magic = [0x45, 0x52, 0x52, 0x52, 0x01, 0x00, 0x00, 0x00];
            client.write_all(&bad_magic).unwrap();
            client.flush().unwrap();
            let mut resp = [0u8; 128];
            assert!(client.read_exact(&mut resp).is_err());
        }

        // 2. Kasus payload terpotong (<8B, hanya 4 byte lalu ditutup)
        {
            let mut client = UnixStream::connect(&sock_path).expect("Gagal koneksi ke soket");
            client.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
            let truncated_req = [0x41, 0x58, 0x54, 0x49]; // "AXTI"
            client.write_all(&truncated_req).unwrap();
            let _ = client.shutdown(std::net::Shutdown::Write);
            let mut resp = [0u8; 128];
            assert!(client.read_exact(&mut resp).is_err());
        }

        // 3. Pastikan server tidak crash dan tetap melayani permintaan sah berikutnya
        {
            let mut client = UnixStream::connect(&sock_path).expect("Gagal koneksi ke soket");
            client.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
            let mut req = [0u8; 8];
            req[0..4].copy_from_slice(&ratu_aurion_node::telemetry::IPC_REQ_MAGIC);
            req[4..6].copy_from_slice(&1u16.to_le_bytes());
            client.write_all(&req).unwrap();
            client.flush().unwrap();
            let mut resp = [0u8; 128];
            client.read_exact(&mut resp).expect("Server harus tetap melayani permintaan sah");
            assert_eq!(&resp[0..4], &ratu_aurion_node::telemetry::IPC_RESP_MAGIC);
        }

        let _ = std::fs::remove_file(&sock_path);
    }

    #[test]
    fn test_ipc_concurrent_queries() {
        let sock_path = temp_socket_path("concurrent");
        let collector = Arc::new(ratu_aurion_node::telemetry::NodeTelemetryCollector::new());
        collector.epoch.store(10, Ordering::Relaxed);
        collector.total_tx_committed.store(2048, Ordering::Relaxed);

        let _server_handle = ratu_aurion_node::telemetry::IpcServer::start(&sock_path, Arc::clone(&collector))
            .expect("Gagal menjalankan server IPC");

        thread::sleep(Duration::from_millis(50));

        let num_clients = 10;
        let mut handles = Vec::new();

        for _ in 0..num_clients {
            let path = sock_path.clone();
            let col = Arc::clone(&collector);
            handles.push(thread::spawn(move || {
                col.total_tx_committed.fetch_add(1, Ordering::Relaxed);

                let mut client = UnixStream::connect(&path).expect("Klien gagal koneksi ke soket IPC");
                client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();

                let mut req = [0u8; 8];
                req[0..4].copy_from_slice(&ratu_aurion_node::telemetry::IPC_REQ_MAGIC);
                req[4..6].copy_from_slice(&1u16.to_le_bytes());
                client.write_all(&req).expect("Gagal menulis request");
                client.flush().unwrap();

                let mut resp = [0u8; 128];
                client.read_exact(&mut resp).expect("Gagal membaca snapshot");

                assert_eq!(&resp[0..4], &ratu_aurion_node::telemetry::IPC_RESP_MAGIC);
                assert_eq!(u64::from_le_bytes(resp[8..16].try_into().unwrap()), 10);
                let txs = u64::from_le_bytes(resp[28..36].try_into().unwrap());
                assert!(txs >= 2048);

                let expected_hash = blake3::hash(&resp[0..96]);
                assert_eq!(expected_hash.as_bytes(), &resp[96..128]);
            }));
        }

        for h in handles {
            h.join().expect("Client thread join gagal");
        }

        let _ = std::fs::remove_file(&sock_path);
    }
}

#[cfg(not(unix))]
mod non_unix_ipc_tests {
    use std::sync::Arc;

    #[test]
    fn test_ipc_non_unix_fallback() {
        let collector = Arc::new(ratu_aurion_node::telemetry::NodeTelemetryCollector::new());
        let res = ratu_aurion_node::telemetry::IpcServer::start("dummy.sock", collector);
        assert!(res.is_ok());
    }
}
