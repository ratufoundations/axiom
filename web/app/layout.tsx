import type { Metadata } from 'next';
import { JetBrains_Mono } from 'next/font/google';
import './globals.css';

const jetbrainsMono = JetBrains_Mono({
  subsets: ['latin'],
  variable: '--font-mono',
});

export const metadata: Metadata = {
  title: 'RATU AURION SOVEREIGN NETWORK — Live Beacon & Explorer',
  description: 'Live Decentralized Peer Mesh, BFT Consensus, and Block Ledger Telemetry',
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" className="dark">
      <body className={`${jetbrainsMono.variable} font-mono antialiased bg-[#0d1117] text-[#c9d1d9] min-h-screen`}>
        {children}
      </body>
    </html>
  );
}
