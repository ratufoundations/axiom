export type PeerRole = 'VALIDATOR' | 'FULLNODE' | 'BOOTNODE' | 'MINER';

export interface PeerNode {
  id: string;
  shortId: string;
  locator: string;
  role: PeerRole;
  ping: number;
  lastSeen: string;
  trafficIn: number;
  trafficOut: number;
  trafficString: string;
  ip: string;
  port: number;
  lat: number;
  lng: number;
  city: string;
  country: string;
  version: string;
  syncedHeight: number;
  votingPower?: number; // Voting power percentage in BFT quorum
}

export interface BlockItem {
  height: number;
  heightFormatted: string;
  proposer: string;
  proposerShort: string;
  miner?: string;
  minerShort?: string;
  txs: number;
  sizeKb: number;
  ageSeconds: number;
  ageText: string;
  hash: string;
  reward: number;
  bftRound: number;
  quorumSigs: number; // e.g. 138 of 142
  quorumPercentage: string; // e.g. "97.2%"
}

export interface TransactionItem {
  id: string;
  from: string;
  to: string;
  displayFrom: string;
  displayTo: string;
  amount: number;
  gasFee: string;
  timeAgo: string;
  type: 'transfer' | 'contract' | 'mining_reward' | 'stake' | 'validator_reward';
  status: 'confirmed' | 'pending';
}

export interface MetricCardData {
  totalPeers: number;
  activeValidators: number;
  activeMiners?: number;
  fullNodes: number;
  blockHeight: number;
  bootnodeUptime: string;
  performanceRate: number;
  tps: number;
  mempool: number;
  bftQuorum?: string; // e.g. "97.2% (+2/3)"
  bftRound?: number;
}

export interface BFTAlert {
  id: string;
  type: 'QUORUM_DROP' | 'ROUND_SKIP' | 'QUORUM_RECOVERED' | 'BFT_WARNING';
  title: string;
  message: string;
  severity: 'warning' | 'critical' | 'info';
  timestamp: string;
  timeMs: number;
  quorumValue?: number;
  roundNumber?: number;
  autoCloseMs?: number;
}
