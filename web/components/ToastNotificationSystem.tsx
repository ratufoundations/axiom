'use client';

import React, { useEffect } from 'react';
import {
  AlertTriangle,
  ShieldAlert,
  FastForward,
  X,
  Clock,
  CheckCircle2,
  Trash2,
  BellRing,
} from 'lucide-react';
import { BFTAlert } from '@/types';

interface ToastNotificationSystemProps {
  alerts: BFTAlert[];
  onDismiss: (id: string) => void;
  onClearAll: () => void;
  onSimulateQuorumDrop: () => void;
  onSimulateRoundSkip: () => void;
}

export const ToastNotificationSystem: React.FC<ToastNotificationSystemProps> = ({
  alerts,
  onDismiss,
  onClearAll,
  onSimulateQuorumDrop,
  onSimulateRoundSkip,
}) => {
  // Auto-dismiss timers for alerts
  useEffect(() => {
    if (alerts.length === 0) return;

    const timers = alerts.map((alert) => {
      const duration = alert.autoCloseMs || 9000;
      return setTimeout(() => {
        onDismiss(alert.id);
      }, duration);
    });

    return () => {
      timers.forEach((t) => clearTimeout(t));
    };
  }, [alerts, onDismiss]);

  return (
    <div
      id="toast-notification-system"
      className="fixed top-14 right-3 z-50 flex flex-col gap-2 max-w-sm w-full pointer-events-none font-mono"
    >
      {/* Alert Cards Container */}
      <div className="flex flex-col gap-2 pointer-events-auto">
        {alerts.map((alert) => {
          const isCritical = alert.severity === 'critical';
          const isRoundSkip = alert.type === 'ROUND_SKIP';

          // Color themes based on warning type
          const borderStyle = isCritical
            ? 'border-[#f85149] shadow-[0_0_15px_rgba(248,81,73,0.3)] bg-[#161b22]'
            : isRoundSkip
            ? 'border-[#d29922] shadow-[0_0_15px_rgba(210,153,34,0.25)] bg-[#161b22]'
            : 'border-[#f85149] shadow-[0_0_15px_rgba(248,81,73,0.25)] bg-[#161b22]';

          const badgeBg = isCritical
            ? 'bg-[#f85149]/20 text-[#f85149] border-[#f85149]/50'
            : isRoundSkip
            ? 'bg-[#d29922]/20 text-[#d29922] border-[#d29922]/50'
            : 'bg-[#f85149]/20 text-[#f85149] border-[#f85149]/50';

          return (
            <div
              key={alert.id}
              className={`p-3 rounded border ${borderStyle} text-xs text-[#c9d1d9] relative overflow-hidden backdrop-blur-md transition-all animate-in fade-in slide-in-from-top-2 duration-300`}
            >
              {/* Top Accent Warning Bar */}
              <div
                className={`absolute top-0 left-0 right-0 h-[2px] ${
                  isCritical
                    ? 'bg-[#f85149]'
                    : isRoundSkip
                    ? 'bg-[#d29922]'
                    : 'bg-[#f85149]'
                }`}
              />

              {/* Toast Header */}
              <div className="flex items-start justify-between gap-2 mb-1.5">
                <div className="flex items-center gap-1.5 flex-wrap">
                  {isCritical ? (
                    <ShieldAlert className="w-4 h-4 text-[#f85149] shrink-0 animate-pulse" />
                  ) : isRoundSkip ? (
                    <FastForward className="w-4 h-4 text-[#d29922] shrink-0 animate-bounce" />
                  ) : (
                    <AlertTriangle className="w-4 h-4 text-[#d29922] shrink-0" />
                  )}

                  <span className={`text-[10px] font-bold px-1.5 py-0.5 rounded border uppercase tracking-wider ${badgeBg}`}>
                    {alert.title}
                  </span>
                </div>

                <div className="flex items-center gap-1.5 shrink-0">
                  <span className="text-[10px] text-[#8b949e] flex items-center gap-0.5">
                    <Clock className="w-2.5 h-2.5" />
                    {alert.timestamp}
                  </span>
                  <button
                    onClick={() => onDismiss(alert.id)}
                    className="p-0.5 text-[#8b949e] hover:text-[#f0f6fc] hover:bg-[#30363d] rounded transition-colors cursor-pointer"
                    title="Dismiss alert"
                  >
                    <X className="w-3.5 h-3.5" />
                  </button>
                </div>
              </div>

              {/* Toast Message Body */}
              <p className="text-[11px] leading-relaxed text-[#f0f6fc]">
                {alert.message}
              </p>

              {/* Specific Metric Badge Tag */}
              <div className="mt-2 pt-1.5 border-t border-[#30363d]/60 flex items-center justify-between text-[10px] text-[#8b949e]">
                {alert.type === 'QUORUM_DROP' && (
                  <span className="text-[#f85149] font-bold">
                    Quorum: {alert.quorumValue}% (&lt; 66.7% BFT Limit)
                  </span>
                )}
                {alert.type === 'ROUND_SKIP' && (
                  <span className="text-[#d29922] font-bold">
                    Skipped to Round #{alert.roundNumber} (Timeout 1500ms)
                  </span>
                )}
                <span className="text-[#8b949e] text-[9px] uppercase tracking-wider">
                  BFT Consensus Watchdog
                </span>
              </div>
            </div>
          );
        })}
      </div>

      {/* Clear All action button when multiple alerts exist */}
      {alerts.length > 1 && (
        <div className="flex justify-end pointer-events-auto">
          <button
            onClick={onClearAll}
            className="px-2.5 py-1 text-[10px] bg-[#161b22] hover:bg-[#21262d] text-[#8b949e] hover:text-[#f0f6fc] border border-[#30363d] rounded flex items-center gap-1.5 shadow-md transition-colors cursor-pointer"
          >
            <Trash2 className="w-3 h-3 text-[#8b949e]" />
            <span>Clear All Warnings ({alerts.length})</span>
          </button>
        </div>
      )}

      {/* Operator BFT Warning Simulation Bar (Compact Quick Test) */}
      <div className="bg-[#161b22]/95 border border-[#30363d] p-1.5 rounded shadow-lg backdrop-blur-xs flex items-center justify-between text-[10px] pointer-events-auto gap-2">
        <span className="text-[#8b949e] flex items-center gap-1 shrink-0 font-semibold">
          <BellRing className="w-3 h-3 text-[#39c5cf]" />
          <span>BFT Alert Test:</span>
        </span>
        <div className="flex items-center gap-1.5">
          <button
            onClick={onSimulateQuorumDrop}
            className="px-2 py-0.5 bg-[#f85149]/15 hover:bg-[#f85149]/25 text-[#f85149] border border-[#f85149]/40 rounded transition-colors cursor-pointer text-[10px] font-semibold"
            title="Simulate Quorum falling below 66.7%"
          >
            Drop &lt;67%
          </button>
          <button
            onClick={onSimulateRoundSkip}
            className="px-2 py-0.5 bg-[#d29922]/15 hover:bg-[#d29922]/25 text-[#d29922] border border-[#d29922]/40 rounded transition-colors cursor-pointer text-[10px] font-semibold"
            title="Simulate Proposer Timeout & Round Skip"
          >
            Round Skip
          </button>
        </div>
      </div>
    </div>
  );
};
