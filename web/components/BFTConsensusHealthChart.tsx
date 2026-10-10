'use client';

import React, { useState, useEffect, useMemo } from 'react';
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
import {
  ShieldCheck,
  Activity,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  Clock,
  Radio,
  SlidersHorizontal,
} from 'lucide-react';

export interface BFTHealthDataPoint {
  time: string;
  timestamp: number;
  minutesAgo: number;
  roundLatency: number; // in ms (e.g. 210ms - 340ms)
  quorumParticipation: number; // in % (e.g. 96.0% - 99.5%)
  validatingCount: number; // e.g. 137 - 142
  round: number;
  status: 'COMMITTED' | 'PRECOMMIT';
}

interface BFTConsensusHealthChartProps {
  currentQuorum?: number;
  currentLatency?: number;
  activeValidators?: number;
  onQuorumDrop?: (val: number) => void;
  onRoundSkip?: (round: number) => void;
}

export const BFTConsensusHealthChart: React.FC<BFTConsensusHealthChartProps> = ({
  currentQuorum = 98.6,
  currentLatency = 245,
  activeValidators = 142,
  onQuorumDrop,
  onRoundSkip,
}) => {
  const [metricMode, setMetricMode] = useState<'quorum' | 'latency' | 'both'>('quorum');
  const [isExpanded, setIsExpanded] = useState(true);

  // Generate 10-minute historical telemetry data in 30-second steps (21 points)
  const [dataPoints, setDataPoints] = useState<BFTHealthDataPoint[]>(() => {
    const points: BFTHealthDataPoint[] = [];
    const now = Date.now();

    for (let i = 20; i >= 0; i--) {
      const pointTime = new Date(now - i * 30 * 1000);
      const timeStr =
        i === 0
          ? 'Now'
          : `${pointTime.getHours().toString().padStart(2, '0')}:${pointTime.getMinutes().toString().padStart(2, '0')}:${pointTime.getSeconds().toString().padStart(2, '0')}`;

      // BFT consensus parameters: high quorum (>95%), round latency ~220-310ms
      const quorumNoise = Math.sin(i * 0.8) * 1.4 + Math.cos(i * 1.5) * 0.8;
      const quorum = Number(Math.min(100, Math.max(94.5, 98.2 + quorumNoise)).toFixed(1));

      const latencyNoise = Math.cos(i * 0.9) * 28 + Math.sin(i * 1.4) * 18;
      const latency = Number(Math.max(180, Math.min(380, 248 + latencyNoise)).toFixed(0));

      const valCount = Math.round((quorum / 100) * 142);

      points.push({
        time: timeStr,
        timestamp: pointTime.getTime(),
        minutesAgo: Number(((i * 30) / 60).toFixed(1)),
        roundLatency: latency,
        quorumParticipation: quorum,
        validatingCount: valCount,
        round: 0,
        status: 'COMMITTED',
      });
    }

    return points;
  });

  // Periodically stream new real-time BFT consensus rounds (every 4 seconds)
  useEffect(() => {
    const interval = setInterval(() => {
      setDataPoints((prev) => {
        const now = new Date();
        const timeStr = `${now.getHours().toString().padStart(2, '0')}:${now.getMinutes().toString().padStart(2, '0')}:${now.getSeconds().toString().padStart(2, '0')}`;
        
        const quorumJitter = (Math.random() - 0.48) * 1.2;
        const newQuorum = Number(Math.min(100, Math.max(95.2, (prev[prev.length - 1]?.quorumParticipation || 98.2) + quorumJitter)).toFixed(1));
        
        const latencyJitter = (Math.random() - 0.5) * 22;
        const newLatency = Number(Math.max(190, Math.min(360, (prev[prev.length - 1]?.roundLatency || 245) + latencyJitter)).toFixed(0));

        const newPoint: BFTHealthDataPoint = {
          time: 'Now',
          timestamp: now.getTime(),
          minutesAgo: 0,
          roundLatency: newLatency,
          quorumParticipation: newQuorum,
          validatingCount: Math.round((newQuorum / 100) * activeValidators),
          round: 0,
          status: 'COMMITTED',
        };

        // Shift timestamps of existing points
        const updated = prev.slice(1).map((p, idx, arr) => {
          if (idx === arr.length - 1) {
            const pDate = new Date(p.timestamp);
            return {
              ...p,
              time: `${pDate.getHours().toString().padStart(2, '0')}:${pDate.getMinutes().toString().padStart(2, '0')}:${pDate.getSeconds().toString().padStart(2, '0')}`,
            };
          }
          return p;
        });

        return [...updated, newPoint];
      });
    }, 4000);

    return () => clearInterval(interval);
  }, [activeValidators]);

  const latestPoint = dataPoints[dataPoints.length - 1] || {
    quorumParticipation: currentQuorum,
    roundLatency: currentLatency,
    validatingCount: 139,
  };

  const quorumStats = useMemo(() => {
    const vals = dataPoints.map((d) => d.quorumParticipation);
    const min = Math.min(...vals);
    const max = Math.max(...vals);
    const avg = (vals.reduce((a, b) => a + b, 0) / vals.length).toFixed(1);
    return { min, max, avg };
  }, [dataPoints]);

  const latencyStats = useMemo(() => {
    const vals = dataPoints.map((d) => d.roundLatency);
    const min = Math.min(...vals);
    const max = Math.max(...vals);
    const avg = (vals.reduce((a, b) => a + b, 0) / vals.length).toFixed(0);
    return { min, max, avg };
  }, [dataPoints]);

  return (
    <div
      id="bft-consensus-health-card"
      className="bg-[#161b22] border border-[#30363d] rounded overflow-hidden flex flex-col font-mono text-xs shadow-sm transition-all"
    >
      {/* Header Bar */}
      <div className="flex flex-wrap items-center justify-between px-3 py-2 bg-[#161b22] border-b border-[#30363d] gap-2">
        <div className="flex items-center gap-2">
          <ShieldCheck className="w-4 h-4 text-[#3fb950]" />
          <span className="font-bold text-[#f0f6fc] tracking-wide uppercase text-xs">
            BFT CONSENSUS HEALTH &amp; QUORUM TELEMETRY
          </span>
          <span className="hidden sm:inline-flex items-center gap-1 text-[10px] text-[#3fb950] bg-[#238636]/15 px-1.5 py-0.5 rounded border border-[#238636]/30">
            <Radio className="w-2.5 h-2.5 animate-pulse text-[#3fb950]" />
            10m Real-Time Window
          </span>
        </div>

        {/* Controls: Mode Switcher & Expand/Collapse */}
        <div className="flex items-center gap-2">
          {/* Mode Switcher */}
          <div className="flex items-center bg-[#0d1117] border border-[#30363d] rounded p-0.5 text-[10px]">
            <button
              onClick={() => setMetricMode('quorum')}
              className={`px-2 py-0.5 rounded transition-colors cursor-pointer ${
                metricMode === 'quorum'
                  ? 'bg-[#3fb950]/20 text-[#3fb950] border border-[#3fb950]/40 font-semibold'
                  : 'text-[#8b949e] hover:text-[#c9d1d9]'
              }`}
              title="Track Quorum Participation (%)"
            >
              Quorum %
            </button>
            <button
              onClick={() => setMetricMode('latency')}
              className={`px-2 py-0.5 rounded transition-colors cursor-pointer ${
                metricMode === 'latency'
                  ? 'bg-[#39c5cf]/20 text-[#39c5cf] border border-[#39c5cf]/40 font-semibold'
                  : 'text-[#8b949e] hover:text-[#c9d1d9]'
              }`}
              title="Track BFT Consensus Round Latency (ms)"
            >
              Round Latency
            </button>
            <button
              onClick={() => setMetricMode('both')}
              className={`hidden md:block px-2 py-0.5 rounded transition-colors cursor-pointer ${
                metricMode === 'both'
                  ? 'bg-[#30363d] text-[#f0f6fc] font-semibold'
                  : 'text-[#8b949e] hover:text-[#c9d1d9]'
              }`}
              title="Overlay Quorum % & Round Latency"
            >
              Dual View
            </button>
          </div>

          {(onQuorumDrop || onRoundSkip) && (
            <div className="hidden lg:flex items-center gap-1 bg-[#0d1117] border border-[#30363d] rounded p-0.5 text-[9px]">
              <span className="text-[#8b949e] px-1 font-semibold">Test Alert:</span>
              {onQuorumDrop && (
                <button
                  onClick={() => onQuorumDrop(59.2)}
                  className="px-1.5 py-0.5 bg-[#f85149]/20 hover:bg-[#f85149]/30 text-[#f85149] rounded border border-[#f85149]/40 cursor-pointer font-semibold transition-colors"
                  title="Simulate Quorum falling to 59.2% (<67%)"
                >
                  &lt;67% Drop
                </button>
              )}
              {onRoundSkip && (
                <button
                  onClick={() => onRoundSkip(1)}
                  className="px-1.5 py-0.5 bg-[#d29922]/20 hover:bg-[#d29922]/30 text-[#d29922] rounded border border-[#d29922]/40 cursor-pointer font-semibold transition-colors"
                  title="Simulate Round Skip event"
                >
                  Round Skip
                </button>
              )}
            </div>
          )}

          <button
            onClick={() => setIsExpanded(!isExpanded)}
            className="p-1 hover:bg-[#21262d] text-[#8b949e] hover:text-[#f0f6fc] rounded transition-colors cursor-pointer"
            title={isExpanded ? 'Collapse BFT Monitor' : 'Expand BFT Monitor'}
          >
            {isExpanded ? <ChevronUp className="w-3.5 h-3.5" /> : <ChevronDown className="w-3.5 h-3.5" />}
          </button>
        </div>
      </div>

      {/* KPI Highlights Bar */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 px-3 py-2 bg-[#0d1117]/80 border-b border-[#30363d]/60 text-[11px]">
        {/* Current Quorum */}
        <div className="flex items-center justify-between px-2.5 py-1.5 bg-[#161b22] border border-[#30363d] rounded">
          <span className="text-[#8b949e]">Quorum Votes:</span>
          <span className="text-[#3fb950] font-bold">
            {latestPoint.quorumParticipation}%{' '}
            <span className="text-[10px] text-[#8b949e] font-normal">
              ({latestPoint.validatingCount}/{activeValidators})
            </span>
          </span>
        </div>

        {/* Round Latency */}
        <div className="flex items-center justify-between px-2.5 py-1.5 bg-[#161b22] border border-[#30363d] rounded">
          <span className="text-[#8b949e]">Round Latency:</span>
          <span className="text-[#39c5cf] font-bold">
            {latestPoint.roundLatency} ms
          </span>
        </div>

        {/* Safety Margin */}
        <div className="flex items-center justify-between px-2.5 py-1.5 bg-[#161b22] border border-[#30363d] rounded">
          <span className="text-[#8b949e]">BFT Margin:</span>
          <span className="text-[#58a6ff] font-bold">
            +{(latestPoint.quorumParticipation - 66.7).toFixed(1)}%{' '}
            <span className="text-[9px] text-[#8b949e] font-normal">&gt; 2/3</span>
          </span>
        </div>

        {/* BFT Consensus State */}
        <div className="flex items-center justify-between px-2.5 py-1.5 bg-[#161b22] border border-[#30363d] rounded">
          <span className="text-[#8b949e]">Consensus State:</span>
          <span className="text-[#3fb950] font-bold flex items-center gap-1">
            <CheckCircle2 className="w-3 h-3 text-[#3fb950]" />
            OPTIMAL
          </span>
        </div>
      </div>

      {/* Recharts Chart Area (Rendered when expanded) */}
      {isExpanded && (
        <div className="p-2.5 bg-[#0d1117]/40">
          <div className="w-full h-40">
            <ResponsiveContainer width="100%" height="100%">
              <LineChart
                data={dataPoints}
                margin={{ top: 8, right: 16, left: -20, bottom: 2 }}
              >
                <CartesianGrid stroke="#30363d" strokeDasharray="2 3" opacity={0.4} />
                <XAxis
                  dataKey="time"
                  stroke="#8b949e"
                  fontSize={10}
                  tickLine={false}
                  interval={4}
                  fontFamily="JetBrains Mono, monospace"
                />
                
                {/* Primary Left Y-Axis */}
                <YAxis
                  yAxisId="left"
                  stroke={metricMode === 'latency' ? '#39c5cf' : '#3fb950'}
                  fontSize={10}
                  tickLine={false}
                  domain={
                    metricMode === 'latency'
                      ? [150, 420]
                      : [60, 100]
                  }
                  unit={metricMode === 'latency' ? 'ms' : '%'}
                  fontFamily="JetBrains Mono, monospace"
                />

                {/* Secondary Right Y-Axis for Dual View */}
                {metricMode === 'both' && (
                  <YAxis
                    yAxisId="right"
                    orientation="right"
                    stroke="#39c5cf"
                    fontSize={10}
                    tickLine={false}
                    domain={[150, 420]}
                    unit="ms"
                    fontFamily="JetBrains Mono, monospace"
                  />
                )}

                <Tooltip
                  content={({ active, payload, label }) => {
                    if (active && payload && payload.length) {
                      const data = payload[0].payload as BFTHealthDataPoint;
                      return (
                        <div className="bg-[#161b22] border border-[#30363d] p-2.5 rounded shadow-2xl text-xs font-mono space-y-1">
                          <div className="text-[#8b949e] border-b border-[#30363d] pb-1 flex items-center justify-between gap-4">
                            <span className="flex items-center gap-1 text-[#f0f6fc] font-bold">
                              <Clock className="w-3 h-3 text-[#39c5cf]" /> Time: {label}
                            </span>
                            <span className="text-[#3fb950] text-[10px]">Round 0 Finalized</span>
                          </div>
                          <div className="flex items-center justify-between gap-4 text-[#3fb950]">
                            <span>Quorum Participation:</span>
                            <span className="font-bold">{data.quorumParticipation}%</span>
                          </div>
                          <div className="flex items-center justify-between gap-4 text-[#8b949e] text-[11px]">
                            <span>Committed Signatures:</span>
                            <span className="text-[#f0f6fc] font-semibold">
                              {data.validatingCount} / {activeValidators} Validators
                            </span>
                          </div>
                          <div className="flex items-center justify-between gap-4 text-[#39c5cf]">
                            <span>Round Latency:</span>
                            <span className="font-bold">{data.roundLatency} ms</span>
                          </div>
                          <div className="flex items-center justify-between gap-4 text-[#58a6ff] text-[11px] pt-0.5 border-t border-[#30363d]/50">
                            <span>Quorum Threshold:</span>
                            <span>66.7% (2/3+ Supermajority)</span>
                          </div>
                        </div>
                      );
                    }
                    return null;
                  }}
                />

                {/* BFT Byzantine 2/3+ Quorum Threshold Reference Line */}
                {(metricMode === 'quorum' || metricMode === 'both') && (
                  <ReferenceLine
                    yAxisId="left"
                    y={66.7}
                    stroke="#d29922"
                    strokeDasharray="4 4"
                    strokeWidth={1.2}
                    label={{
                      value: '2/3+ BFT Threshold (66.7%)',
                      fill: '#d29922',
                      fontSize: 9,
                      position: 'insideBottomRight',
                    }}
                  />
                )}

                {/* Latency SLA Target Reference Line */}
                {metricMode === 'latency' && (
                  <ReferenceLine
                    yAxisId="left"
                    y={300}
                    stroke="#58a6ff"
                    strokeDasharray="3 3"
                    strokeWidth={1}
                    label={{
                      value: 'Target SLA (300ms)',
                      fill: '#58a6ff',
                      fontSize: 9,
                      position: 'insideTopRight',
                    }}
                  />
                )}

                {/* Quorum Participation % Line */}
                {(metricMode === 'quorum' || metricMode === 'both') && (
                  <Line
                    yAxisId="left"
                    type="monotone"
                    dataKey="quorumParticipation"
                    name="Quorum Participation %"
                    stroke="#3fb950"
                    strokeWidth={2}
                    dot={false}
                    activeDot={{
                      r: 4,
                      fill: '#3fb950',
                      stroke: '#0d1117',
                      strokeWidth: 2,
                    }}
                  />
                )}

                {/* Round Latency Line */}
                {(metricMode === 'latency' || metricMode === 'both') && (
                  <Line
                    yAxisId={metricMode === 'both' ? 'right' : 'left'}
                    type="monotone"
                    dataKey="roundLatency"
                    name="BFT Round Latency"
                    stroke="#39c5cf"
                    strokeWidth={metricMode === 'both' ? 1.5 : 2}
                    dot={false}
                    activeDot={{
                      r: 4,
                      fill: '#39c5cf',
                      stroke: '#0d1117',
                      strokeWidth: 2,
                    }}
                  />
                )}
              </LineChart>
            </ResponsiveContainer>
          </div>

          {/* Sub-Legend & Operator Summary */}
          <div className="flex flex-wrap items-center justify-between pt-1.5 px-1 text-[10px] text-[#8b949e] border-t border-[#30363d]/40">
            <div className="flex items-center gap-3">
              {(metricMode === 'quorum' || metricMode === 'both') && (
                <span className="flex items-center gap-1 text-[#3fb950]">
                  <span className="w-2 h-0.5 bg-[#3fb950]"></span>
                  Quorum Participation (Avg: {quorumStats.avg}%)
                </span>
              )}
              {(metricMode === 'latency' || metricMode === 'both') && (
                <span className="flex items-center gap-1 text-[#39c5cf]">
                  <span className="w-2 h-0.5 bg-[#39c5cf]"></span>
                  Round Latency (Avg: {latencyStats.avg}ms)
                </span>
              )}
              <span className="flex items-center gap-1 text-[#d29922]">
                <span className="w-2 h-0.5 bg-[#d29922] border-t border-dashed"></span>
                Byzantine Limit (66.7%)
              </span>
            </div>

            <div className="text-[#8b949e]">
              Deterministic 1-Block Finality • Zero Forks Recorded
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
