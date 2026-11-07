import { AgentsCapabilities } from "@/components/sections/agents-capabilities";
import { BudgetForensics } from "@/components/sections/budget-forensics";
import { Faq } from "@/components/sections/faq";
import { Hero } from "@/components/sections/hero";
import { Install } from "@/components/sections/install";
import { MemoryForensics } from "@/components/sections/memory-forensics";
import { Problem } from "@/components/sections/problem";
import { SiteFooter } from "@/components/sections/site-footer";
import { Stats } from "@/components/sections/stats";
import { TimelineDumbzone } from "@/components/sections/timeline-dumbzone";

/** Marketing landing page composing every forensic section in narrative order. */
const Page = () => (
  <>
    <a
      className="sr-only focus:not-sr-only focus:absolute focus:top-4 focus:left-4 focus:z-50 focus:rounded-lg focus:bg-primary focus:px-4 focus:py-2 focus:text-primary-foreground"
      href="#main-content"
    >
      Skip to content
    </a>
    <main className="flex min-h-svh flex-col" id="main-content">
      <Hero />
      <Problem />
      <BudgetForensics />
      <TimelineDumbzone />
      <Stats />
      <MemoryForensics />
      <AgentsCapabilities />
      <Install />
      <Faq />
    </main>
    <SiteFooter />
  </>
);

export default Page;
