"use client";

import { Button } from "@workspace/ui/components/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@workspace/ui/components/select";
import type {
  CodeBlockLanguage,
  CodeLine,
  CodeToken,
} from "@workspace/ui/lib/highlighter";
import {
  isHighlightable,
  plainLines,
  tokenizeLines,
} from "@workspace/ui/lib/highlighter";
import { cn } from "@workspace/ui/lib/utils";
import { CheckIcon, CopyIcon } from "lucide-react";
import type { ComponentProps, HTMLAttributes } from "react";
import {
  createContext,
  memo,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

/** One highlighted run of characters; class-free tokens render as body text. */
const TokenSpan = ({ token }: { token: CodeToken }) => (
  <span className={token.className ? `th-${token.className}` : undefined}>
    {token.value}
  </span>
);

// Line number styles using CSS counters
const LINE_NUMBER_CLASSES = cn(
  "block",
  "before:content-[counter(line)]",
  "before:inline-block",
  "before:[counter-increment:line]",
  "before:w-8",
  "before:mr-4",
  "before:text-right",
  "before:text-muted-foreground/50",
  "before:font-mono",
  "before:select-none"
);

/** One source line; empty lines keep a newline so blank rows retain height. */
const LineSpan = ({
  line,
  showLineNumbers,
}: {
  line: CodeLine;
  showLineNumbers: boolean;
}) => (
  <span className={showLineNumbers ? LINE_NUMBER_CLASSES : "block"}>
    {line.tokens.length === 0
      ? "\n"
      : line.tokens.map((token) => <TokenSpan key={token.key} token={token} />)}
  </span>
);

// Types
type CodeBlockProps = HTMLAttributes<HTMLDivElement> & {
  code: string;
  language: CodeBlockLanguage;
  showLineNumbers?: boolean;
};

interface CodeBlockContextType {
  code: string;
}

// Context
const CodeBlockContext = createContext<CodeBlockContextType>({
  code: "",
});

/** Tokenizes code into renderable lines, falling back to plain text when huge. */
export const highlightCode = (
  code: string,
  language: CodeBlockLanguage
): CodeLine[] =>
  isHighlightable(code, language)
    ? tokenizeLines(code, language)
    : plainLines(code);

const PRE_STYLE = {
  backgroundColor: "var(--th-background)",
  color: "var(--th-token)",
};

/** The `pre`/`code` pair, memoized so re-renders skip long token lists. */
const CodeBlockBody = memo(
  ({
    lines,
    showLineNumbers,
  }: {
    lines: CodeLine[];
    showLineNumbers: boolean;
  }) => (
    <pre className="m-0 p-4 text-sm" style={PRE_STYLE}>
      <code
        className={cn(
          "font-mono text-sm",
          showLineNumbers && "[counter-increment:line_0] [counter-reset:line]"
        )}
      >
        {lines.map((line) => (
          <LineSpan
            key={line.key}
            line={line}
            showLineNumbers={showLineNumbers}
          />
        ))}
      </code>
    </pre>
  ),
  (prevProps, nextProps) =>
    prevProps.lines === nextProps.lines &&
    prevProps.showLineNumbers === nextProps.showLineNumbers
);

CodeBlockBody.displayName = "CodeBlockBody";

export const CodeBlockContainer = ({
  className,
  language,
  style,
  ...props
}: HTMLAttributes<HTMLDivElement> & { language: string }) => (
  <div
    className={cn(
      "group relative w-full overflow-hidden rounded-md border bg-background text-foreground",
      className
    )}
    data-language={language}
    style={{
      containIntrinsicSize: "auto 200px",
      contentVisibility: "auto",
      ...style,
    }}
    {...props}
  />
);

export const CodeBlockHeader = ({
  children,
  className,
  ...props
}: HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn(
      "flex items-center justify-between border-b bg-muted/80 px-3 py-2 text-muted-foreground text-xs",
      className
    )}
    {...props}
  >
    {children}
  </div>
);

export const CodeBlockTitle = ({
  children,
  className,
  ...props
}: HTMLAttributes<HTMLDivElement>) => (
  <div className={cn("flex items-center gap-2", className)} {...props}>
    {children}
  </div>
);

