import { useEffect, useState } from "react";

/** Copies `text` to the clipboard and says so for a moment. */
export function CopyButton({ text, label = "Copy" }: { text: string; label?: string }) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(timer);
  }, [copied]);

  if (!navigator.clipboard) return null;
  return (
    <button
      type="button"
      className="copy-button"
      onClick={() => navigator.clipboard.writeText(text).then(() => setCopied(true), () => {})}
    >
      {copied ? "Copied" : label}
    </button>
  );
}
