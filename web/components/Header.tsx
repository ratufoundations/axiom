'use client';

import React from 'react';
import { ShieldCheck, RefreshCw, Bell, AlertTriangle } from 'lucide-react';

interface HeaderProps {
  isLive: boolean;
  onToggleLive: () => void;
  onManualRefresh: () => void;
  alertCount?: number;
  onSimulateQuorumDrop?: () => void;
  onSimulateRoundSkip?: () => void;
}

export const Header: React.FC<HeaderProps> = ({
  isLive,
  onToggleLive,
  onManualRefresh,
  alertCount = 0,
  onSimulateQuorumDrop,
  onSimulateRoundSkip,
}) => {
  return (
    <header
      id="aurion-header"
      className="w-full flex flex-col md:flex-row items-start md:items-center justify-between py-3 px-4 bg-[#0d1117] border-b border-[#30363d] gap-3"
    >
      {/* Left: Brand Icon + Title & Subtitle */}
      <div className="flex items-center gap-3">
        {/* Aurion Geometric Logo */}
        <div className="relative flex items-center justify-center w-8 h-8 rounded bg-[#161b22] border border-[#30363d] shadow-sm overflow-hidden group">
          <svg
            viewBox="0 0 32 32"
            fill="none"
            className="w-6 h-6 transform transition-transform group-hover:scale-110"
          >
            <defs>
              <linearGradient id="aurionGrad" x1="0%" y1="0%" x2="100%" y2="100%">
                <stop offset="0%" stopColor="#58a6ff" />
                <stop offset="50%" stopColor="#388bfd" />
                <stop offset="100%" stopColor="#1f6feb" />
              </linearGradient>
              <linearGradient id="aurionNeon" x1="0%" y1="100%" x2="100%" y2="0%">
                <stop offset="0%" stopColor="#39c5cf" />
                <stop offset="100%" stopColor="#3fb950" />
              </linearGradient>
            </defs>
            <path
              d="M16 4 L28 26 L22 26 L16 14 L10 26 L4 26 Z"
              fill="url(#aurionGrad)"
            />
            <path
              d="M16 11 L21 21 L11 21 Z"
              fill="#0d1117"
            />
            <path
              d="M13 18 L19 18 L16 12 Z"
              fill="url(#aurionNeon)"
            />
          </svg>
        </div>

        <div>
          <h1 className="text-sm md:text-base font-bold tracking-tight text-[#f0f6fc] font-mono flex items-center gap-2">
            AURION SOVEREIGN NETWORK — LIVE BEACON &amp; BLOCKCHAIN EXPLORER
          </h1>
          <p className="text-xs text-[#8b949e] font-mono flex items-center gap-1.5 flex-wrap">
            <span>Live Decentralized Peer Mesh, BFT Consensus, and Block Ledger</span>
            <span className="hidden sm:inline text-[#30363d]">•</span>
            <span className="hidden sm:inline-flex items-center gap-1 text-[11px] text-[#39c5cf] bg-[#39c5cf]/10 px-1.5 py-0.2 rounded border border-[#39c5cf]/30">
              <ShieldCheck className="w-2.5 h-2.5" /> Byzantine Fault Tolerant
            </span>
          </p>
        </div>
      </div>

      {/* Right: Live Status Badges & Controls */}
      <div className="flex items-center gap-2 self-end md:self-auto flex-wrap">
        {/* Active BFT Alerts Badge */}
        {alertCount > 0 ? (
          <div className="flex items-center gap-1.5 px-2 py-1 text-xs font-mono rounded bg-[#f85149]/20 text-[#f85149] border border-[#f85149]/50 shadow-[0_0_10px_rgba(248,81,73,0.3)] animate-pulse">
            <AlertTriangle className="w-3 h-3 text-[#f85149]" />
            <span className="font-bold">{alertCount} BFT Warning{alertCount > 1 ? 's' : ''}</span>
          </div>
        ) : (
          <div className="hidden lg:flex items-center gap-1.5 px-2 py-1 text-[11px] font-mono rounded bg-[#161b22] text-[#8b949e] border border-[#30363d]">
            <span className="text-[#3fb950] font-semibold">2/3+ Quorum</span>
            <span>Commit Rate 98.6%</span>
          </div>
        )}

        <button
          onClick={onManualRefresh}
          title="Refresh Telemetry"
          className="px-2 py-1 text-xs text-[#8b949e] hover:text-[#f0f6fc] hover:bg-[#161b22] border border-[#30363d] rounded flex items-center gap-1 transition-colors cursor-pointer"
        >
          <RefreshCw className="w-3 h-3" />
        </button>

        <button
          onClick={onToggleLive}
          title={isLive ? "Pause Live Stream" : "Resume Live Stream"}
          className={`px-2.5 py-1 text-xs font-mono rounded flex items-center gap-1.5 border transition-all cursor-pointer ${
            isLive
              ? "bg-[#238636]/20 text-[#3fb950] border-[#238636]/60 shadow-[0_0_8px_rgba(63,185,80,0.15)]"
              : "bg-[#21262d] text-[#8b949e] border-[#30363d]"
          }`}
        >
          <span className="relative flex h-2 w-2">
            {isLive && (
              <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-[#3fb950] opacity-75"></span>
            )}
            <span
              className={`relative inline-flex rounded-full h-2 w-2 ${
                isLive ? "bg-[#3fb950]" : "bg-[#8b949e]"
              }`}
            ></span>
          </span>
          <span className="font-semibold tracking-wide">
            {isLive ? "• LIVE" : "• PAUSED"}
          </span>
        </button>

        <div className="px-2.5 py-1 text-xs font-mono rounded flex items-center gap-1.5 bg-[#161b22] text-[#3fb950] border border-[#30363d]">
          <span className="inline-flex rounded-full h-2 w-2 bg-[#3fb950]"></span>
          <span className="font-semibold tracking-wide">• CONNECTED</span>
        </div>
      </div>
    </header>
  );
};
