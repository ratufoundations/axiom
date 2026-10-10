'use client';

import React, { useState } from 'react';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Search, ShieldCheck, ChevronUp, ChevronDown, Copy, Check } from 'lucide-react';
import { PeerNode } from '@/types';

interface PeerTelemetryTableProps {
  peers: PeerNode[];
  onSelectPeer?: (peer: PeerNode) => void;
}

export function PeerTelemetryTable({ peers, onSelectPeer }: PeerTelemetryTableProps) {
  const [isCollapsed, setIsCollapsed] = useState(false);
  const [searchTerm, setSearchTerm] = useState('');
  const [roleFilter, setRoleFilter] = useState<'ALL' | 'VALIDATOR' | 'FULLNODE'>('ALL');
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const handleCopy = (text: string, e: React.MouseEvent) => {
    e.stopPropagation();
    navigator.clipboard.writeText(text);
    setCopiedId(text);
    setTimeout(() => setCopiedId(null), 2000);
  };

  const filteredPeers = peers.filter((p) => {
    const matchesSearch =
      p.locator.toLowerCase().includes(searchTerm.toLowerCase()) ||
      p.id.toLowerCase().includes(searchTerm.toLowerCase()) ||
      p.city.toLowerCase().includes(searchTerm.toLowerCase());

    const isValidator = p.role === 'VALIDATOR' || (p.role as string) === 'MINER';
    const matchesRole =
      roleFilter === 'ALL' ||
      (roleFilter === 'VALIDATOR' && isValidator) ||
      (roleFilter === 'FULLNODE' && p.role === 'FULLNODE');

    return matchesSearch && matchesRole;
  });

  return (
    <div className="bg-[#161b22] border border-[#30363d] rounded flex flex-col overflow-hidden font-mono text-xs">
      {/* Header Bar */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-[#30363d] bg-[#161b22] gap-2 flex-wrap">
        <div className="flex items-center gap-3">
          <span className="text-xs font-bold tracking-wide text-[#f0f6fc] uppercase flex items-center gap-1.5">
            <ShieldCheck className="w-3.5 h-3.5 text-[#3fb950]" />
            TABEL TELEMETRI PEER (BFT MESH)
          </span>
          <span className="text-[11px] text-[#8b949e]">
            ({filteredPeers.length} peer aktif terhubung)
          </span>
        </div>

        <div className="flex items-center gap-2">
          {/* Search Box */}
          <div className="relative">
            <Search className="w-3 h-3 text-[#8b949e] absolute left-2 top-1/2 -translate-y-1/2" />
            <Input
              type="text"
              placeholder="Cari peer / IP..."
              value={searchTerm}
              onChange={(e) => setSearchTerm(e.target.value)}
              className="bg-[#0d1117] border-[#30363d] pl-6 pr-2 py-0.5 h-7 text-xs text-[#c9d1d9] w-36"
            />
          </div>

          {/* Filter Buttons */}
          <div className="flex gap-1">
            <Button
              size="sm"
              variant={roleFilter === 'ALL' ? 'secondary' : 'ghost'}
              className="h-6 px-2 text-[10px]"
              onClick={() => setRoleFilter('ALL')}
            >
              ALL
            </Button>
            <Button
              size="sm"
              variant={roleFilter === 'VALIDATOR' ? 'secondary' : 'ghost'}
              className="h-6 px-2 text-[10px] text-[#3fb950]"
              onClick={() => setRoleFilter('VALIDATOR')}
            >
              VALIDATOR
            </Button>
            <Button
              size="sm"
              variant={roleFilter === 'FULLNODE' ? 'secondary' : 'ghost'}
              className="h-6 px-2 text-[10px] text-[#d29922]"
              onClick={() => setRoleFilter('FULLNODE')}
            >
              FULLNODE
            </Button>
          </div>

          <Button
            size="icon"
            variant="ghost"
            className="h-6 w-6 text-[#8b949e]"
            onClick={() => setIsCollapsed(!isCollapsed)}
          >
            {isCollapsed ? <ChevronDown className="w-3.5 h-3.5" /> : <ChevronUp className="w-3.5 h-3.5" />}
          </Button>
        </div>
      </div>

      {/* Table Data */}
      {!isCollapsed && (
        <Table>
          <TableHeader className="bg-[#161b22]/50 border-b border-[#30363d]">
            <TableRow className="border-none hover:bg-transparent text-[11px] text-[#8b949e]">
              <TableHead className="py-2 px-3 text-[#8b949e]">LOCATOR TRANSPORT</TableHead>
              <TableHead className="py-2 px-3 text-[#8b949e]">PERAN (ROLE)</TableHead>
              <TableHead className="py-2 px-3 text-[#8b949e]">PEER ID</TableHead>
              <TableHead className="py-2 px-3 text-[#8b949e]">PING</TableHead>
              <TableHead className="py-2 px-3 text-[#8b949e]">TERAKHIR TERLIHAT</TableHead>
              <TableHead className="py-2 px-3 text-right text-[#8b949e]">TRAFFIC</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody className="divide-y divide-[#30363d]/40">
            {filteredPeers.map((peer) => {
              const isValidator = peer.role === 'VALIDATOR' || (peer.role as string) === 'MINER';
              return (
                <TableRow
                  key={peer.id}
                  onClick={() => onSelectPeer?.(peer)}
                  className="hover:bg-[#21262d]/50 cursor-pointer border-none"
                >
                  <TableCell className="py-2 px-3 whitespace-nowrap text-[#f0f6fc]">
                    <span className="text-[#8b949e] mr-1">{peer.country}</span>
                    {peer.locator}
                  </TableCell>
                  <TableCell className="py-2 px-3 whitespace-nowrap">
                    {isValidator ? (
                      <Badge variant="outline" className="text-[#3fb950] border-[#238636]/50 bg-[#238636]/20 text-[10px]">
                        VALIDATOR
                      </Badge>
                    ) : (
                      <Badge variant="outline" className="text-[#d29922] border-[#d29922]/50 bg-[#d29922]/10 text-[10px]">
                        FULLNODE
                      </Badge>
                    )}
                  </TableCell>
                  <TableCell className="py-2 px-3 whitespace-nowrap text-[#8b949e]">
                    <div className="flex items-center gap-1.5">
                      <span>{peer.shortId}</span>
                      <button
                        onClick={(e) => handleCopy(peer.id, e)}
                        className="hover:text-[#f0f6fc] cursor-pointer"
                      >
                        {copiedId === peer.id ? <Check className="w-2.5 h-2.5 text-[#3fb950]" /> : <Copy className="w-2.5 h-2.5" />}
                      </button>
                    </div>
                  </TableCell>
                  <TableCell className="py-2 px-3 whitespace-nowrap text-[#f0f6fc]">
                    {peer.ping} <span className="text-[10px] text-[#8b949e]">ms</span>
                  </TableCell>
                  <TableCell className="py-2 px-3 whitespace-nowrap text-[#8b949e]">
                    {peer.lastSeen}
                  </TableCell>
                  <TableCell className="py-2 px-3 whitespace-nowrap text-right text-[#c9d1d9]">
                    {peer.trafficString}
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}
    </div>
  );
}
