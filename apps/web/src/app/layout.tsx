import type { Metadata } from "next";
import { DM_Sans, Geist, Geist_Mono } from "next/font/google";
import type { ReactNode } from "react";

import "@workspace/ui/globals.css";
import { cn } from "@workspace/ui/lib/utils";
import { Providers } from "@/components/providers";

const METADATA_TITLE = "Peektrace — Context-window forensics for coding agents";

const METADATA_DESCRIPTION =
  "Peektrace is a local, loopback-only CLI and inspector that reconstructs where every token in a Claude Code, Codex, Pi, or OpenCode session went — including the invisible thinking band — and scores each session Healthy, Degrading, or Rotting. One self-contained binary. Nothing leaves your machine.";

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: {
    title: METADATA_TITLE,
    description: METADATA_DESCRIPTION,
    type: "website",
    siteName: "Peektrace",
  },
  twitter: {
    card: "summary_large_image",
    title: METADATA_TITLE,
    description: METADATA_DESCRIPTION,
  },
};

const geistHeading = Geist({ subsets: ["latin"], variable: "--font-heading" });

const dmSans = DM_Sans({ subsets: ["latin"], variable: "--font-sans" });

const fontMono = Geist_Mono({
  subsets: ["latin"],
  variable: "--font-mono",
});

const RootLayout = ({
  children,
}: Readonly<{
  children: ReactNode;
}>) => (
  <html
    className={cn("font-sans", dmSans.variable, geistHeading.variable)}
    lang="en"
    suppressHydrationWarning
  >
    <body
      className={`${dmSans.variable} ${fontMono.variable} font-sans antialiased`}
    >
      <Providers>{children}</Providers>
    </body>
  </html>
);

export default RootLayout;
