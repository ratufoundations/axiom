'use client';

import React, { useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Check, Copy, ShieldCheck, Box, Activity, Server } from 'lucide-react';
import { BlockItem, TransactionItem, PeerNode } from '@/types';

interface InspectorModalProps {
  selectedBlock: BlockItem | null;
  selectedTx: TransactionItem | null;
  selectedPeer: PeerNode | null;
  onClose: () => void;
}

export function InspectorModal({
  selectedBlock,
  selectedTx,
  selectedPeer,
  onClose,
}: InspectorModalProps) {
  const [copied, setCopied] = useState(false);
  const isOpen = Boolean(selectedBlock || selectedTx || selectedPeer);

  const copyText = (val: string) => {
    navigator.clipboard.writeText(val);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <Dialog open={isOpen} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="bg-[#161b22] border-[#30363d] text-[#c9d1d9] font-mono sm:max-w-[550px]">
        <DialogHeader className="border-b border-[#30363d] pb-3">
          <DialogTitle className="flex items-center gap-2 text-[#f0f6fc] text-sm">
            {selectedBlock && <Box className="w-4 h-4 text-[#58a6ff]" />}
            {selectedTx && <Activity className="w-4 h-4 text-[#39c5cf]" />}
            {selectedPeer && <Server className="w-4 h-4 text-[#3fb950]" />}
            <span>
              {selectedBlock
                ? `Rincian Blok BFT ${selectedBlock.heightFormatted}`
                : selectedTx
                ? 'Rincian Mutasi Transaksi'
                : 'Telemetri Simpul Peer (BFT)'}
            </span>
          </DialogTitle>
        </DialogHeader>

        {/* Konten Detail Blok */}
        {selectedBlock && (
          <div className="space-y-3 pt-2 text-xs">
            <div>
              <div className="text-[11px] text-[#8b949e] mb-1">HASH BLOK</div>
              <div className="bg-[#0d1117] p-2 rounded border border-[#30363d] text-[#58a6ff] break-all flex items-center justify-between">
                <span>{selectedBlock.hash}</span>
                <Button
                  size="icon"
                  variant="ghost"
                  className="h-6 w-6 text-[#8b949e] hover:text-[#f0f6fc]"
                  onClick={() => copyText(selectedBlock.hash)}
                >
                  {copied ? <Check className="w-3.5 h-3.5 text-[#3fb950]" /> : <Copy className="w-3.5 h-3.5" />}
                </Button>
              </div>
            </div>

            <div className="grid grid-cols-2 gap-2">
              <div className="bg-[#0d1117] p-2.5 rounded border border-[#30363d]">
                <span className="text-[#8b949e] block text-[10px]">VALIDATOR PROPOSER</span>
                <span className="text-[#f0f6fc] font-semibold">{selectedBlock.proposerShort}</span>
              </div>
              <div className="bg-[#0d1117] p-2.5 rounded border border-[#30363d]">
                <span className="text-[#8b949e] block text-[10px]">RONDE BFT</span>
                <span className="text-[#3fb950] font-semibold flex items-center gap-1">
                  <ShieldCheck className="w-3 h-3 text-[#3fb950]" />
                  Ronde {selectedBlock.bftRound ?? 0}
                </span>
              </div>
              <div className="bg-[#0d1117] p-2.5 rounded border border-[#30363d]">
                <span className="text-[#8b949e] block text-[10px]">TOTAL TRANSAKSI</span>
                <span className="text-[#f0f6fc] font-semibold">{selectedBlock.txs} Txs</span>
              </div>
              <div className="bg-[#0d1117] p-2.5 rounded border border-[#30363d]">
                <span className="text-[#8b949e] block text-[10px]">HADIAH BLOK (REWARD)</span>
                <span className="text-[#d29922] font-semibold">{selectedBlock.reward} AUR</span>
              </div>
            </div>
          </div>
        )}

        {/* Konten Detail Transaksi */}
        {selectedTx && (
          <div className="space-y-3 pt-2 text-xs">
            <div>
              <div className="text-[11px] text-[#8b949e] mb-1">ID TRANSAKSI / PENGIRIM</div>
              <div className="bg-[#0d1117] p-2 rounded border border-[#30363d] text-[#39c5cf] break-all flex items-center justify-between">
                <span>{selectedTx.from}</span>
                <Button
                  size="icon"
                  variant="ghost"
                  className="h-6 w-6 text-[#8b949e] hover:text-[#f0f6fc]"
                  onClick={() => copyText(selectedTx.from)}
                >
                  {copied ? <Check className="w-3.5 h-3.5 text-[#3fb950]" /> : <Copy className="w-3.5 h-3.5" />}
                </Button>
              </div>
            </div>

            <div className="grid grid-cols-2 gap-2">
              <div className="bg-[#0d1117] p-2.5 rounded border border-[#30363d]">
                <span className="text-[#8b949e] block text-[10px]">JUMLAH TRANSFER</span>
                <span className="text-[#d29922] font-semibold text-sm">{selectedTx.amount} AUR</span>
              </div>
              <div className="bg-[#0d1117] p-2.5 rounded border border-[#30363d]">
                <span className="text-[#8b949e] block text-[10px]">BIAYA GAS / FEE</span>
                <span className="text-[#3fb950] font-semibold">{selectedTx.gasFee}</span>
              </div>
              <div className="bg-[#0d1117] p-2.5 rounded border border-[#30363d] col-span-2">
                <span className="text-[#8b949e] block text-[10px]">PENERIMA</span>
                <span className="text-[#f0f6fc]">{selectedTx.to}</span>
              </div>
            </div>
          </div>
        )}

        {/* Konten Detail Peer */}
        {selectedPeer && (
          <div className="space-y-3 pt-2 text-xs">
            <div className="grid grid-cols-2 gap-2">
              <div className="bg-[#0d1117] p-2.5 rounded border border-[#30363d]">
                <span className="text-[#8b949e] block text-[10px]">PERAN KONSENSUS</span>
                <Badge variant={selectedPeer.role === 'VALIDATOR' ? 'default' : 'secondary'} className="mt-1">
                  {selectedPeer.role}
                </Badge>
              </div>
              <div className="bg-[#0d1117] p-2.5 rounded border border-[#30363d]">
                <span className="text-[#8b949e] block text-[10px]">PING / LATENSI</span>
                <span className="text-[#58a6ff] font-bold">{selectedPeer.ping} ms</span>
              </div>
              <div className="bg-[#0d1117] p-2.5 rounded border border-[#30363d] col-span-2">
                <span className="text-[#8b949e] block text-[10px]">MULTIADDR LOCATOR</span>
                <span className="text-[#f0f6fc] break-all">{selectedPeer.locator}</span>
              </div>
            </div>
          </div>
        )}

        <div className="flex justify-end pt-2">
          <Button variant="outline" size="sm" onClick={onClose} className="border-[#30363d] bg-[#21262d] text-[#f0f6fc]">
            Tutup
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
