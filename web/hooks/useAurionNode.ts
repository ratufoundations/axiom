'use client';

import { useState, useEffect, useRef } from 'react';
import { BlockItem, TransactionItem, MetricCardData } from '@/types';
import { INITIAL_BLOCKS, INITIAL_TRANSACTIONS } from '@/data/initialData';

interface UseAurionNodeOptions {
  rpcUrl?: string; // default: http://127.0.0.1:9545
  wsUrl?: string;  // default: ws://127.0.0.1:9545/ws
  isLive?: boolean;
}

export function useAurionNode({
  rpcUrl = 'http://127.0.0.1:9545',
  wsUrl = 'ws://127.0.0.1:9545/ws',
  isLive = true,
}: UseAurionNodeOptions = {}) {
  const [connected, setConnected] = useState(false);
  const [metrics, setMetrics] = useState<MetricCardData>({
    totalPeers: 4,
    activeValidators: 4,
    fullNodes: 0,
    blockHeight: 0,
    bootnodeUptime: '0m',
    performanceRate: 100,
    tps: 0,
    mempool: 0,
    bftQuorum: '100%',
    bftRound: 0,
  });
  const [blocks, setBlocks] = useState<BlockItem[]>(INITIAL_BLOCKS);
  const [transactions, setTransactions] = useState<TransactionItem[]>(INITIAL_TRANSACTIONS);
  const wsRef = useRef<WebSocket | null>(null);

  // 1. Polling State via JSON-RPC 2.0 (aur_blockHeight & aur_chainId)
  useEffect(() => {
    if (!isLive) return;

    const fetchMetrics = async () => {
      try {
        const resHeight = await fetch(rpcUrl, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            jsonrpc: '2.0',
            method: 'aur_blockHeight',
            id: 1,
          }),
        });

        if (resHeight.ok) {
          const json = await resHeight.json();
          if (json.result) {
            setMetrics((prev) => ({
              ...prev,
              blockHeight: json.result.sequence_number,
              bftRound: json.result.epoch,
            }));
            setConnected(true);
          }
        }
      } catch {
        setConnected(false);
      }
    };

    fetchMetrics();
    const interval = setInterval(fetchMetrics, 3000);
    return () => clearInterval(interval);
  }, [rpcUrl, isLive]);

  // 2. Real-Time WebSocket Streaming (/ws)
  useEffect(() => {
    if (!isLive) {
      wsRef.current?.close();
      return;
    }

    try {
      const ws = new WebSocket(wsUrl);
      wsRef.current = ws;

      ws.onopen = () => {
        // Berlangganan mutasi transaksi baru
        ws.send(
          JSON.stringify({
            jsonrpc: '2.0',
            method: 'aur_subscribe',
            params: ['newMutations'],
            id: 1,
          })
        );
      };

      ws.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data);
          if (data.method === 'aur_subscription' && data.params?.result) {
            const mut = data.params.result;
            // Konversi atomic unit ke nilai AUR desimal
            const amountAur = Number((BigInt(mut.amount) / BigInt('10000000000')).toString());

            const newTx: TransactionItem = {
              id: `tx-${mut.sequence_number}`,
              from: mut.sender,
              to: mut.recipient,
              displayFrom: mut.sender.slice(0, 8),
              displayTo: mut.recipient.slice(0, 8),
              amount: amountAur,
              gasFee: '0.001 AUR',
              timeAgo: 'Baru saja',
              type: 'transfer',
              status: 'confirmed',
            };

            setTransactions((prev) => [newTx, ...prev.slice(0, 9)]);
          }
        } catch {
          // Abaikan frame non-JSON (seperti pong/ping)
        }
      };

      ws.onclose = () => {
        setConnected(false);
      };

      return () => {
        ws.close();
      };
    } catch {
      setConnected(false);
    }
  }, [wsUrl, isLive]);

  return { connected, metrics, blocks, transactions, setBlocks, setTransactions };
}
