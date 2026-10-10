#!/usr/bin/env python3
import datetime
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parent.parent
LOGS_DIR = ROOT_DIR / "logs"

FORBIDDEN_PATTERNS = [
    (r"\bunsafe\b", "Penggunaan kata kunci 'unsafe'"),
    (r"\bf32\b", "Penggunaan tipe floating-point 'f32'"),
    (r"\bf64\b", "Penggunaan tipe floating-point 'f64'"),
]

def log(msg: str):
    print(f"[GUARD] {msg}")

def fail(msg: str):
    print(f"\n[GUARD ERROR] {msg}\n", file=sys.stderr)
    sys.exit(1)

def run_cmd_capture(cmd: list[str]) -> tuple[int, str]:
    res = subprocess.run(cmd, cwd=ROOT_DIR, capture_output=True, text=True)
    output = (res.stdout + "\n" + res.stderr).strip()
    return res.returncode, output

def detect_active_task() -> str:
    task_file = ROOT_DIR / "task-register.md"
    if not task_file.exists():
        return "UNKNOWN"
    content = task_file.read_text(encoding="utf-8")
    for line in content.splitlines():
        if "Dalam Pengerjaan" in line or "In Progress" in line or "Review" in line:
            match = re.search(r"(?:PERF-[A-Z]+-\d+|SEC-[A-Z]+-\d+|GATEWAY-[A-Z]+-\d+|NET-[A-Z]+-\d+|E2E-[A-Z]+-\d+|REFACTOR-REBRAND-\d+|OPT-[A-Z]+-\d+|TR-\d+)", line)
            if match:
                return match.group(0)
    for line in reversed(content.splitlines()):
        match = re.search(r"(?:PERF-[A-Z]+-\d+|SEC-[A-Z]+-\d+|GATEWAY-[A-Z]+-\d+|NET-[A-Z]+-\d+|E2E-[A-Z]+-\d+|REFACTOR-REBRAND-\d+|OPT-[A-Z]+-\d+|TR-\d+)", line)
        if match:
            return match.group(0)
    return "MISC"

def audit_source_code() -> tuple[int, list[str]]:
    crates_dir = ROOT_DIR / "crates"
    bin_dir = ROOT_DIR / "bin"
    total_files = 0
    violations = []

    for base in [crates_dir, bin_dir]:
        if not base.exists():
            continue
        for rs_file in base.rglob("*.rs"):
            if "target" in rs_file.parts:
                continue
            total_files += 1
            lines = rs_file.read_text(encoding="utf-8").splitlines()
            for line_no, line in enumerate(lines, start=1):
                stripped = line.strip()
                if stripped.startswith("//") or stripped.startswith("/*") or stripped.startswith("*"):
                    continue
                if "forbid(unsafe_code)" in line:
                    continue
                for pattern, desc in FORBIDDEN_PATTERNS:
                    if re.search(pattern, line):
                        violations.append(f"{rs_file.relative_to(ROOT_DIR)}:{line_no} -> {desc}")

    return total_files, violations

def get_git_diff_stat() -> str:
    _, stat = run_cmd_capture(["git", "diff", "--stat"])
    _, staged_stat = run_cmd_capture(["git", "diff", "--cached", "--stat"])
    combined = []
    if stat:
        combined.append("Perubahan belum staged:\n" + stat)
    if staged_stat:
        combined.append("Perubahan staged:\n" + staged_stat)
    return "\n".join(combined) if combined else "Tidak ada perubahan berkas."

def write_audit_log(task_id: str, files_scanned: int, check_res: str, clippy_res: str, test_res: str):
    LOGS_DIR.mkdir(parents=True, exist_ok=True)
    today = datetime.date.today().strftime("%Y-%m-%d")
    timestamp = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    log_file = LOGS_DIR / f"{today}_{task_id}.log"

    git_stat = get_git_diff_stat()

    log_entry = f"""
================================================================================
FAKTA AUDIT OTOMATIS RATU AURION - {timestamp}
TASK TERDETEKSI: {task_id}
================================================================================

[AUDIT_KODE_STATIK]
- Total Berkas Rust Dipindai: {files_scanned}
- Unsafe Code: NIHIL (Terverifikasi)
- Floating-Point: NIHIL (Terverifikasi)

[METRIK_GIT]
{git_stat}

[EKSEKUSI_KOMPILASI]
{check_res}

[EKSEKUSI_CLIPPY]
{clippy_res}

[EKSEKUSI_UNIT_TEST]
{test_res}

[STATUS_INTEGRITAS]
Semua gerbang verifikasi lolos secara deterministik.
================================================================================
"""
    with open(log_file, "a", encoding="utf-8") as f:
        f.write(log_entry.strip() + "\n\n")

    log(f"Fakta audit otomatis berhasil dicatat ke '{log_file.relative_to(ROOT_DIR)}'.")

def main():
    log("=== MEMULAI GERBANG AUDIT OTOMATIS RATU AURION ===")
    task_id = detect_active_task()

    # 1. Audit Statis
    total_files, violations = audit_source_code()
    if violations:
        fail("Pelanggaran aturan absolut terdeteksi:\n" + "\n".join(violations))

    # 2. Cargo Check
    code, check_out = run_cmd_capture(["cargo", "check", "--workspace"])
    if code != 0:
        fail(f"Cargo check gagal:\n{check_out}")

    # 3. Cargo Clippy
    code, clippy_out = run_cmd_capture(["cargo", "clippy", "--workspace", "--", "-D", "warnings"])
    if code != 0:
        fail(f"Cargo clippy menemukan masalah:\n{clippy_out}")

    # 4. Cargo Test
    code, test_out = run_cmd_capture(["cargo", "test", "--workspace"])
    if code != 0:
        fail(f"Pengujian unit test gagal:\n{test_out}")

    # 5. Tulis Log Otomatis
    write_audit_log(task_id, total_files, "Lolos tanpa peringatan.", "Lolos tanpa peringatan.", test_out)

    # 6. Git Commit & Push
    if os.environ.get("CI") == "true":
        log("Lingkungan CI terdeteksi. Melewati commit & push otomatis.")
        return

    _, status = run_cmd_capture(["git", "status", "--porcelain"])
    if not status:
        log("Tidak ada perubahan berkas untuk dikomit.")
        return

    subprocess.run(["git", "add", "-A"], cwd=ROOT_DIR, check=True)
    default_msg = f"chore({task_id.lower()}): automated verified update under guards"
    commit_msg = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("GUARD_COMMIT_MSG", default_msg)
    subprocess.run(["git", "commit", "-m", commit_msg], cwd=ROOT_DIR, check=True)
    subprocess.run(["git", "push"], cwd=ROOT_DIR, check=True)
    log("Perubahan berhasil dikomit dan didorong ke remote repository.")

if __name__ == "__main__":
    main()
