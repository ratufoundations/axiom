'use client';

import React from 'react';
import { Activity, ShieldCheck, ArrowRight, CheckCircle2 } from 'lucide-react';
import { BlockItem, TransactionItem } from '@/types';

interface BlockchainActivityPanelProps {
  blocks: BlockItem[];
  transactions: TransactionItem[];
  onSelectBlock?: (block: BlockItem) => void;
  onSelectTransaction?: (tx: TransactionItem) => void;
}

export const BlockchainActivityPanel: React.FC<BlockchainActivityPanelProps> = ({
  blocks,
  transactions,
  onSelectBlock,
  onSelectTransaction,
}) => {
  return (
    <div
      id="panel-blockchain-activity"
      className="flex flex-col gap-2.5 h-full"
    >
      {/* 1. BLOK TERBARU (LATEST BLOCKS) - BFT Consensus & Proposer */}
      <div
        id="section-latest-blocks"
        className="bg-[#161b22] border border-[#30363d] rounded flex flex-col flex-1 overflow-hidden"
      >
        <div className="flex items-center justify-between px-3 py-2 border-b border-[#30363d] bg-[#161b22]">
          <div className="flex items-center gap-2">
            <span className="text-xs font-mono font-bold tracking-wide text-[#f0f6fc] uppercase">
              BLOK TERBARU <span className="text-[#8b949e] font-normal">(LATEST BLOCKS)</span>
            </span>
          </div>
          <span className="text-[10px] font-mono text-[#3fb950] flex items-center gap-1 bg-[#238636]/15 px-1.5 py-0.5 rounded border border-[#238636]/30">
            <ShieldCheck className="w-3 h-3" />
            <span>BFT Quorum Finalized</span>
          </span>
        </div>

        <div className="flex-1 overflow-y-auto divide-y divide-[#30363d]/50 p-1">
          {blocks.slice(0, 4).map((block, idx) => {
            const proposerLabel = block.proposerShort || block.minerShort || '0x8b3...';
            const commitRate = block.quorumPercentage || '98.6%';

            return (
              <div
                key={`${block.height}-${idx}`}
                onClick={() => onSelectBlock?.(block)}
                className="flex items-center justify-between px-2.5 py-2 text-xs font-mono hover:bg-[#21262d]/60 cursor-pointer transition-colors rounded group"
              >
                {/* Left: Block number & Proposer */}
                <div className="flex items-center gap-2">
                  <span className="text-[#58a6ff] font-semibold group-hover:underline">
                    Block {block.heightFormatted}
                  </span>
                  <span className="text-[#8b949e] text-[11px] truncate max-w-[150px]">
                    oleh Proposer <span className="text-[#c9d1d9]">{proposerLabel}</span>
                  </span>
                </div>

                {/* Right: Specs (Txs, Commits, Time) */}
                <div className="flex items-center gap-2 sm:gap-3 text-[11px] text-[#8b949e]">
                  <span>{block.txs} Txs</span>
                  <span className="hidden sm:inline text-[#3fb950] bg-[#3fb950]/10 px-1 rounded text-[10px]">
                    +2/3 ({commitRate})
                  </span>
                  <span className="text-[#8b949e]/80 min-w-[50px] text-right">
                    {block.ageText}
                  </span>
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {/* 2. TRANSAKSI TERBARU (LATEST TRANSACTIONS) */}
      <div
        id="section-latest-transactions"
        className="bg-[#161b22] border border-[#30363d] rounded flex flex-col flex-1 overflow-hidden"
      >
        <div className="flex items-center justify-between px-3 py-2 border-b border-[#30363d] bg-[#161b22]">
          <span className="text-xs font-mono font-bold tracking-wide text-[#f0f6fc] uppercase">
            TRANSAKSI TERBARU <span className="text-[#8b949e] font-normal">(LATEST TRANSACTIONS)</span>
          </span>
          <Activity className="w-3.5 h-3.5 text-[#39c5cf]" />
        </div>

        <div className="flex-1 overflow-y-auto divide-y divide-[#30363d]/50 p-1">
          {transactions.slice(0, 4).map((tx, idx) => {
            const barColors = [
              'bg-[#3fb950]',
              'bg-[#39c5cf]',
              'bg-[#d29922]',
              'bg-[#58a6ff]',
            ];
            const indicatorColor = barColors[idx % barColors.length];

            return (
              <div
                key={`${tx.id}-${idx}`}
                onClick={() => onSelectTransaction?.(tx)}
                className="flex items-center justify-between px-2.5 py-2 text-xs font-mono hover:bg-[#21262d]/60 cursor-pointer transition-colors rounded group relative pl-3"
              >
                {/* Colored Left Vertical Accent Bar */}
                <div
                  className={`absolute left-1 top-2 bottom-2 w-[2px] rounded-full ${indicatorColor}`}
                />

                {/* Sender -> Receiver Hash */}
                <div className="flex items-center gap-1.5 truncate max-w-[210px] md:max-w-[230px]">
                  <span className="text-[#8b949e] text-[11px]">Alamat hash:</span>
                  <span className="text-[#39c5cf] text-[11px] font-medium truncate">
                    {tx.displayFrom}
                  </span>
                  <ArrowRight className="w-2.5 h-2.5 text-[#8b949e] shrink-0" />
                  <span className="text-[#f0f6fc] text-[11px] truncate">
                    {tx.displayTo}
                  </span>
                </div>

                {/* Amount & Gas Fee */}
                <div className="flex items-center gap-3 text-[11px] text-[#8b949e] shrink-0">
                  <span className="text-[#f0f6fc] font-medium">
                    {tx.amount} koin AUR
                  </span>
                  <span className="text-[#8b949e] hidden sm:inline text-[10px]">
                    gas fee
                  </span>
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
};
