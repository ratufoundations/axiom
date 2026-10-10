'use client';

import React, { useMemo, useState } from 'react';
import {
  ResponsiveContainer,
  LineChart,
  Line,
  XAxis,
  YAxis,
  Tooltip,
  CartesianGrid,
  ReferenceLine,
} from 'recharts';
import { Activity, Clock, TrendingDown, ArrowUpRight, CheckCircle2 } from 'lucide-react';

interface LatencyDataPoint {
  time: string;
  minute: number;
  avgLatency: number;
  minerLatency: number;
  fullNodeLatency: number;
}

interface LatencyHistoryChartProps {
  currentLatency?: number;
  height?: number;
  compact?: boolean;
}

export const LatencyHistoryChart: React.FC<LatencyHistoryChartProps> = ({
  currentLatency = 42,
  height = 240,
  compact = false,
}) => {
  const [filterRange, setFilterRange] = useState<'60m' | '30m' | '15m'>('60m');
  const [showMiners, setShowMiners] = useState(true);
  const [showFullNodes, setShowFullNodes] = useState(true);

  // Generate 60 minutes of realistic network latency telemetry
  const fullData: LatencyDataPoint[] = useMemo(() => {
    const data: LatencyDataPoint[] = [];
    const now = new Date();

    // Deterministic pseudo-random seed pattern with realistic micro-variations
    for (let i = 60; i >= 0; i--) {
      const pointTime = new Date(now.getTime() - i * 60 * 1000);
      const timeStr = i === 0 ? 'Now' : `${pointTime.getHours().toString().padStart(2, '0')}:${pointTime.getMinutes().toString().padStart(2, '0')}`;

      // Base sine oscillation + slight noise + occasional peak around 24m ago and 45m ago
      const wave = Math.sin(i / 5) * 2.8;
      const microNoise = Math.sin(i * 1.7) * 1.5 + Math.cos(i * 0.9) * 1.2;
      const spike = i === 24 ? 8.5 : i === 45 ? 6.2 : i === 12 ? -4.1 : 0;

      const avg = Number((41.8 + wave + microNoise + spike).toFixed(1));
      const miner = Number((avg - 5.5 + Math.sin(i * 2.1) * 1.1).toFixed(1));
      const fullNode = Number((avg + 3.2 + Math.cos(i * 1.8) * 1.2).toFixed(1));

      data.push({
        time: timeStr,
        minute: i,
        avgLatency: avg,
        minerLatency: miner,
        fullNodeLatency: fullNode,
      });
    }

    // Set the latest point to exactly currentLatency
    if (data.length > 0) {
      data[data.length - 1].avgLatency = currentLatency;
    }

    return data;
  }, [currentLatency]);

  const filteredData = useMemo(() => {
    if (filterRange === '15m') return fullData.slice(fullData.length - 16);
    if (filterRange === '30m') return fullData.slice(fullData.length - 31);
    return fullData;
  }, [fullData, filterRange]);

  const stats = useMemo(() => {
    const values = filteredData.map((d) => d.avgLatency);
    const min = Math.min(...values);
    const max = Math.max(...values);
    const sum = values.reduce((acc, v) => acc + v, 0);
    const mean = (sum / values.length).toFixed(1);
    const sorted = [...values].sort((a, b) => a - b);
    const p95 = sorted[Math.floor(sorted.length * 0.95)] || max;

    return { min, max, mean, p95 };
  }, [filteredData]);

  return (
    <div className="flex flex-col h-full w-full bg-[#161b22] text-[#c9d1d9] font-mono select-none">
      {/* Top Controls & Meta */}
      <div className="flex flex-wrap items-center justify-between px-3 py-2 border-b border-[#30363d] gap-2 bg-[#161b22]/90">
        <div className="flex items-center gap-2">
          <Activity className="w-3.5 h-3.5 text-[#39c5cf]" />
          <span className="text-xs font-bold text-[#f0f6fc] uppercase tracking-wide">
            Network Latency (Last 60 Minutes)
          </span>
          <span className="hidden sm:inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] bg-[#238636]/20 text-[#3fb950] border border-[#238636]/40">
            <CheckCircle2 className="w-2.5 h-2.5" /> SLA Normal
          </span>
        </div>

        {/* Range selectors and series toggles */}
        <div className="flex items-center gap-2 text-[10px]">
          <div className="flex items-center bg-[#0d1117] border border-[#30363d] rounded p-0.5">
            {(['15m', '30m', '60m'] as const).map((r) => (
              <button
                key={r}
                onClick={() => setFilterRange(r)}
                className={`px-2 py-0.5 rounded transition-colors ${
                  filterRange === r
                    ? 'bg-[#30363d] text-[#f0f6fc] font-semibold'
                    : 'text-[#8b949e] hover:text-[#c9d1d9]'
                }`}
              >
                {r}
              </button>
            ))}
          </div>

          {!compact && (
            <div className="hidden md:flex items-center gap-2">
              <button
                onClick={() => setShowMiners(!showMiners)}
                className={`flex items-center gap-1 px-1.5 py-0.5 rounded border transition-colors ${
                  showMiners
                    ? 'bg-[#3fb950]/10 border-[#3fb950]/40 text-[#3fb950]'
                    : 'border-[#30363d] text-[#8b949e] opacity-50'
                }`}
              >
                <span className="w-1.5 h-1.5 rounded-full bg-[#3fb950]"></span>
                <span>Validators (BFT)</span>
              </button>
              <button
                onClick={() => setShowFullNodes(!showFullNodes)}
                className={`flex items-center gap-1 px-1.5 py-0.5 rounded border transition-colors ${
                  showFullNodes
                    ? 'bg-[#d29922]/10 border-[#d29922]/40 text-[#d29922]'
                    : 'border-[#30363d] text-[#8b949e] opacity-50'
                }`}
              >
                <span className="w-1.5 h-1.5 rounded-full bg-[#d29922]"></span>
                <span>Full Nodes</span>
              </button>
            </div>
          )}
        </div>
      </div>

      {/* Summary KPI Pills */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 px-3 py-2 bg-[#0d1117]/60 border-b border-[#30363d]/60 text-[10px]">
        <div className="flex items-center justify-between px-2 py-1 bg-[#161b22] border border-[#30363d] rounded">
          <span className="text-[#8b949e]">Current Avg:</span>
          <span className="text-[#39c5cf] font-bold text-xs">{currentLatency} ms</span>
        </div>
        <div className="flex items-center justify-between px-2 py-1 bg-[#161b22] border border-[#30363d] rounded">
          <span className="text-[#8b949e]">Period Mean:</span>
          <span className="text-[#f0f6fc] font-bold text-xs">{stats.mean} ms</span>
        </div>
        <div className="flex items-center justify-between px-2 py-1 bg-[#161b22] border border-[#30363d] rounded">
          <span className="text-[#8b949e]">Min / Max:</span>
          <span className="text-[#c9d1d9] font-medium text-[11px]">
            <span className="text-[#3fb950]">{stats.min}</span> / <span className="text-[#d29922]">{stats.max} ms</span>
          </span>
        </div>
        <div className="flex items-center justify-between px-2 py-1 bg-[#161b22] border border-[#30363d] rounded">
          <span className="text-[#8b949e]">95th Percentile:</span>
          <span className="text-[#58a6ff] font-bold text-xs">{stats.p95} ms</span>
        </div>
      </div>

      {/* Recharts Time-Series Line Chart */}
      <div className="flex-1 w-full px-2 py-2 min-h-[190px]">
        <ResponsiveContainer width="100%" height={compact ? 180 : height}>
          <LineChart
            data={filteredData}
            margin={{ top: 10, right: 15, left: -18, bottom: 4 }}
          >
            <CartesianGrid stroke="#30363d" strokeDasharray="2 3" opacity={0.5} />
            <XAxis
              dataKey="time"
              stroke="#8b949e"
              fontSize={10}
              tickLine={false}
              interval={filterRange === '60m' ? 9 : 4}
              fontFamily="JetBrains Mono, monospace"
            />
            <YAxis
              stroke="#8b949e"
              fontSize={10}
              domain={[28, 62]}
              tickLine={false}
              unit="ms"
              fontFamily="JetBrains Mono, monospace"
            />
            <Tooltip
              content={({ active, payload, label }) => {
                if (active && payload && payload.length) {
                  return (
                    <div className="bg-[#161b22] border border-[#30363d] p-2.5 rounded shadow-xl text-xs font-mono space-y-1">
                      <div className="text-[#8b949e] border-b border-[#30363d] pb-1 flex items-center justify-between gap-3">
                        <span className="flex items-center gap-1">
                          <Clock className="w-3 h-3 text-[#39c5cf]" /> Time:
                        </span>
                        <span className="text-[#f0f6fc] font-bold">{label}</span>
                      </div>
                      <div className="flex items-center justify-between gap-4 text-[#39c5cf]">
                        <span>Avg Latency:</span>
                        <span className="font-bold">{payload[0]?.value} ms</span>
                      </div>
                      {payload[1] && (
                        <div className="flex items-center justify-between gap-4 text-[#3fb950]">
                          <span>Validators Latency:</span>
                          <span className="font-semibold">{payload[1]?.value} ms</span>
                        </div>
                      )}
                      {payload[2] && (
                        <div className="flex items-center justify-between gap-4 text-[#d29922]">
                          <span>Full Nodes:</span>
                          <span className="font-semibold">{payload[2]?.value} ms</span>
                        </div>
                      )}
                    </div>
                  );
                }
                return null;
              }}
            />
            <ReferenceLine
              y={42}
              stroke="#388bfd"
              strokeDasharray="4 4"
              opacity={0.7}
              label={{
                value: 'Target SLA (42ms)',
                fill: '#58a6ff',
                fontSize: 9,
                position: 'insideBottomRight',
              }}
            />

            {/* Primary Average Latency Line */}
            <Line
              type="monotone"
              dataKey="avgLatency"
              name="Average Latency"
              stroke="#39c5cf"
              strokeWidth={2}
              dot={false}
              activeDot={{
                r: 4.5,
                fill: '#39c5cf',
                stroke: '#0d1117',
                strokeWidth: 2,
              }}
            />

            {/* Validators Sub-Series */}
            {showMiners && !compact && (
              <Line
                type="monotone"
                dataKey="minerLatency"
                name="Validators (BFT)"
                stroke="#3fb950"
                strokeWidth={1.2}
                strokeDasharray="3 3"
                dot={false}
                opacity={0.8}
              />
            )}

            {/* Full Nodes Sub-Series */}
            {showFullNodes && !compact && (
              <Line
                type="monotone"
                dataKey="fullNodeLatency"
                name="Full Nodes"
                stroke="#d29922"
                strokeWidth={1.2}
                strokeDasharray="3 3"
                dot={false}
                opacity={0.8}
              />
            )}
          </LineChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
};
