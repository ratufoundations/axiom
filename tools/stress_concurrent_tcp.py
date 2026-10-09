#!/usr/bin/env python3
import concurrent.futures
import os
import shutil
import socket
import subprocess
import sys
import time
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parent.parent
NODE_PORT = 9005
STRESS_DIR = ROOT_DIR / "target" / "stress"
NODE_DATA_DIR = STRESS_DIR / "data"
NODE_ARCHIVE_DIR = STRESS_DIR / "archive"
NUM_CLIENTS = 16
TX_PER_CLIENT = 100

BIN_EXT = ".exe" if os.name == "nt" else ""
NODE_BIN = ROOT_DIR / "target" / "release" / f"ratu-aurion-node{BIN_EXT}"
CLI_BIN = ROOT_DIR / "target" / "release" / f"ratu-aurion-cli{BIN_EXT}"

def log(msg: str):
    print(f"[STRESS-TEST] {msg}")

def fail(msg: str):
    print(f"\n[STRESS-TEST FAILURE] {msg}\n", file=sys.stderr)
    sys.exit(1)

def ensure_binaries():
    log("Mengompilasi biner ratu-aurion-node dan ratu-aurion-cli dengan profil rilis...")
    cmd = ["cargo", "build", "--release", "-p", "ratu-aurion-node", "-p", "ratu-aurion-cli"]
    res = subprocess.run(cmd, cwd=ROOT_DIR)
    if res.returncode != 0:
        fail("Gagal mengompilasi biner release.")
    if not NODE_BIN.exists() or not CLI_BIN.exists():
        fail("Biner target release tidak ditemukan setelah kompilasi.")

def setup_environment():
    log("Menyiapkan direktori penyimpanan pengujian stres...")
    if STRESS_DIR.exists():
        for _ in range(5):
            try:
                shutil.rmtree(STRESS_DIR)
                break
            except Exception:
                time.sleep(0.1)
    NODE_DATA_DIR.mkdir(parents=True, exist_ok=True)
    NODE_ARCHIVE_DIR.mkdir(parents=True, exist_ok=True)

def wait_for_node(port: int, timeout: float = 5.0):
    start = time.time()
    while time.time() - start < timeout:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(0.5)
            if sock.connect_ex(("127.0.0.1", port)) == 0:
                return True
        time.sleep(0.05)
    return False

def worker_routine(client_id: int) -> tuple[int, int, list[int]]:
    key_path = STRESS_DIR / f"worker_{client_id}.key"
    keygen_cmd = [str(CLI_BIN), "keygen", "--out", str(key_path)]
    subprocess.run(keygen_cmd, cwd=ROOT_DIR, check=True, stdout=subprocess.DEVNULL)

    latencies_micros = []
    success_count = 0
    fail_count = 0
    recipient_hex = "88" * 32

    for seq in range(1, TX_PER_CLIENT + 1):
        t0 = time.time_ns()
        cmd = [
            str(CLI_BIN), "transfer",
            "--key", str(key_path),
            "--to", recipient_hex,
            "--amount", "1.0000000000",
            "--epoch", "1",
            "--seq", str(seq),
            "--node", f"127.0.0.1:{NODE_PORT}"
        ]
        res = subprocess.run(cmd, cwd=ROOT_DIR, capture_output=True, text=True)
        elapsed_us = (time.time_ns() - t0) // 1000
        latencies_micros.append(elapsed_us)

        if res.returncode == 0 and "berhasil dikomit" in res.stdout:
            success_count += 1
        else:
            fail_count += 1

    return success_count, fail_count, latencies_micros

def run_stress_test():
    ensure_binaries()
    setup_environment()

    log("Menyalakan simpul ratu-aurion-node pada port 9005...")
    node_cmd = [
        str(NODE_BIN),
        "--data-dir", str(NODE_DATA_DIR),
        "--archive-dir", str(NODE_ARCHIVE_DIR),
        "--listen", f"127.0.0.1:{NODE_PORT}",
        "--epoch", "1",
        "--seed-byte", "99"
    ]
    node_proc = subprocess.Popen(node_cmd, cwd=ROOT_DIR, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

    try:
        if not wait_for_node(NODE_PORT):
            fail("Simpul gagal menyala pada port 9005.")

        log(f"Menjalankan beban konkurensi: {NUM_CLIENTS} thread secara simultan...")
        start_total = time.time_ns()

        all_latencies = []
        total_success = 0
        total_fail = 0

        with concurrent.futures.ThreadPoolExecutor(max_workers=NUM_CLIENTS) as executor:
            futures = [executor.submit(worker_routine, cid) for cid in range(NUM_CLIENTS)]
            for fut in concurrent.futures.as_completed(futures):
                s, f, lats = fut.result()
                total_success += s
                total_fail += f
                all_latencies.extend(lats)

        elapsed_total_ms = (time.time_ns() - start_total) // 1_000_000
        all_latencies.sort()
        n = len(all_latencies)

        p50 = all_latencies[n * 50 // 100] if n > 0 else 0
        p95 = all_latencies[n * 95 // 100] if n > 0 else 0
        p99 = all_latencies[n * 99 // 100] if n > 0 else 0

        log("\n--- HASIL UJI BEBAN KONKURENSI TCP ---")
        log(f"Total Transaksi Berhasil : {total_success} / {NUM_CLIENTS * TX_PER_CLIENT}")
        log(f"Total Transaksi Gagal    : {total_fail}")
        log(f"Waktu Total Eksekusi     : {elapsed_total_ms} ms")
        log(f"Latensi Kunci p50        : {p50} µs")
        log(f"Latensi Kunci p95        : {p95} µs")
        log(f"Latensi Kunci p99        : {p99} µs")

        log("\n--- AUDIT INTEGRITAS FISIK SEGMEN DISK ---")
        log_path = NODE_DATA_DIR / "epoch_1_seg_0.log"
        if not log_path.exists():
            fail("Berkas segmen disk epoch_1_seg_0.log tidak ditemukan.")

        expected_size = 42 + (total_success * 161)
        actual_size = os.path.getsize(log_path)
        log(f"Ukuran Berkas Aktual     : {actual_size} byte")
        log(f"Ukuran Berkas Ekspektasi : {expected_size} byte")

        if actual_size != expected_size:
            fail(f"Ukuran disk tidak sesuai: aktual={actual_size}, ekspektasi={expected_size}")

        log("Integritas linier disk valid 100%. Tidak ditemukan tumpang tindih mutasi.")

    finally:
        log("Menghentikan proses simpul pengujian...")
        node_proc.terminate()
        try:
            node_proc.wait(timeout=2.0)
        except subprocess.TimeoutExpired:
            node_proc.kill()

if __name__ == "__main__":
    run_stress_test()
