#!/usr/bin/env bash
set -euo pipefail

NODE_USER="axiom"
NODE_GROUP="axiom"
INSTALL_BIN="/usr/local/bin/axiom-node"
CONFIG_DIR="/etc/axiom"
DATA_DIR="/var/lib/axiom/data"
ARCHIVE_DIR="/var/lib/axiom/archive"
SYSTEMD_FILE="/etc/systemd/system/axiom-node.service"

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

# 3. Buat struktur direktori data dan konfigurasi
mkdir -p "$CONFIG_DIR" "$DATA_DIR" "$ARCHIVE_DIR"
chown -R "$NODE_USER":"$NODE_GROUP" /var/lib/axiom
chmod 750 /var/lib/axiom "$DATA_DIR" "$ARCHIVE_DIR"
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
EOF
    chmod 640 "$CONFIG_DIR/node.env"
    echo "[DEPLOY] Konfigurasi awal dibuat di $CONFIG_DIR/node.env."
fi

# 6. Pasang berkas unit service
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
    --seed-byte ${AXIOM_SEED_BYTE}

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
ReadWritePaths=/var/lib/axiom

[Install]
WantedBy=multi-user.target
EOF

chmod 644 "$SYSTEMD_FILE"

# 7. Muat ulang systemd dan aktifkan layanan
systemctl daemon-reload
systemctl enable axiom-node.service
systemctl restart axiom-node.service

echo "[DEPLOY] Deployment selesai. Status layanan:"
systemctl status axiom-node.service --no-pager