export const CodeBlockFilename = ({
  children,
  className,
  ...props
}: HTMLAttributes<HTMLSpanElement>) => (
  <span className={cn("font-mono", className)} {...props}>
    {children}
  </span>
);

export const CodeBlockActions = ({
  children,
  className,
  ...props
}: HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn("-my-1 -mr-1 flex items-center gap-2", className)}
    {...props}
  >
    {children}
  </div>
);

export const CodeBlockContent = ({
  code,
  language,
  showLineNumbers = false,
}: {
  code: string;
  language: CodeBlockLanguage;
  showLineNumbers?: boolean;
}) => {
  const lines = useMemo(() => highlightCode(code, language), [code, language]);

  return (
    <div className="relative overflow-auto">
      <CodeBlockBody lines={lines} showLineNumbers={showLineNumbers} />
    </div>
  );
};

export const CodeBlock = ({
  code,
  language,
  showLineNumbers = false,
  className,
  children,
  ...props
}: CodeBlockProps) => {
  const contextValue = useMemo(() => ({ code }), [code]);

  return (
    <CodeBlockContext.Provider value={contextValue}>
      <CodeBlockContainer className={className} language={language} {...props}>
        {children}
        <CodeBlockContent
          code={code}
          language={language}
          showLineNumbers={showLineNumbers}
        />
      </CodeBlockContainer>
    </CodeBlockContext.Provider>
  );
};

export type CodeBlockCopyButtonProps = ComponentProps<typeof Button> & {
  onCopy?: () => void;
  onError?: (error: Error) => void;
  timeout?: number;
};

export const CodeBlockCopyButton = ({
  onCopy,
  onError,
  timeout = 2000,
  children,
  className,
  ...props
}: CodeBlockCopyButtonProps) => {
  const [isCopied, setIsCopied] = useState(false);
  const timeoutRef = useRef<number>(0);
  const { code } = useContext(CodeBlockContext);

  const copyToClipboard = useCallback(async () => {
    if (typeof window === "undefined" || !navigator?.clipboard?.writeText) {
      onError?.(new Error("Clipboard API not available"));
      return;
    }

    try {
      if (!isCopied) {
        await navigator.clipboard.writeText(code);
        setIsCopied(true);
        onCopy?.();
        timeoutRef.current = window.setTimeout(
          () => setIsCopied(false),
          timeout
        );
      }
    } catch (error) {
      onError?.(error as Error);
    }
  }, [code, onCopy, onError, timeout, isCopied]);

  useEffect(
    () => () => {
      window.clearTimeout(timeoutRef.current);
    },
    []
  );

  const Icon = isCopied ? CheckIcon : CopyIcon;

  return (
    <Button
      className={cn("shrink-0", className)}
      onClick={copyToClipboard}
      size="icon"
      variant="ghost"
      {...props}
    >
      {children ?? <Icon size={14} />}
    </Button>
  );
};

export type CodeBlockLanguageSelectorProps = ComponentProps<typeof Select>;

export const CodeBlockLanguageSelector = (
  props: CodeBlockLanguageSelectorProps
) => <Select {...props} />;

export type CodeBlockLanguageSelectorTriggerProps = ComponentProps<
  typeof SelectTrigger
>;

export const CodeBlockLanguageSelectorTrigger = ({
  className,
  ...props
}: CodeBlockLanguageSelectorTriggerProps) => (
  <SelectTrigger
    className={cn(
      "h-7 border-none bg-transparent px-2 text-xs shadow-none",
      className
    )}
    size="sm"
    {...props}
  />
);

export type CodeBlockLanguageSelectorValueProps = ComponentProps<
  typeof SelectValue
>;

export const CodeBlockLanguageSelectorValue = (
  props: CodeBlockLanguageSelectorValueProps
) => <SelectValue {...props} />;

export type CodeBlockLanguageSelectorContentProps = ComponentProps<
  typeof SelectContent
>;

export const CodeBlockLanguageSelectorContent = ({
  align = "end",
  ...props
}: CodeBlockLanguageSelectorContentProps) => (
  <SelectContent align={align} {...props} />
);

export type CodeBlockLanguageSelectorItemProps = ComponentProps<
  typeof SelectItem
>;

export const CodeBlockLanguageSelectorItem = (
  props: CodeBlockLanguageSelectorItemProps
) => <SelectItem {...props} />;
