'use client';

import React from 'react';
import { Network, ShieldCheck, Box, Info, CheckCircle2 } from 'lucide-react';
import { MetricCardData } from '@/types';
import { BFTConsensusHealthChart } from './BFTConsensusHealthChart';

interface MetricsGridProps {
  metrics: MetricCardData;
  onQuorumDrop?: (val: number) => void;
  onRoundSkip?: (round: number) => void;
}

export const MetricsGrid: React.FC<MetricsGridProps> = ({
  metrics,
  onQuorumDrop,
  onRoundSkip,
}) => {
  const activeValidators = metrics.activeValidators ?? metrics.activeMiners ?? 142;

  return (
    <div id="metrics-section" className="p-3 pb-2.5 flex flex-col gap-2.5 bg-[#0d1117]">
      {/* Upper 6 Core Metric Cards */}
      <div
        id="metrics-grid"
        className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-2.5"
      >
        {/* 1. TOTAL PEER AKTIF */}
        <div
          id="card-total-peers"
          className="bg-[#161b22] border border-[#30363d] rounded p-3 flex flex-col justify-between relative overflow-hidden"
        >
          <div className="flex items-center justify-between">
            <span className="text-[11px] font-mono font-medium tracking-wide text-[#8b949e] uppercase">
              TOTAL PEER AKTIF
            </span>
            <Network className="w-3.5 h-3.5 text-[#58a6ff]" />
          </div>

          <div className="mt-1">
            <span className="text-xl md:text-2xl font-bold font-mono text-[#f0f6fc] tracking-tight">
              {metrics.totalPeers.toLocaleString()}
            </span>
          </div>

          {/* Blue Sparkline Chart */}
          <div className="h-6 w-full mt-2">
            <svg
              viewBox="0 0 100 24"
              preserveAspectRatio="none"
              className="w-full h-full overflow-visible"
            >
              <defs>
                <linearGradient id="blueSparkGrad" x1="0%" y1="0%" x2="0%" y2="100%">
                  <stop offset="0%" stopColor="#388bfd" stopOpacity="0.35" />
                  <stop offset="100%" stopColor="#388bfd" stopOpacity="0.0" />
                </linearGradient>
              </defs>
              <path
                d="M 0 18 Q 15 19, 25 15 T 45 16 T 65 10 T 80 12 T 95 6 T 100 7 L 100 24 L 0 24 Z"
                fill="url(#blueSparkGrad)"
              />
              <path
                d="M 0 18 Q 15 19, 25 15 T 45 16 T 65 10 T 80 12 T 95 6 T 100 7"
                fill="none"
                stroke="#58a6ff"
                strokeWidth="1.8"
                strokeLinecap="round"
              />
            </svg>
          </div>
        </div>

        {/* 2. VALIDATOR BFT AKTIF */}
        <div
          id="card-active-validators"
          className="bg-[#161b22] border border-[#30363d] rounded p-3 flex flex-col justify-between relative overflow-hidden"
        >
          <div className="flex items-center justify-between">
            <span className="text-[11px] font-mono font-medium tracking-wide text-[#8b949e] uppercase flex items-center gap-1">
              <span>VALIDATOR BFT</span>
            </span>
            <ShieldCheck className="w-3.5 h-3.5 text-[#3fb950]" />
          </div>

          <div className="mt-1 flex items-baseline justify-between">
            <span className="text-xl md:text-2xl font-bold font-mono text-[#f0f6fc] tracking-tight">
              {activeValidators.toLocaleString()}
            </span>
            <span className="text-[10px] text-[#3fb950] font-mono">Quorum 67%+</span>
          </div>

          {/* Green Mini Bar Chart for Voting Weight / Round Distribution */}
          <div className="h-6 w-full mt-2 flex items-end justify-between gap-[2px]">
            {[14, 18, 20, 16, 22, 19, 23, 21, 22, 24, 21, 23, 19, 22, 24, 23, 21, 24, 22, 24].map(
              (height, i) => (
                <div
                  key={i}
                  style={{ height: `${(height / 24) * 100}%` }}
                  className="w-full bg-[#3fb950] rounded-xs opacity-85 hover:opacity-100 transition-opacity"
                  title={`BFT Validator Partition #${i + 1}`}
                />
              )
            )}
          </div>
        </div>

        {/* 3. FULL NODES */}
        <div
          id="card-full-nodes"
          className="bg-[#161b22] border border-[#30363d] rounded p-3 flex flex-col justify-between relative overflow-hidden"
        >
          <div className="flex items-center justify-between">
            <span className="text-[11px] font-mono font-medium tracking-wide text-[#8b949e] uppercase">
              FULL NODES
            </span>
            <Box className="w-3.5 h-3.5 text-[#d29922]" />
          </div>

          <div className="mt-1">
            <span className="text-xl md:text-2xl font-bold font-mono text-[#f0f6fc] tracking-tight">
              {metrics.fullNodes.toLocaleString()}
            </span>
          </div>

          {/* Orange Sparkline Chart */}
          <div className="h-6 w-full mt-2">
            <svg
              viewBox="0 0 100 24"
              preserveAspectRatio="none"
              className="w-full h-full overflow-visible"
            >
              <defs>
                <linearGradient id="orangeSparkGrad" x1="0%" y1="0%" x2="0%" y2="100%">
                  <stop offset="0%" stopColor="#d29922" stopOpacity="0.35" />
                  <stop offset="100%" stopColor="#d29922" stopOpacity="0.0" />
                </linearGradient>
              </defs>
              <path
                d="M 0 16 Q 15 15, 30 18 T 55 12 T 75 14 T 90 9 T 100 11 L 100 24 L 0 24 Z"
                fill="url(#orangeSparkGrad)"
              />
              <path
                d="M 0 16 Q 15 15, 30 18 T 55 12 T 75 14 T 90 9 T 100 11"
                fill="none"
                stroke="#d29922"
                strokeWidth="1.8"
                strokeLinecap="round"
              />
            </svg>
          </div>
        </div>

        {/* 4. TINGGI BLOK (BLOCK HEIGHT) */}
        <div
          id="card-block-height"
          className="bg-[#161b22] border border-[#30363d] rounded p-3 flex flex-col justify-between relative overflow-hidden"
        >
          <div className="flex items-center justify-between">
            <span className="text-[11px] font-mono font-medium tracking-wide text-[#8b949e] uppercase">
              TINGGI BLOK (BLOCK HEIGHT)
            </span>
          </div>

          <div className="mt-1 my-auto">
            <span className="text-xl md:text-2xl font-bold font-mono text-[#f0f6fc] tracking-tight flex items-baseline gap-1">
              <span className="text-[#388bfd]">#</span>
              {metrics.blockHeight.toLocaleString()}
            </span>
          </div>

          <div className="text-[10px] font-mono text-[#8b949e] flex items-center justify-between mt-1">
            <span>BFT Finality: Instan</span>
            <span className="text-[#3fb950] flex items-center gap-0.5">
              <CheckCircle2 className="w-2.5 h-2.5" /> Finalized
            </span>
          </div>
        </div>

        {/* 5. UPTIME BOOTNODE */}
        <div
          id="card-uptime-bootnode"
          className="bg-[#161b22] border border-[#30363d] rounded p-3 flex flex-col justify-between relative overflow-hidden"
        >
          <div className="flex items-center justify-between">
            <span className="text-[11px] font-mono font-medium tracking-wide text-[#8b949e] uppercase">
              UPTIME BOOTNODE
            </span>
            <Info className="w-3.5 h-3.5 text-[#39c5cf]" />
          </div>

          <div className="mt-1">
            <span className="text-xl md:text-2xl font-bold font-mono text-[#f0f6fc] tracking-tight">
              {metrics.bootnodeUptime}
            </span>
          </div>

          <div className="text-[11px] font-mono text-[#8b949e] mt-1 flex items-center justify-between">
            <span>Info Performa:</span>
            <span className="text-[#39c5cf] font-semibold">{metrics.performanceRate}%</span>
          </div>
        </div>

        {/* 6. TRANSAKSI PER DETIK (TPS) */}
        <div
          id="card-tps"
          className="bg-[#161b22] border border-[#30363d] rounded p-3 flex flex-col justify-between relative overflow-hidden"
        >
          <div className="flex items-center justify-between">
            <span className="text-[11px] font-mono font-medium tracking-wide text-[#8b949e] uppercase">
              TRANSAKSI PER DETIK (TPS)
            </span>
          </div>

          <div className="mt-1">
            <span className="text-xl md:text-2xl font-bold font-mono text-[#f0f6fc] tracking-tight">
              {metrics.tps.toFixed(1)} <span className="text-xs text-[#8b949e] font-normal">tx/s</span>
            </span>
          </div>

          <div className="text-[11px] font-mono text-[#8b949e] mt-1 flex items-center justify-between">
            <span>Mempool:</span>
            <span className="text-[#d29922] font-semibold">{metrics.mempool.toLocaleString()}</span>
          </div>
        </div>
      </div>

      {/* Real-Time Visual Component: BFT Consensus Health & Quorum Telemetry (Recharts Line Chart, 10m History) */}
      <BFTConsensusHealthChart
        activeValidators={activeValidators}
        currentQuorum={98.6}
        currentLatency={245}
        onQuorumDrop={onQuorumDrop}
        onRoundSkip={onRoundSkip}
      />
    </div>
  );
};
