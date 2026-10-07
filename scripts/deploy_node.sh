#!/usr/bin/env bash
set -euo pipefail

NODE_USER="axiom"
NODE_GROUP="axiom"
INSTALL_BIN="/usr/local/bin/axiom-node"
CONFIG_DIR="/etc/axiom"
DATA_DIR="/var/lib/axiom/data"
ARCHIVE_DIR="/var/lib/axiom/archive"
LOG_DIR="/var/log/axiom"
SYSTEMD_FILE="/etc/systemd/system/axiom-node.service"
JOURNALD_DROPIN_DIR="/etc/systemd/journald.conf.d"
JOURNALD_FILE="$JOURNALD_DROPIN_DIR/axiom.conf"
LOGROTATE_FILE="/etc/logrotate.d/axiom-node"

echo "[DEPLOY] Memulai deployment axiom-node..."

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
chown -R "$NODE_USER":"$NODE_GROUP" /var/lib/axiom "$LOG_DIR"
chmod 750 /var/lib/axiom "$DATA_DIR" "$ARCHIVE_DIR" "$LOG_DIR"
chown -R root:"$NODE_GROUP" "$CONFIG_DIR"
chmod 750 "$CONFIG_DIR"

# 4. Salin biner release
if [[ -f "./target/release/axiom-node" ]]; then
    install -m 755 -o root -g root ./target/release/axiom-node "$INSTALL_BIN"
    echo "[DEPLOY] Biner berhasil dipasang di $INSTALL_BIN."
else
    echo "[ERROR] Biner target/release/axiom-node tidak ditemukan. Kompilasi terlebih dahulu dengan: cargo build --release -p axiom-node" >&2
    exit 1
fi

# 5. Pasang berkas konfigurasi default jika belum ada
if [[ ! -f "$CONFIG_DIR/node.env" ]]; then
    cat << 'EOF' > "$CONFIG_DIR/node.env"
AXIOM_DATA_DIR=/var/lib/axiom/data
AXIOM_ARCHIVE_DIR=/var/lib/axiom/archive
AXIOM_LISTEN_ADDR=0.0.0.0:9001
AXIOM_EPOCH=1
AXIOM_SEED_BYTE=11
AXIOM_IPC_SOCKET=/run/axiom/telemetry.sock
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
/var/log/axiom/*.log {
    daily
    missingok
    rotate 14
    compress
    delaycompress
    notifempty
    create 0640 axiom axiom
    sharedscripts
    
    # Menggunakan copytruncate agar file descriptor simpul tetap valid 
    # tanpa membutuhkan SIGHUP handler khusus di runtime
    copytruncate

    # Jalankan pengecekan integritas setelah rotasi
    postrotate
        /usr/bin/find /var/log/axiom -type f -name "*.gz" -mtime +14 -delete
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
User=axiom
Group=axiom
EnvironmentFile=/etc/axiom/node.env

ExecStart=/usr/local/bin/axiom-node \
    --data-dir ${AXIOM_DATA_DIR} \
    --archive-dir ${AXIOM_ARCHIVE_DIR} \
    --listen ${AXIOM_LISTEN_ADDR} \
    --epoch ${AXIOM_EPOCH} \
    --seed-byte ${AXIOM_SEED_BYTE} \
    --ipc-socket ${AXIOM_IPC_SOCKET}

# Pengalihan stream stdout dan stderr ke berkas terdedikasi
StandardOutput=append:/var/log/axiom/node.log
StandardError=append:/var/log/axiom/node-error.log

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
ReadWritePaths=/var/lib/axiom /var/log/axiom /run/axiom
RuntimeDirectory=axiom
RuntimeDirectoryMode=0755

[Install]
WantedBy=multi-user.target
EOF

chmod 644 "$SYSTEMD_FILE"
echo "[DEPLOY] Berkas unit systemd dipasang di $SYSTEMD_FILE."

# 9. Muat ulang daemon, terapkan konfigurasi, dan aktifkan layanan
systemctl restart systemd-journald || true
systemctl daemon-reload
systemctl enable axiom-node.service
systemctl restart axiom-node.service

echo "[DEPLOY] Deployment selesai. Status layanan:"
systemctl status axiom-node.service --no-pager
