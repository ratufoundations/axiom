'use client';

import React, { useState } from 'react';
import { Header } from '@/components/Header';
import { MetricsGrid } from '@/components/MetricsGrid';
import { PeerMapPanel } from '@/components/PeerMapPanel';
import { BlockchainActivityPanel } from '@/components/BlockchainActivityPanel';
import { PeerTelemetryTable } from '@/components/PeerTelemetryTable';
import { InspectorModal } from '@/components/InspectorModal';
import { ToastNotificationSystem } from '@/components/ToastNotificationSystem';

import { useAurionNode } from '@/hooks/useAurionNode';
import { INITIAL_PEERS, INITIAL_BLOCKS, INITIAL_TRANSACTIONS } from '@/data/initialData';
import { PeerNode, BlockItem, TransactionItem, BFTAlert } from '@/types';

export default function ExplorerPage() {
  const [isLive, setIsLive] = useState(true);

  // Hook live telemetri dari node Ratu Aurion
  const { connected, metrics, blocks: liveBlocks, transactions: liveTxs } = useAurionNode({
    isLive,
  });

  const [peers, setPeers] = useState<PeerNode[]>(INITIAL_PEERS);
  const blocks = liveBlocks.length > 0 ? liveBlocks : INITIAL_BLOCKS;
  const transactions = liveTxs.length > 0 ? liveTxs : INITIAL_TRANSACTIONS;

  // Modal inspection states
  const [selectedBlock, setSelectedBlock] = useState<BlockItem | null>(null);
  const [selectedTx, setSelectedTx] = useState<TransactionItem | null>(null);
  const [selectedPeer, setSelectedPeer] = useState<PeerNode | null>(null);

  // Toast alerts
  const [alerts, setAlerts] = useState<BFTAlert[]>([
    {
      id: 'bft-status',
      type: 'BFT_WARNING',
      title: 'BFT WATCHDOG: AKTIF',
      message: 'Byzantine Fault Tolerance watchdog memantau kuorum 2/3+ (66,7%) dan rotasi ronde.',
      severity: 'info',
      timestamp: new Date().toLocaleTimeString(),
      timeMs: Date.now(),
      autoCloseMs: 8000,
    },
  ]);

  const handleSimulateQuorumDrop = () => {
    const alert: BFTAlert = {
      id: `alert-drop-${Date.now()}`,
      type: 'QUORUM_DROP',
      title: 'PERINGATAN: DEFISIT KUORUM BFT (<66,7%)',
      message: 'Partisipasi validator turun menjadi 64.2%. Blok pre-commit tertunda.',
      severity: 'critical',
      timestamp: new Date().toLocaleTimeString(),
      timeMs: Date.now(),
      quorumValue: 64.2,
      autoCloseMs: 10000,
    };
    setAlerts((prev) => [alert, ...prev]);
  };

  const handleSimulateRoundSkip = () => {
    const alert: BFTAlert = {
      id: `alert-skip-${Date.now()}`,
      type: 'ROUND_SKIP',
      title: 'PERINGATAN: LEADER TIMEOUT / ROUND SKIP',
      message: 'Leader ronde saat ini gagal mengusulkan blok sebelum batas waktu. View-change dipicu.',
      severity: 'warning',
      timestamp: new Date().toLocaleTimeString(),
      timeMs: Date.now(),
      roundNumber: (metrics.bftRound ?? 0) + 1,
      autoCloseMs: 10000,
    };
    setAlerts((prev) => [alert, ...prev]);
  };

  return (
    <div className="min-h-screen bg-[#0d1117] text-[#c9d1d9] flex flex-col font-mono relative">
      <Header
        isLive={isLive}
        onToggleLive={() => setIsLive(!isLive)}
        onManualRefresh={() => {}}
        alertCount={alerts.length}
        onSimulateQuorumDrop={handleSimulateQuorumDrop}
        onSimulateRoundSkip={handleSimulateRoundSkip}
      />

      <ToastNotificationSystem
        alerts={alerts}
        onDismiss={(id) => setAlerts((prev) => prev.filter((a) => a.id !== id))}
        onClearAll={() => setAlerts([])}
        onSimulateQuorumDrop={handleSimulateQuorumDrop}
        onSimulateRoundSkip={handleSimulateRoundSkip}
      />

      <MetricsGrid
        metrics={metrics}
        onQuorumDrop={handleSimulateQuorumDrop}
        onRoundSkip={handleSimulateRoundSkip}
      />

      <main className="flex-1 p-3 pt-0 grid grid-cols-1 lg:grid-cols-12 gap-2.5">
        <div className="lg:col-span-7 h-full min-h-[360px]">
          <PeerMapPanel
            avgLatency={42}
            validatorCount={metrics.activeValidators}
            fullNodeCount={metrics.fullNodes}
          />
        </div>

        <div className="lg:col-span-5 h-full">
          <BlockchainActivityPanel
            blocks={blocks}
            transactions={transactions}
            onSelectBlock={(b) => setSelectedBlock(b)}
            onSelectTransaction={(t) => setSelectedTx(t)}
          />
        </div>

        <div className="col-span-12">
          <PeerTelemetryTable
            peers={peers}
            onSelectPeer={(p) => setSelectedPeer(p)}
          />
        </div>
      </main>

      <InspectorModal
        selectedBlock={selectedBlock}
        selectedTx={selectedTx}
        selectedPeer={selectedPeer}
        onClose={() => {
          setSelectedBlock(null);
          setSelectedTx(null);
          setSelectedPeer(null);
        }}
      />
    </div>
  );
}
