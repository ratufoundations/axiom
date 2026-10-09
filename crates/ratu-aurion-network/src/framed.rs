//! Modul transmisi stream berbingkai panjang (FramedStream) dengan batas ukuran dan timeout.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use crate::error::NetworkError;

/// Batas ukuran muatan bingkai maksimum (64 KB = 65.536 byte).
pub const MAX_FRAME_SIZE: usize = 65_536;

/// Batas waktu default socket I/O dalam milidetik (5.000 ms).
pub const DEFAULT_SOCKET_TIMEOUT_MS: u64 = 5_000;

/// Kapasitas default antrean saluran masuk berbatas (bounded ingress channel).
pub const INGRESS_CHANNEL_CAPACITY: usize = 4_096;

/// Pembungkus stream I/O berbingkai prefix panjang 4-byte Little-Endian.
#[derive(Debug)]
pub struct FramedStream<S> {
    stream: S,
    max_frame_size: usize,
    timeout_ms: Option<u64>,
    is_closed: bool,
}

impl<S> FramedStream<S> {
    /// Mengonstruksi FramedStream baru dari stream yang ada dengan batas frame standar 64 KB.
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            max_frame_size: MAX_FRAME_SIZE,
            timeout_ms: None,
            is_closed: false,
        }
    }

    /// Mengonstruksi FramedStream baru dengan batas frame kustom.
    pub fn with_max_frame_size(stream: S, max_frame_size: usize) -> Self {
        Self {
            stream,
            max_frame_size,
            timeout_ms: None,
            is_closed: false,
        }
    }

    /// Memeriksa apakah stream telah ditandai tertutup akibat galat fatal.
    #[inline]
    pub fn is_closed(&self) -> bool {
        self.is_closed
    }

    /// Menutup stream secara eksplisit.
    #[inline]
    pub fn close(&mut self) {
        self.is_closed = true;
    }

    /// Mengambil referensi stream internal.
    #[inline]
    pub fn stream(&self) -> &S {
        &self.stream
    }

    /// Mengambil referensi mutable stream internal.
    #[inline]
    pub fn stream_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    /// Mengonsumsi wrapper dan mengembalikan stream aslinya.
    #[inline]
    pub fn into_inner(self) -> S {
        self.stream
    }
}

impl FramedStream<TcpStream> {
    /// Mengonstruksi FramedStream dari TcpStream dengan konfigurasi batas waktu baca dan tulis.
    pub fn from_tcp(stream: TcpStream, timeout_ms: u64) -> Result<Self, NetworkError> {
        let dur = Some(Duration::from_millis(timeout_ms));
        stream.set_read_timeout(dur).map_err(NetworkError::IoError)?;
        stream.set_write_timeout(dur).map_err(NetworkError::IoError)?;

        Ok(Self {
            stream,
            max_frame_size: MAX_FRAME_SIZE,
            timeout_ms: Some(timeout_ms),
            is_closed: false,
        })
    }

    /// Memperbarui batas waktu baca dan tulis socket TCP.
    pub fn set_timeout(&mut self, timeout_ms: u64) -> Result<(), NetworkError> {
        let dur = Some(Duration::from_millis(timeout_ms));
        self.stream.set_read_timeout(dur).map_err(NetworkError::IoError)?;
        self.stream.set_write_timeout(dur).map_err(NetworkError::IoError)?;
        self.timeout_ms = Some(timeout_ms);
        Ok(())
    }
}

impl<S: Read + Write> FramedStream<S> {
    /// Membaca satu bingkai lengkap dari stream.
    ///
    /// 1. Membaca 4-byte Little-Endian panjang payload.
    /// 2. Memvalidasi panjang payload <= MAX_FRAME_SIZE. Jika melebihi batas,
    ///    menutup stream dan mengembalikan `NetworkError::FrameTooLarge`.
    /// 3. Membaca tepat `length` byte payload.
    pub fn read_frame(&mut self) -> Result<Vec<u8>, NetworkError> {
        if self.is_closed {
            return Err(NetworkError::IoError(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Framed connection is closed",
            )));
        }

        let mut len_buf = [0u8; 4];
        if let Err(e) = self.stream.read_exact(&mut len_buf) {
            if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock {
                return Err(NetworkError::IoTimeout);
            }
            return Err(NetworkError::IoError(e));
        }

        let frame_len = u32::from_le_bytes(len_buf) as usize;
        if frame_len > self.max_frame_size {
            self.is_closed = true;
            return Err(NetworkError::FrameTooLarge {
                size: frame_len,
                max: self.max_frame_size,
            });
        }

        let mut payload = vec![0u8; frame_len];
        if let Err(e) = self.stream.read_exact(&mut payload) {
            if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock {
                return Err(NetworkError::IoTimeout);
            }
            return Err(NetworkError::IoError(e));
        }

        Ok(payload)
    }

    /// Menuliskan satu bingkai payload dengan prefix 4-byte panjang Little-Endian.
    pub fn write_frame(&mut self, payload: &[u8]) -> Result<(), NetworkError> {
        if self.is_closed {
            return Err(NetworkError::IoError(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Framed connection is closed",
            )));
        }

        let size = payload.len();
        if size > self.max_frame_size {
            return Err(NetworkError::FrameTooLarge {
                size,
                max: self.max_frame_size,
            });
        }

        let len_bytes = (size as u32).to_le_bytes();
        let mut buf = Vec::with_capacity(4 + size);
        buf.extend_from_slice(&len_bytes);
        buf.extend_from_slice(payload);

        if let Err(e) = self.stream.write_all(&buf) {
            if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock {
                return Err(NetworkError::IoTimeout);
            }
            return Err(NetworkError::IoError(e));
        }

        self.stream.flush().map_err(NetworkError::IoError)?;
        Ok(())
    }
}

/// Penerima antrean masuk berbatas (Bounded Ingress Receiver) untuk propagasi backpressure.
#[derive(Debug, Clone)]
pub struct IngressReceiver<T> {
    sender: SyncSender<T>,
}

impl<T> IngressReceiver<T> {
    /// Mengonstruksi IngressReceiver baru yang terhubung ke SyncSender berbatas.
    #[inline]
    pub fn new(sender: SyncSender<T>) -> Self {
        Self { sender }
    }

    /// Mencoba meneruskan paket masuk ke antrean berbatas tanpa memblokir thread (fail-fast / backpressure).
    ///
    /// Jika antrean telah penuh, mengembalikan `NetworkError::ConnectionThrottled`.
    pub fn try_send(&self, packet: T) -> Result<(), NetworkError> {
        self.sender.try_send(packet).map_err(|e| match e {
            std::sync::mpsc::TrySendError::Full(_) => NetworkError::ConnectionThrottled,
            std::sync::mpsc::TrySendError::Disconnected(_) => {
                NetworkError::IoError(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Ingress channel receiver disconnected",
                ))
            }
        })
    }

    /// Meneruskan paket masuk ke antrean berbatas secara memblokir hingga ada slot kosong.
    pub fn send_blocking(&self, packet: T) -> Result<(), NetworkError> {
        self.sender.send(packet).map_err(|_| {
            NetworkError::IoError(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Ingress channel receiver disconnected",
            ))
        })
    }
}
