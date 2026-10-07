#!/usr/bin/env python3
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parent.parent

FORBIDDEN_PATTERNS = [
    (r"\bunsafe\b", "Penggunaan kata kunci 'unsafe' terdeteksi."),
    (r"\bf32\b", "Penggunaan tipe floating-point 'f32' terdeteksi."),
    (r"\bf64\b", "Penggunaan tipe floating-point 'f64' terdeteksi."),
]

def log(msg: str):
    print(f"[GUARD] {msg}")

def fail(msg: str):
    print(f"\n[GUARD ERROR] {msg}\n", file=sys.stderr)
    sys.exit(1)

def run_cmd(cmd: list[str], err_msg: str):
    log(f"Menjalankan: {' '.join(cmd)}")
    result = subprocess.run(cmd, cwd=ROOT_DIR)
    if result.returncode != 0:
        fail(err_msg)

def scan_rust_source_code():
    log("Memindai kepatuhan source code Rust terhadap unsafe dan floating-point...")
    crates_dir = ROOT_DIR / "crates"
    bin_dir = ROOT_DIR / "bin"
    
    target_dirs = [crates_dir, bin_dir]
    violations = []

    for base in target_dirs:
        if not base.exists():
            continue
        for rs_file in base.rglob("*.rs"):
            # Lewatkan direktori target build jika ada
            if "target" in rs_file.parts:
                continue

            try:
                content = rs_file.read_text(encoding="utf-8")
            except Exception as e:
                fail(f"Gagal membaca berkas {rs_file}: {e}")

            lines = content.splitlines()
            for line_no, line in enumerate(lines, start=1):
                # Abaikan baris komentar
                stripped = line.strip()
                if stripped.startswith("//") or stripped.startswith("/*") or stripped.startswith("*"):
                    continue

                for pattern, desc in FORBIDDEN_PATTERNS:
                    # Izinkan penegakan lint '#![forbid(unsafe_code)]'
                    if "forbid(unsafe_code)" in line:
                        continue
                    if re.search(pattern, line):
                        violations.append(f"{rs_file.relative_to(ROOT_DIR)}:{line_no} -> {desc} Baris: '{stripped}'")

    if violations:
        fail("Pelanggaran aturan absolut ditemukan:\n" + "\n".join(violations))
    log("Audit kode statis bersih: Tidak ditemukan unsafe maupun floating-point.")

def run_compiler_checks():
    run_cmd(["cargo", "check", "--workspace"], "Kompilasi 'cargo check' gagal.")
    run_cmd(["cargo", "clippy", "--workspace", "--", "-D", "warnings"], "Pemeriksaan 'cargo clippy' menemukan peringatan/galat.")

def verify_logging_discipline():
    log("Memverifikasi ketaatan dokumentasi logging...")
    logs_dir = ROOT_DIR / "logs"
    
    if not logs_dir.exists() or not logs_dir.is_dir():
        fail("Direktori 'logs/' tidak ditemukan. Seluruh agen wajib mencatat progres pada direktori 'logs/'.")
    
    log_files = list(logs_dir.glob("*.log"))
    if not log_files:
        fail("Tidak ditemukan berkas catatan (*.log) di direktori 'logs/'. Agen wajib mendokumentasikan log sebelum menyelesaikan pekerjaan.")
    
    # Periksa apakah ada berkas log yang berisi penanda wajib
    required_tags = ["[PRIORITAS]", "[DETAIL_MIKRO]", "[EKSEKUSI_UJI]", "[STATUS_AKTUAL]"]
    latest_log = max(log_files, key=lambda f: f.stat().st_mtime)
    content = latest_log.read_text(encoding="utf-8")
    
    missing_tags = [tag for tag in required_tags if tag not in content]
    if missing_tags:
        fail(f"Berkas log '{latest_log.name}' tidak memenuhi struktur standar. Tag yang hilang: {', '.join(missing_tags)}")
        
    log(f"Disiplin logging terverifikasi: '{latest_log.name}' memuat seluruh audit detail wajib.")

def get_git_status() -> str:
    res = subprocess.run(["git", "status", "--porcelain"], cwd=ROOT_DIR, capture_output=True, text=True)
    if res.returncode != 0:
        fail("Gagal membaca git status.")
    return res.stdout.strip()

def git_commit_and_push():
    status = get_git_status()
    if not status:
        log("Tidak ada perubahan berkas untuk di-commit. Workspace bersih.")
        return

    log("Perubahan terdeteksi. Mempersiapkan git commit dan push...")
    run_cmd(["git", "add", "-A"], "Gagal melakukan git add.")

    commit_msg = os.environ.get("GUARD_COMMIT_MSG", "chore(axiom): update workspace implementation under guard verification")
    run_cmd(["git", "commit", "-m", commit_msg], "Gagal melakukan git commit.")
    run_cmd(["git", "push"], "Gagal melakukan git push ke GitHub remote.")
    log("Perubahan berhasil diverifikasi, di-commit, dan di-push ke GitHub.")

def main():
    log("=== MEMULAI VERIFIKASI STANDAR AXIOM ===")
    scan_rust_source_code()
    run_compiler_checks()
    verify_logging_discipline()
    git_commit_and_push()
    log("=== SELURUH VERIFIKASI LOLOS ===")

if __name__ == "__main__":
    main()

