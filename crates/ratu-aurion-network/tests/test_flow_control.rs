//! Integration tests for Bounded TCP Flow Control & Rate Limiting (OPT-NETWORK-01).

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::sync_channel;
use std::time::Duration;

use ratu_aurion_network::error::NetworkError;
use ratu_aurion_network::framed::{FramedStream, IngressReceiver, MAX_FRAME_SIZE};
use ratu_aurion_network::rate_limiter::TokenBucketLimiter;

#[test]
fn test_token_bucket_rate_limiter_burst_and_refill() {
    // Capacity 10 tokens, fill rate 10 tokens/sec (1 token per 100 ms), starting at t = 0 ms
    let mut limiter = TokenBucketLimiter::new(10, 10, 0);
    assert_eq!(limiter.tokens(), 10);

    // Consume 10 tokens immediately; assert all 10 return Ok(())
    for _ in 0..10 {
        assert_eq!(limiter.try_consume(0, 1), Ok(()));
    }
    assert_eq!(limiter.tokens(), 0);

    // Attempt 11th token consumption immediately; assert returns RateLimitExceeded
    let res_11th = limiter.try_consume(0, 1);
    assert_eq!(
        res_11th,
        Err(NetworkError::RateLimitExceeded {
            peer: "default".to_string()
        })
    );

    // Advance simulated time by 300 ms; assert exactly 3 tokens are replenished
    limiter.refill_at(300);
    assert_eq!(limiter.tokens(), 3);

    // Can consume exactly 3 tokens at t = 300 ms
    assert_eq!(limiter.try_consume(300, 1), Ok(()));
    assert_eq!(limiter.try_consume(300, 1), Ok(()));
    assert_eq!(limiter.try_consume(300, 1), Ok(()));
    assert_eq!(limiter.tokens(), 0);

    // Next consume fails
    assert_eq!(
        limiter.try_consume(300, 1),
        Err(NetworkError::RateLimitExceeded {
            peer: "default".to_string()
        })
    );
}

#[test]
fn test_framed_stream_rejects_oversized_frame() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let client = TcpStream::connect(addr).unwrap();
    let (server, _) = listener.accept().unwrap();

    let mut framed_reader = FramedStream::from_tcp(server, 2_000).unwrap();
    let mut framed_writer = FramedStream::from_tcp(client, 2_000).unwrap();

    // 1. Attempt to write a frame of 65,537 bytes (> 64 KB)
    let oversized_payload = vec![0xAAu8; MAX_FRAME_SIZE + 1];
    let write_res = framed_writer.write_frame(&oversized_payload);
    assert_eq!(
        write_res,
        Err(NetworkError::FrameTooLarge {
            size: MAX_FRAME_SIZE + 1,
            max: MAX_FRAME_SIZE,
        })
    );

    // 2. Client sends an oversized length header directly to test reader rejection
    let raw_client = framed_writer.into_inner();
    let mut raw_client_clone = raw_client.try_clone().unwrap();
    let oversized_len = ((MAX_FRAME_SIZE + 1) as u32).to_le_bytes();
    raw_client_clone.write_all(&oversized_len).unwrap();
    raw_client_clone.flush().unwrap();

    // Assert framed decoder returns FrameTooLarge and marks connection closed
    let read_res = framed_reader.read_frame();
    assert_eq!(
        read_res,
        Err(NetworkError::FrameTooLarge {
            size: MAX_FRAME_SIZE + 1,
            max: MAX_FRAME_SIZE,
        })
    );
    assert!(framed_reader.is_closed());
}

#[test]
fn test_slow_read_socket_timeout_enforcement() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let mut client = TcpStream::connect(addr).unwrap();
    let (server, _) = listener.accept().unwrap();

    // Server socket with read timeout 500 ms
    let mut server_framed = FramedStream::from_tcp(server, 500).unwrap();

    // Client sends only 1 byte (incomplete 4-byte header), then halts
    client.write_all(&[0x01]).unwrap();
    client.flush().unwrap();

    let start = std::time::Instant::now();
    let read_res = server_framed.read_frame();
    let elapsed = start.elapsed();

    // Assert server read operation terminates with IoTimeout without blocking indefinitely
    assert_eq!(read_res, Err(NetworkError::IoTimeout));
    assert!(elapsed >= Duration::from_millis(400));
}

#[test]
fn test_bounded_ingress_backpressure_propagation() {
    // Ingress channel of bounded capacity 4
    let (tx, rx) = sync_channel::<Vec<u8>>(4);
    let receiver = IngressReceiver::new(tx);

    // Send 4 valid network packets; assert all 4 enter channel cleanly
    for i in 0..4 {
        let packet = vec![i as u8; 64];
        assert_eq!(receiver.try_send(packet), Ok(()));
    }

    // Send 5th packet with downstream consumer paused; assert backpressure rejection
    let packet_5th = vec![0x99u8; 64];
    let res_5th = receiver.try_send(packet_5th.clone());
    assert_eq!(res_5th, Err(NetworkError::ConnectionThrottled));

    // Downstream consumer unblocks 1 packet
    let popped = rx.recv().unwrap();
    assert_eq!(popped[0], 0);

    // Now 5th packet enters channel successfully
    assert_eq!(receiver.try_send(packet_5th), Ok(()));
}
