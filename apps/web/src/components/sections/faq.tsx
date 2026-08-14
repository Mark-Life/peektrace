"use client";

import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@workspace/ui/components/accordion";
import type { ReactNode } from "react";

/**
 * Inline monospace chip for technical terms, flags, paths, and addresses.
 */
const Chip = ({ children }: { children: ReactNode }) => (
  <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-[0.9em]">
    {children}
  </code>
);

interface FaqEntry {
  answer: ReactNode;
  question: string;
}

const FAQ_ENTRIES: readonly FaqEntry[] = [
  {
    question: "Is this a cloud service? Where does my data go?",
    answer: (
      <>
        Nowhere. The server binds <Chip>127.0.0.1</Chip> and reads your local{" "}
        <Chip>~/.claude</Chip>, <Chip>~/.codex</Chip>, and <Chip>~/.pi</Chip>{" "}
        transcripts directly. The local run log is a SQLite file, and it makes
        no network calls.
      </>
    ),
  },
  {
    question: "Which agents does it support?",
    answer: (
      <>
        Session browsing + context forensics for Claude Code, Codex, and Pi.
        Memory view/edit is Claude-only today. OpenCode identity is tracked but
        its transcripts aren&apos;t listable yet. The in-app capability matrix
        is the source of truth.
      </>
    ),
  },
  {
    question: "How do you count tokens I can't see in the transcript?",
    answer: (
      <>
        Ground-truth usage. Retained thinking is stored as empty strings but
        reported in <Chip>output_tokens</Chip>; Peektrace reconstructs it as{" "}
        <Chip>output_tokens</Chip> minus visible text/tool_use, so peak context
        adds up to the real measured size.
      </>
    ),
  },
  {
    question: "Can it modify my files?",
    answer: (
      <>
        Only Claude memories, only when you ask, and only through atomic
        compare-and-swap writes. Run with <Chip>--read-only</Chip> to make
        writes impossible at the type level.
      </>
    ),
  },
  {
    question: "Do I need Node, npm, or Bun?",
    answer: (
      <>
        No. The installer drops a single self-contained binary with the
        inspector UI baked in.
      </>
    ),
  },
  {
    question: "What does 'dumb zone' mean?",
    answer: (
      <>
        Context usage at or above ~40% of the window &mdash; the empirical band
        where model quality starts to rot even though the tokens still fit.
      </>
    ),
  },
  {
    question: "How do I report a bug?",
    answer: (
      <>
        Run <Chip>peektrace doctor</Chip>. It writes a fully redacted JSON
        support bundle to <Chip>~/.peektrace</Chip> locally &mdash; nothing is
        uploaded &mdash; then email that file to the maintainer at{" "}
        <Chip>108@mark-life.com</Chip>.
      </>
    ),
  },
];

/**
 * FAQ section: straight-answer accordion covering data locality, agent support,
 * token accounting, write safety, distribution, terminology, and bug reporting.
 */
export const Faq = () => (
  <section className="bg-muted/30 py-24" id="faq">
    <div className="mx-auto w-full max-w-6xl px-6">
      <h2 className="font-heading text-3xl md:text-4xl">Straight answers.</h2>

      <Accordion className="mt-10 max-w-3xl" collapsible type="single">
        {FAQ_ENTRIES.map((entry) => (
          <AccordionItem key={entry.question} value={entry.question}>
            <AccordionTrigger className="p-4 font-heading text-sm/relaxed">
              {entry.question}
            </AccordionTrigger>
            <AccordionContent className="px-4 text-muted-foreground text-sm/relaxed">
              {entry.answer}
            </AccordionContent>
          </AccordionItem>
        ))}
      </Accordion>
    </div>
  </section>
);
