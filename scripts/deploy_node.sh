#!/usr/bin/env bash
set -euo pipefail

NODE_USER="ratu-aurion"
NODE_GROUP="ratu-aurion"
INSTALL_BIN="/usr/local/bin/ratu-aurion-node"
CONFIG_DIR="/etc/ratu-aurion"
DATA_DIR="/var/lib/ratu-aurion/data"
ARCHIVE_DIR="/var/lib/ratu-aurion/archive"
LOG_DIR="/var/log/ratu-aurion"
SYSTEMD_FILE="/etc/systemd/system/ratu-aurion-node.service"
JOURNALD_DROPIN_DIR="/etc/systemd/journald.conf.d"
JOURNALD_FILE="$JOURNALD_DROPIN_DIR/ratu-aurion.conf"
LOGROTATE_FILE="/etc/logrotate.d/ratu-aurion-node"

echo "[DEPLOY] Memulai deployment ratu-aurion-node..."

# 1. Pastikan dieksekusi dengan hak akses root
if [[ $EUID -ne 0 ]]; then
   echo "[ERROR] Skrip ini wajib dijalankan dengan akses root (sudo)." >&2
   exit 1
fi

# 2. Buat grup dan pengguna sistem khusus
if ! getent group "$NODE_GROUP" >/dev/null; then
    groupadd --system "$NODE_GROUP"
    echo "[DEPLOY] Grup sistem '$NODE_GROUP' dibuat."
fi

if ! id -u "$NODE_USER" >/dev/null 2>&1; then
    useradd --system --no-create-home --shell /usr/sbin/nologin --gid "$NODE_GROUP" "$NODE_USER"
    echo "[DEPLOY] Pengguna sistem '$NODE_USER' dibuat."
fi

# 3. Buat struktur direktori data, arsip, konfigurasi, dan log
mkdir -p "$CONFIG_DIR" "$DATA_DIR" "$ARCHIVE_DIR" "$LOG_DIR"
chown -R "$NODE_USER":"$NODE_GROUP" /var/lib/ratu-aurion "$LOG_DIR"
chmod 750 /var/lib/ratu-aurion "$DATA_DIR" "$ARCHIVE_DIR" "$LOG_DIR"
chown -R root:"$NODE_GROUP" "$CONFIG_DIR"
chmod 750 "$CONFIG_DIR"

# 4. Salin biner release
if [[ -f "./target/release/ratu-aurion-node" ]]; then
    install -m 755 -o root -g root ./target/release/ratu-aurion-node "$INSTALL_BIN"
    echo "[DEPLOY] Biner berhasil dipasang di $INSTALL_BIN."
else
    echo "[ERROR] Biner target/release/ratu-aurion-node tidak ditemukan. Kompilasi terlebih dahulu dengan: cargo build --release -p ratu-aurion-node" >&2
    exit 1
fi

# 5. Pasang berkas konfigurasi default jika belum ada
if [[ ! -f "$CONFIG_DIR/node.env" ]]; then
    cat << 'EOF' > "$CONFIG_DIR/node.env"
AXIOM_DATA_DIR=/var/lib/ratu-aurion/data
AXIOM_ARCHIVE_DIR=/var/lib/ratu-aurion/archive
AXIOM_LISTEN_ADDR=0.0.0.0:9001
AXIOM_EPOCH=1
AXIOM_SEED_BYTE=11
AXIOM_IPC_SOCKET=/run/ratu-aurion/telemetry.sock
EOF
    chmod 640 "$CONFIG_DIR/node.env"
    echo "[DEPLOY] Konfigurasi awal dibuat di $CONFIG_DIR/node.env."
fi

# 6. Pasang konfigurasi drop-in journald
mkdir -p "$JOURNALD_DROPIN_DIR"
cat << 'EOF' > "$JOURNALD_FILE"
[Journal]
# Batas penyimpanan fisik journald di disk (/var/log/journal)
SystemMaxUse=2G
SystemKeepFree=5G
SystemMaxFileSize=256M

# Retensi waktu penyimpanan log
MaxRetentionSec=1month

# Pembatasan laju entri log per proses (mencegah I/O thrashing saat lonjakan trafik)
RateLimitIntervalSec=30s
RateLimitBurst=10000
EOF
chmod 644 "$JOURNALD_FILE"
echo "[DEPLOY] Konfigurasi journald drop-in dipasang di $JOURNALD_FILE."

# 7. Pasang konfigurasi logrotate
cat << 'EOF' > "$LOGROTATE_FILE"
/var/log/ratu-aurion/*.log {
    daily
    missingok
    rotate 14
    compress
    delaycompress
    notifempty
    create 0640 ratu-aurion ratu-aurion
    sharedscripts
    
    # Menggunakan copytruncate agar file descriptor simpul tetap valid 
    # tanpa membutuhkan SIGHUP handler khusus di runtime
    copytruncate

    # Jalankan pengecekan integritas setelah rotasi
    postrotate
        /usr/bin/find /var/log/ratu-aurion -type f -name "*.gz" -mtime +14 -delete
    endscript
}
EOF
chmod 644 "$LOGROTATE_FILE"
echo "[DEPLOY] Konfigurasi logrotate dipasang di $LOGROTATE_FILE."

# 8. Pasang berkas unit service systemd
cat << 'EOF' > "$SYSTEMD_FILE"
[Unit]
Description=Axiom Deterministic Ledger Node
After=network.target network-online.target
Wants=network-online.target

[Service]
Type=simple
User=ratu-aurion
Group=ratu-aurion
EnvironmentFile=/etc/ratu-aurion/node.env

ExecStart=/usr/local/bin/ratu-aurion-node \
    --data-dir ${AXIOM_DATA_DIR} \
    --archive-dir ${AXIOM_ARCHIVE_DIR} \
    --listen ${AXIOM_LISTEN_ADDR} \
    --epoch ${AXIOM_EPOCH} \
    --seed-byte ${AXIOM_SEED_BYTE} \
    --ipc-socket ${AXIOM_IPC_SOCKET}

# Pengalihan stream stdout dan stderr ke berkas terdedikasi
StandardOutput=append:/var/log/ratu-aurion/node.log
StandardError=append:/var/log/ratu-aurion/node-error.log

# Proteksi rate-limit bawaan unit systemd
LogRateLimitIntervalSec=30s
LogRateLimitBurst=10000

KillMode=process
KillSignal=SIGINT
TimeoutStopSec=30
Restart=always
RestartSec=5s

LimitNOFILE=65536
LimitMEMLOCK=infinity

NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
ReadWritePaths=/var/lib/ratu-aurion /var/log/ratu-aurion /run/ratu-aurion
RuntimeDirectory=ratu-aurion
RuntimeDirectoryMode=0755

[Install]
WantedBy=multi-user.target
EOF

chmod 644 "$SYSTEMD_FILE"
echo "[DEPLOY] Berkas unit systemd dipasang di $SYSTEMD_FILE."

# 9. Muat ulang daemon, terapkan konfigurasi, dan aktifkan layanan
systemctl restart systemd-journald || true
systemctl daemon-reload
systemctl enable ratu-aurion-node.service
systemctl restart ratu-aurion-node.service

echo "[DEPLOY] Deployment selesai. Status layanan:"
systemctl status ratu-aurion-node.service --no-pager
