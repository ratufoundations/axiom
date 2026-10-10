'use client';

import React, { useState, useEffect } from 'react';
import {
  RefreshCw,
  LineChart as ChartIcon,
  Map as MapIcon,
  Columns,
  X,
  Maximize2,
  ChevronUp,
  ChevronDown,
} from 'lucide-react';
import { MESH_NODES_GEO, MESH_CONNECTIONS } from '@/data/initialData';
import { LatencyHistoryChart } from './LatencyHistoryChart';

interface PeerMapPanelProps {
  avgLatency: number;
  validatorCount?: number;
  minerCount?: number;
  fullNodeCount: number;
}

export const PeerMapPanel: React.FC<PeerMapPanelProps> = ({
  avgLatency,
  validatorCount,
  minerCount,
  fullNodeCount,
}) => {
  const [viewMode, setViewMode] = useState<'map' | 'latency' | 'split'>('map');
  const [showDrawer, setShowDrawer] = useState(false);
  const [hoveredNode, setHoveredNode] = useState<{
    id: string;
    x: number;
    y: number;
    role: string;
    label: string;
  } | null>(null);

  // Animated pulse progress for packets along mesh lines
  const [pulseTick, setPulseTick] = useState(0);

  useEffect(() => {
    const interval = setInterval(() => {
      setPulseTick((prev) => (prev + 1) % 100);
    }, 40);
    return () => clearInterval(interval);
  }, []);

  // Calculate coordinates lookup for line arcs
  const nodeMap = React.useMemo(() => {
    const map = new Map<string, (typeof MESH_NODES_GEO)[0]>();
    MESH_NODES_GEO.forEach((node) => map.set(node.id, node));
    return map;
  }, []);

  // Render the SVG map canvas and associated overlays
  const renderMapCanvas = (isCompactMap = false) => (
    <div className={`relative w-full ${isCompactMap ? 'h-[240px]' : 'flex-1 min-h-[340px]'} bg-[#0d1117] overflow-hidden select-none`}>
      {/* Top-Left Metric Overlay: Avg Latency with quick chart toggle */}
      <div className="absolute top-3 left-3 z-10">
        <button
          onClick={() => {
            if (viewMode === 'map') {
              setShowDrawer(!showDrawer);
            } else {
              setViewMode('latency');
            }
          }}
          className="bg-[#161b22]/90 hover:bg-[#21262d] backdrop-blur-xs border border-[#30363d] px-2.5 py-1 rounded text-left transition-colors cursor-pointer flex items-center gap-2 group shadow-md"
          title="Click to view 60-minute latency time-series chart"
        >
          <span className="text-[11px] font-mono text-[#8b949e]">
            Avg Latency: <span className="text-[#39c5cf] font-bold">{avgLatency}ms</span>
          </span>
          <span className="text-[9px] px-1.5 py-0.5 bg-[#39c5cf]/15 text-[#39c5cf] rounded border border-[#39c5cf]/30 group-hover:bg-[#39c5cf]/25 flex items-center gap-1 font-semibold">
            <ChartIcon className="w-2.5 h-2.5" />
            <span>60m Trend</span>
          </span>
        </button>
      </div>

      {/* SVG World Map + Mesh Overlay */}
      <svg
        viewBox="0 0 920 460"
        className="w-full h-full object-cover"
        preserveAspectRatio="xMidYMid meet"
      >
        <defs>
          {/* Radial glow for miners */}
          <radialGradient id="minerGlow" cx="50%" cy="50%" r="50%">
            <stop offset="0%" stopColor="#3fb950" stopOpacity="0.8" />
            <stop offset="60%" stopColor="#3fb950" stopOpacity="0.3" />
            <stop offset="100%" stopColor="#3fb950" stopOpacity="0" />
          </radialGradient>

          {/* Radial glow for full nodes */}
          <radialGradient id="fullNodeGlow" cx="50%" cy="50%" r="50%">
            <stop offset="0%" stopColor="#d29922" stopOpacity="0.8" />
            <stop offset="60%" stopColor="#d29922" stopOpacity="0.3" />
            <stop offset="100%" stopColor="#d29922" stopOpacity="0" />
          </radialGradient>

          {/* Cyan pulse glow */}
          <radialGradient id="cyanGlow" cx="50%" cy="50%" r="50%">
            <stop offset="0%" stopColor="#39c5cf" stopOpacity="0.9" />
            <stop offset="100%" stopColor="#39c5cf" stopOpacity="0" />
          </radialGradient>
        </defs>

        {/* Grid lines (Subtle Lat/Long Coordinates) */}
        <g opacity="0.15" stroke="#30363d" strokeWidth="0.8" strokeDasharray="3 4">
          <line x1="0" y1="115" x2="920" y2="115" />
          <line x1="0" y1="230" x2="920" y2="230" />
          <line x1="0" y1="345" x2="920" y2="345" />
          <line x1="230" y1="0" x2="230" y2="460" />
          <line x1="460" y1="0" x2="460" y2="460" />
          <line x1="690" y1="0" x2="690" y2="460" />
        </g>

        {/* World Continents Realistic Vector Silhouettes */}
        <g fill="#161b22" stroke="#21262d" strokeWidth="0.75">
          {/* North America */}
          <path d="M 80 60 Q 130 50 180 55 Q 240 50 250 85 Q 260 110 245 135 Q 265 145 255 175 Q 240 190 225 210 Q 210 230 195 240 Q 185 220 170 200 Q 150 190 140 160 Q 120 140 100 120 Q 70 95 80 60 Z" />
          {/* Greenland */}
          <path d="M 285 40 Q 330 35 340 55 Q 320 85 295 80 Q 275 65 285 40 Z" />
          {/* South America */}
          <path d="M 220 245 Q 255 250 280 270 Q 315 310 295 360 Q 270 410 255 425 Q 240 400 245 350 Q 235 300 215 270 Z" />
          {/* Europe */}
          <path d="M 420 85 Q 460 75 490 85 Q 520 100 505 130 Q 485 150 495 180 Q 465 185 440 180 Q 425 155 415 130 Q 410 105 420 85 Z" />
          {/* Great Britain / Scandinavia */}
          <path d="M 425 110 Q 440 105 445 125 Q 430 140 420 130 Z" />
          <path d="M 460 55 Q 490 50 485 90 Q 465 80 460 55 Z" />
          {/* Africa */}
          <path d="M 425 195 Q 475 190 500 215 Q 525 260 510 320 Q 495 370 470 380 Q 450 350 440 300 Q 415 270 405 230 Q 410 205 425 195 Z" />
          {/* Asia Main */}
          <path d="M 515 90 Q 600 70 700 85 Q 780 110 790 160 Q 755 190 730 220 Q 690 245 660 230 Q 630 250 590 235 Q 560 210 535 200 Q 515 160 515 90 Z" />
          {/* Japan */}
          <path d="M 770 145 Q 785 160 780 185 Q 765 175 770 145 Z" />
          {/* South East Asia Islands */}
          <path d="M 660 270 Q 710 275 730 295 Q 690 310 660 270 Z" />
          <path d="M 710 290 Q 740 295 730 315 Z" />
          {/* Australia */}
          <path d="M 690 325 Q 760 310 790 340 Q 785 390 750 405 Q 715 395 685 365 Q 675 340 690 325 Z" />
          {/* New Zealand */}
          <path d="M 815 385 Q 825 400 815 415 Q 805 405 815 385 Z" />
        </g>

        {/* Peer Mesh Connection Arcs */}
        <g>
          {MESH_CONNECTIONS.map((conn, idx) => {
            const fromNode = nodeMap.get(conn.from);
            const toNode = nodeMap.get(conn.to);
            if (!fromNode || !toNode) return null;

            // Calculate curvature
            const dx = toNode.x - fromNode.x;
            const dy = toNode.y - fromNode.y;
            const cx = (fromNode.x + toNode.x) / 2 - dy * 0.15;
            const cy = (fromNode.y + toNode.y) / 2 + dx * 0.08;

            const isToOceania = toNode.id.startsWith('oc') || fromNode.id.startsWith('oc');
            const isTransatlantic =
              (fromNode.id.startsWith('na') && toNode.id.startsWith('eu')) ||
              (fromNode.id.startsWith('eu') && toNode.id.startsWith('na'));

            let strokeColor = '#388bfd';
            let opacity = 0.45;
            let strokeWidth = 1.2;

            if (isToOceania) {
              strokeColor = idx % 2 === 0 ? '#d29922' : '#3fb950';
              opacity = 0.65;
              strokeWidth = 1.4;
            } else if (isTransatlantic) {
              strokeColor = '#58a6ff';
              opacity = 0.45;
            } else if (
              (fromNode.role === 'VALIDATOR' || fromNode.role === 'MINER') &&
              (toNode.role === 'VALIDATOR' || toNode.role === 'MINER')
            ) {
              strokeColor = '#3fb950';
              opacity = 0.4;
            }

            const pathD = `M ${fromNode.x} ${fromNode.y} Q ${cx} ${cy} ${toNode.x} ${toNode.y}`;

            // Animated packet along bezier curve
            const t = ((pulseTick + idx * 7) % 100) / 100;
            const px = (1 - t) * (1 - t) * fromNode.x + 2 * (1 - t) * t * cx + t * t * toNode.x;
            const py = (1 - t) * (1 - t) * fromNode.y + 2 * (1 - t) * t * cy + t * t * toNode.y;

            return (
              <g key={`conn-${idx}`}>
                <path
                  d={pathD}
                  fill="none"
                  stroke={strokeColor}
                  strokeWidth={strokeWidth}
                  strokeOpacity={opacity}
                  strokeLinecap="round"
                />
                {idx % 2 === 0 && (
                  <circle
                    cx={px}
                    cy={py}
                    r="1.8"
                    fill={isToOceania ? '#d29922' : '#39c5cf'}
                    opacity="0.9"
                  />
                )}
              </g>
            );
          })}
        </g>

        {/* Node Markers */}
        <g>
          {MESH_NODES_GEO.map((node) => {
            const isValidator = node.role === 'VALIDATOR' || node.role === 'MINER';
            const dotColor = isValidator ? '#3fb950' : '#d29922';

            return (
              <g
                key={node.id}
                className="cursor-pointer transition-transform hover:scale-125"
                onMouseEnter={() =>
                  setHoveredNode({
                    id: node.id,
                    x: node.x,
                    y: node.y,
                    role: node.role,
                    label: node.label,
                  })
                }
                onMouseLeave={() => setHoveredNode(null)}
              >
                <circle
                  cx={node.x}
                  cy={node.y}
                  r={isValidator ? '7' : '6'}
                  fill={isValidator ? 'url(#minerGlow)' : 'url(#fullNodeGlow)'}
                />
                <circle
                  cx={node.x}
                  cy={node.y}
                  r={isValidator ? '2.8' : '2.4'}
                  fill={dotColor}
                  stroke="#0d1117"
                  strokeWidth="0.8"
                />
              </g>
            );
          })}
        </g>
      </svg>

      {/* Hovered Node Tooltip */}
      {hoveredNode && (
        <div
          className="absolute z-20 pointer-events-none bg-[#161b22] border border-[#30363d] px-2.5 py-1.5 rounded shadow-lg text-[10px] font-mono transform -translate-x-1/2 -translate-y-full mb-2"
          style={{
            left: `${(hoveredNode.x / 920) * 100}%`,
            top: `${(hoveredNode.y / 460) * 100}%`,
          }}
        >
          <div className="font-bold text-[#f0f6fc]">{hoveredNode.label}</div>
          <div className="text-[#8b949e] flex items-center gap-1.5 mt-0.5">
            <span
              className={`w-1.5 h-1.5 rounded-full ${
                hoveredNode.role === 'VALIDATOR' || hoveredNode.role === 'MINER' ? 'bg-[#3fb950]' : 'bg-[#d29922]'
              }`}
            />
            <span className="text-[#f0f6fc] font-semibold">
              {hoveredNode.role === 'VALIDATOR' ? 'VALIDATOR (BFT)' : hoveredNode.role}
            </span>
            <span>•</span>
            <span className="text-[#58a6ff]">Port 4000</span>
          </div>
        </div>
      )}

      {/* Bottom-Left Overlay: Telemetry and In/Out Bandwidth Diagram */}
      <div
        id="map-bottom-telemetry"
        className="absolute bottom-3 left-3 z-10 flex flex-col gap-1.5 pointer-events-auto"
      >
        <div className="flex items-center gap-2 text-[10px] font-mono text-[#8b949e] bg-[#161b22]/90 border border-[#30363d]/80 px-2 py-1 rounded backdrop-blur-xs shadow-md">
          <span>Avg Latency: <strong className="text-[#f0f6fc] font-normal">{avgLatency}ms</strong></span>
          <span className="text-[#30363d]">|</span>
          <button
            onClick={() => setShowDrawer(!showDrawer)}
            className="text-[#39c5cf] hover:text-[#58a6ff] hover:underline flex items-center gap-1 font-semibold transition-colors cursor-pointer"
            title="Toggle 60m latency time-series chart overlay"
          >
            <ChartIcon className="w-3 h-3" />
            <span>{showDrawer ? 'Hide Trend' : '60m Trend'}</span>
          </button>
          <span className="text-[#30363d]">|</span>
          <span className="flex items-center gap-1 text-[#3fb950]">
            <span className="w-1.5 h-1.5 rounded-full bg-[#3fb950]"></span> Validator (BFT)
          </span>
        </div>

        {/* Bandwidth Diagram (masuk / Keluar waveform) */}
        {!isCompactMap && (
          <div className="bg-[#161b22]/90 border border-[#30363d] rounded p-2 w-[160px] backdrop-blur-xs">
            <div className="h-12 w-full relative flex items-center justify-center">
              <svg
                viewBox="0 0 100 40"
                preserveAspectRatio="none"
                className="w-full h-full overflow-visible"
              >
                <defs>
                  <linearGradient id="masukGrad" x1="0%" y1="0%" x2="0%" y2="100%">
                    <stop offset="0%" stopColor="#3fb950" stopOpacity="0.45" />
                    <stop offset="100%" stopColor="#3fb950" stopOpacity="0.05" />
                  </linearGradient>
                  <linearGradient id="keluarGrad" x1="0%" y1="0%" x2="0%" y2="100%">
                    <stop offset="0%" stopColor="#d29922" stopOpacity="0.05" />
                    <stop offset="100%" stopColor="#d29922" stopOpacity="0.45" />
                  </linearGradient>
                </defs>

                {/* Zero centerline */}
                <line x1="0" y1="20" x2="100" y2="20" stroke="#30363d" strokeWidth="1" />

                {/* Upper Green Wave (masuk) */}
                <path
                  d="M 5 20 L 10 20 L 15 5 L 35 5 L 40 20 Z"
                  fill="url(#masukGrad)"
                  stroke="#3fb950"
                  strokeWidth="1.2"
                  strokeLinejoin="round"
                />

                {/* Lower Orange Wave (Keluar) */}
                <path
                  d="M 45 20 L 50 20 L 55 33 L 75 33 L 80 20 Z"
                  fill="url(#keluarGrad)"
                  stroke="#d29922"
                  strokeWidth="1.2"
                  strokeLinejoin="round"
                />
              </svg>
            </div>

            <div className="flex items-center justify-between text-[10px] font-mono text-[#8b949e] px-1 mt-1">
              <span className="text-[#3fb950]">masuk</span>
              <span className="text-[#d29922]">Keluar</span>
            </div>
          </div>
        )}
      </div>

      {/* Bottom-Right Overlay: Concentric Circular Telemetry Rings */}
      {!isCompactMap && (
        <div
          id="map-concentric-gauge"
          className="absolute bottom-3 right-3 z-10 flex items-center justify-center pointer-events-none"
        >
          <div className="relative w-16 h-16 flex items-center justify-center">
            <svg viewBox="0 0 60 60" className="w-full h-full transform -rotate-90">
              <circle
                cx="30"
                cy="30"
                r="24"
                stroke="#1f2937"
                strokeWidth="3.5"
                fill="none"
              />
              <circle
                cx="30"
                cy="30"
                r="24"
                stroke="#39c5cf"
                strokeWidth="3.5"
                strokeDasharray="150.8"
                strokeDashoffset="38"
                strokeLinecap="round"
                fill="none"
              />
              <circle
                cx="30"
                cy="30"
                r="17"
                stroke="#1f2937"
                strokeWidth="3.5"
                fill="none"
              />
              <circle
                cx="30"
                cy="30"
                r="17"
                stroke="#3fb950"
                strokeWidth="3.5"
                strokeDasharray="106.8"
                strokeDashoffset="14"
                strokeLinecap="round"
                fill="none"
              />
            </svg>
          </div>
        </div>
      )}

      {/* Collapsible Latency Drawer over Map */}
      {showDrawer && !isCompactMap && (
        <div className="absolute inset-x-0 bottom-0 z-30 bg-[#161b22]/95 border-t border-[#30363d] backdrop-blur-md shadow-2xl transition-all animate-in fade-in slide-in-from-bottom duration-200">
          <div className="flex items-center justify-between px-3 py-1.5 bg-[#0d1117] border-b border-[#30363d] text-[11px] font-mono">
            <div className="flex items-center gap-2">
              <ChartIcon className="w-3.5 h-3.5 text-[#39c5cf]" />
              <span className="font-bold text-[#f0f6fc]">
                NETWORK LATENCY (LAST 60 MINUTES) — RECHARTS TIME-SERIES
              </span>
            </div>
            <div className="flex items-center gap-2">
              <button
                onClick={() => setViewMode('latency')}
                className="text-[10px] text-[#39c5cf] hover:underline"
              >
                Expand View &rarr;
              </button>
              <button
                onClick={() => setShowDrawer(false)}
                className="p-1 hover:bg-[#30363d] rounded text-[#8b949e] hover:text-[#f0f6fc]"
                title="Close Drawer"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </div>
          </div>
          <div className="p-1">
            <LatencyHistoryChart currentLatency={avgLatency} height={190} compact={true} />
          </div>
        </div>
      )}
    </div>
  );

  return (
    <div
      id="panel-3d-mesh-topology"
      className="bg-[#161b22] border border-[#30363d] rounded flex flex-col h-full overflow-hidden relative"
    >
      {/* Top Header of Map */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-[#30363d] bg-[#161b22] z-10 flex-wrap gap-2">
        <div className="flex items-center gap-2 flex-wrap">
          <span className="text-xs font-mono font-bold tracking-wide text-[#f0f6fc] uppercase">
            {viewMode === 'latency'
              ? 'NETWORK LATENCY TELEMETRY'
              : '3D MESH TOPOLOGY / PEER MAP'}
          </span>

          {/* View Mode Switcher */}
          <div className="flex items-center bg-[#0d1117] border border-[#30363d] rounded p-0.5 text-[10px] font-mono">
            <button
              onClick={() => setViewMode('map')}
              className={`flex items-center gap-1 px-2 py-0.5 rounded transition-colors cursor-pointer ${
                viewMode === 'map'
                  ? 'bg-[#30363d] text-[#f0f6fc] font-semibold'
                  : 'text-[#8b949e] hover:text-[#c9d1d9]'
              }`}
              title="Show 3D Mesh Topology Map"
            >
              <MapIcon className="w-2.5 h-2.5" />
              <span>MAP</span>
            </button>
            <button
              onClick={() => setViewMode('latency')}
              className={`flex items-center gap-1 px-2 py-0.5 rounded transition-colors cursor-pointer ${
                viewMode === 'latency'
                  ? 'bg-[#39c5cf]/20 text-[#39c5cf] border border-[#39c5cf]/40 font-semibold'
                  : 'text-[#8b949e] hover:text-[#39c5cf]'
              }`}
              title="Show 60-Minute Latency Time-Series (Recharts)"
            >
              <ChartIcon className="w-2.5 h-2.5" />
              <span>LATENCY (60M)</span>
            </button>
            <button
              onClick={() => setViewMode('split')}
              className={`hidden sm:flex items-center gap-1 px-2 py-0.5 rounded transition-colors cursor-pointer ${
                viewMode === 'split'
                  ? 'bg-[#30363d] text-[#f0f6fc] font-semibold'
                  : 'text-[#8b949e] hover:text-[#c9d1d9]'
              }`}
              title="Split View (Map + Latency Line Chart)"
            >
              <Columns className="w-2.5 h-2.5" />
              <span>SPLIT</span>
            </button>
          </div>
        </div>

        <div className="flex items-center gap-3 text-xs font-mono">
          <div className="flex items-center gap-1.5">
            <span className="w-2 h-2 rounded-full bg-[#3fb950] shadow-[0_0_6px_#3fb950]"></span>
            <span className="text-[#8b949e]">Validator (BFT)</span>
          </div>
          <div className="flex items-center gap-1.5">
            <span className="w-2 h-2 rounded-full bg-[#d29922] shadow-[0_0_6px_#d29922]"></span>
            <span className="text-[#8b949e]">Full Node</span>
          </div>
          <button
            title="Reset view"
            className="p-1 hover:bg-[#21262d] text-[#8b949e] hover:text-[#f0f6fc] rounded transition-colors"
          >
            <RefreshCw className="w-3 h-3" />
          </button>
        </div>
      </div>

      {/* Main Content Area based on viewMode */}
      {viewMode === 'map' && renderMapCanvas(false)}

      {viewMode === 'latency' && (
        <div className="flex-1 w-full bg-[#0d1117] flex flex-col overflow-hidden">
          <LatencyHistoryChart currentLatency={avgLatency} height={340} />
        </div>
      )}

      {viewMode === 'split' && (
        <div className="flex-1 w-full bg-[#0d1117] flex flex-col overflow-y-auto divide-y divide-[#30363d]">
          {renderMapCanvas(true)}
          <div className="w-full bg-[#161b22]">
            <LatencyHistoryChart currentLatency={avgLatency} height={210} compact={false} />
          </div>
        </div>
      )}
    </div>
  );
};
